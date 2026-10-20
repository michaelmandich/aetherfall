"""Generate game models with the Meshy API, in one consistent style.

Assets to make are listed in art/meshy/requests.json; the shared style
(model versions, settings, the style text added to every prompt) is
art/meshy/style.json. Every task ever paid for is recorded in the
committed art/meshy/ledger.json.

Concept first (units, weapon props, buildings and resource nodes):

    python tools/meshy/meshy.py plan [--batch 2] [--only spearman,hall]
    python tools/meshy/meshy.py concepts --batch 2 --max-credits 200 [--candidates 3]
    python tools/meshy/meshy.py pick spearman 1        (a candidate number or a concept task id)
    python tools/meshy/meshy.py models --batch 2 --max-credits 400

1. `concepts` makes concept images, N candidates per asset (style.json
   concept.candidates, or --candidates), each its own task with one image:
   text to image, or image to image when style.json lists style references
   for the asset's kind (at most 5, sent as data URIs; they set the art
   style only). The prompt is the framing text, the subject, the faction
   (and its magic), the age, the kind's text and the global look. Images
   land in art/meshy/out/<key>/concept_<n>_<task>.png (git-ignored). An
   asset already picked or already installed gets no new candidates unless
   --only names it.
2. `pick` records the chosen concept in the ledger. No API call, no key;
   picking again replaces the pick (the next `models` makes the new one).
3. `models` makes each picked concept into a textured model (image to 3D),
   rigs it when the asset says "rig": true (humanoid units), and installs
   it: units in art/units/source/<key>.glb (the rigged GLB when rigged, with
   Meshy's basic walking and running animations beside it as
   <key>_walking.glb and <key>_running.glb), props in
   art/units/source/props/<key>.glb, buildings and nodes in
   art/static/source/<kind>/<key>.glb. Every download is also kept in
   art/meshy/out/<key>/. Assets with no pick are listed and skipped.

Text to 3D (batch 1's nodes and buildings; units and props are concept only):

    python tools/meshy/meshy.py make --max-credits 400 [--only ...]
    python tools/meshy/meshy.py preview --candidates 3 --max-credits 200 --only house
    python tools/meshy/meshy.py refine house <preview-task-id> --max-credits 20

`make` makes (mesh + texture) and installs every model not made yet;
`preview` only makes candidate meshes (pick one from their thumbnails in
art/meshy/out/, then `refine` it: the preview must be one of that same
asset's).

    python tools/meshy/meshy.py status
    python tools/meshy/meshy.py balance

Every paid command takes --max-credits: an asset's whole remaining cost is
reserved before its first task starts, and nothing starts beyond the cap.
The account's task queue is respected (at most `queue_limit` tasks in
flight, from style.json; a full queue is waited out), with later stages
started first (rigs, then models, then concepts; textures before meshes)
so it stays full. Every task is recorded in the ledger as soon as it
starts and after every change, keyed by a hash of exactly what was asked,
so an interrupted run resumes instead of paying twice, and a made model
whose installed file was deleted is installed again without paying.

An asset's optional "retired" list in requests.json names variants that
were made once and taken out of the game: they are never made, previewed
or refined again.

The API key comes from the MESHY_API_KEY environment variable or the
git-ignored .env file at the repo root. It is never printed.
"""

from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import shutil
import sys
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from http.client import HTTPException
from pathlib import Path
from typing import Callable

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / "art" / "meshy"
LEDGER = ART / "ledger.json"
OUT = ART / "out"
SOURCE = ROOT / "art" / "static" / "source"
UNITS = ROOT / "art" / "units" / "source"
API = "https://api.meshy.ai"
TEXT_TO_3D = "/openapi/v2/text-to-3d"
TEXT_TO_IMAGE = "/openapi/v1/text-to-image"
IMAGE_TO_IMAGE = "/openapi/v1/image-to-image"
IMAGE_TO_3D = "/openapi/v1/image-to-3d"
RIGGING = "/openapi/v1/rigging"

POLL_SECONDS = 8.0
TASK_TIMEOUT_SECONDS = 45 * 60
IN_FLIGHT = ("PENDING", "IN_PROGRESS")
FINAL_FAILURES = ("FAILED", "CANCELED", "EXPIRED")

KINDS = ("nodes", "buildings", "units", "props")
TEXT_TO_3D_KINDS = ("nodes", "buildings")
FRAMINGS = ("character", "object", "prop")
ANIMATIONS = ("walking", "running")  # Meshy's basic animations, installed beside a rigged unit
MAX_REFERENCES = 5
MAX_REFERENCE_BYTES = 20 * 1024 * 1024
MAX_CONCEPT_PROMPT = 1800  # no documented limit; a longer prompt is a mistake
IMAGE_TYPES = {".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg", ".webp": "image/webp"}
STYLE_ONLY = "The reference images show the target art style and rendering only: do not copy their subjects, shapes or composition."


# Credits per stage (docs.meshy.ai/en/api/pricing): Meshy-6 previews cost
# 20, other models 5; texturing 10 at 2k/4k and 15 at 8k.
def preview_cost(ai_model: str) -> int:
    return 20 if ai_model.startswith("meshy-6") or ai_model in ("latest", "meshy-7.1") else 5


def refine_cost(resolution: str) -> int:
    return 15 if resolution == "8k" else 10


# One image: GPT Image models (gpt-image-2*) cost 9 from text, 12 from images.
IMAGE_COST = {"nano-banana": 3, "nano-banana-2": 6, "nano-banana-2-lite": 3, "nano-banana-pro": 9}
NANO_ASPECTS = ("1:1", "16:9", "9:16", "4:3", "3:4")
GPT_ASPECTS = NANO_ASPECTS + ("3:2", "2:3")
# Image to 3D, textured: 30 at 2k/4k, 35 at 8k (no geometry_resolution).
MODEL_AI = ("meshy-6", "meshy-7.1", "latest")
RIG_COST = 5


def image_cost(path: str, ai_model: str, posed: bool) -> int:
    if ai_model.startswith("gpt-image-2"):
        return 12 if path == IMAGE_TO_IMAGE else 9
    if ai_model not in IMAGE_COST:
        raise MeshyError(f"style.json concept.ai_model {ai_model!r}: meshy.py does not know its price (known: {', '.join(IMAGE_COST)}, gpt-image-2*)")
    # A pose preset switches nano-banana to nano-banana-pro (docs: text to image).
    return IMAGE_COST["nano-banana-pro" if posed and ai_model == "nano-banana" else ai_model]


def model_cost(resolution: str) -> int:
    return 35 if resolution == "8k" else 30


class MeshyError(Exception):
    pass


class QueueFull(MeshyError):
    """The account already has as many tasks in flight as its plan allows."""


# --------------------------------------------------------------------------
# Config
# --------------------------------------------------------------------------

@dataclass(frozen=True)
class Asset:
    id: str
    kind: str  # "nodes" | "buildings" | "units" | "props"
    variant: str  # "" for a single model
    prompt: str
    # Per-asset style overrides (e.g. palms are not a northern forest).
    prompt_suffix: str | None = None
    texture_suffix: str | None = None
    batch: str | None = None
    age: int | None = None
    faction: str | None = None
    magic: bool = False
    rig: bool = False  # a humanoid to rig after its model
    height_m: float = 1.7  # the rigged character's height
    polycount: int | None = None  # None: style.json polycount[kind]
    framing: str | None = None  # None: character if rig, prop for props, else object

    def __post_init__(self):
        if self.framing is None:
            object.__setattr__(self, "framing", "character" if self.rig else "prop" if self.kind == "props" else "object")

    @property
    def key(self) -> str:
        return f"{self.id}_{self.variant}" if self.variant else self.id

    def target(self) -> Path:
        if self.kind == "units":
            return UNITS / f"{self.key}.glb"
        if self.kind == "props":
            return UNITS / "props" / f"{self.key}.glb"
        return SOURCE / self.kind / f"{self.key}.glb"

    def animation_target(self, name: str) -> Path:
        return self.target().with_name(f"{self.key}_{name}.glb")


def rel(path: Path) -> str:
    return path.relative_to(ROOT).as_posix()


def load_style() -> dict:
    return json.loads((ART / "style.json").read_text(encoding="utf-8"))


ASSET_FIELDS = ("id", "kind", "prompt", "batch", "age", "faction", "magic", "rig", "height_m", "polycount",
                "framing", "variants", "retired", "prompt_suffix", "texture_suffix")


def is_int(v) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def check_asset(a, style: dict) -> None:
    """Refuse a malformed requests.json entry, before anything is paid."""
    if not isinstance(a, dict) or not isinstance(a.get("id"), str) or not a["id"]:
        raise MeshyError(f"requests.json: every asset needs a non-empty \"id\" (got {str(a)[:80]})")

    def bad(what: str):
        raise MeshyError(f"{a['id']}: {what} (art/meshy/requests.json)")

    unknown = sorted(k for k in a if k not in ASSET_FIELDS and not k.startswith("_"))
    if unknown:
        bad(f"unknown field {', '.join(unknown)} (known: {', '.join(ASSET_FIELDS)})")
    if a.get("kind") not in KINDS:
        bad(f"kind must be one of {', '.join(KINDS)}, not {a.get('kind')!r}")
    if not isinstance(a.get("prompt"), str) or not a["prompt"].strip():
        bad("prompt must describe the subject")
    if "batch" in a and not ((isinstance(a["batch"], str) and a["batch"]) or is_int(a["batch"])):
        bad(f"batch must be a string such as \"2\", not {a['batch']!r}")
    if "age" in a:
        if not is_int(a["age"]) or not 1 <= a["age"] <= 5:
            bad(f"age must be a whole number from 1 to 5, not {a['age']!r}")
        if not isinstance((style.get("ages") or {}).get(str(a["age"])), str):
            bad(f"age {a['age']} has no text in style.json ages")
    factions = style.get("factions") or {}
    if "faction" in a and not (isinstance(a["faction"], str) and isinstance(factions.get(a["faction"]), str)):
        bad(f"faction {a['faction']!r} is not in style.json factions ({', '.join(factions) or 'none'})")
    for flag in ("magic", "rig"):
        if flag in a and not isinstance(a[flag], bool):
            bad(f"{flag} must be true or false, not {a[flag]!r}")
    if a.get("magic"):
        if "faction" not in a:
            bad("magic needs a faction (style.json magic is per faction)")
        if not isinstance((style.get("magic") or {}).get(a["faction"]), str):
            bad(f"magic: faction {a['faction']} has no text in style.json magic")
    if "framing" in a and a["framing"] not in FRAMINGS:
        bad(f"framing must be one of {', '.join(FRAMINGS)}, not {a['framing']!r}")
    if a.get("rig"):
        if a["kind"] != "units":
            bad(f"only units can be rigged (rigging is for humanoid characters), not {a['kind']}")
        if a.get("framing", "character") != "character":
            bad("a rigged unit needs framing character (its concept in an A-pose)")
    if "height_m" in a and not (isinstance(a["height_m"], (int, float)) and not isinstance(a["height_m"], bool) and a["height_m"] > 0):
        bad(f"height_m must be a positive number of metres, not {a['height_m']!r}")
    if "polycount" in a and not (is_int(a["polycount"]) and 100 <= a["polycount"] <= 300_000):
        bad(f"polycount must be a whole number from 100 to 300000, not {a['polycount']!r}")
    variants = a.get("variants")
    if variants is not None and not (isinstance(variants, dict) and all(isinstance(k, str) and k and isinstance(v, str) for k, v in variants.items())):
        bad("variants must map variant names to extra descriptions")
    for text in ("prompt_suffix", "texture_suffix"):
        if text in a and not isinstance(a[text], str):
            bad(f"{text} must be text")


def load_assets(only: set[str] | None = None, batch: str | None = None, style: dict | None = None) -> list[Asset]:
    style = load_style() if style is None else style
    spec = json.loads((ART / "requests.json").read_text(encoding="utf-8"))
    if not isinstance(spec.get("assets"), list):
        raise MeshyError("requests.json: \"assets\" must be a list")
    assets, names = [], set()
    for a in spec["assets"]:
        check_asset(a, style)
        variants = a.get("variants") or {"": a["prompt"]}
        retired = a.get("retired", [])
        if not isinstance(retired, list) or not all(isinstance(r, str) and r for r in retired):
            raise MeshyError(f"{a['id']}: retired must be a list of variant names")
        retired = set(retired)
        unknown = retired - set(variants)
        if unknown:
            raise MeshyError(f"{a['id']}: retired {', '.join(sorted(unknown))} is not a variant")
        for variant, extra in variants.items():
            prompt = a["prompt"] if not variant else f"{a['prompt']}, {extra}"
            asset = Asset(a["id"], a["kind"], variant, prompt, a.get("prompt_suffix"), a.get("texture_suffix"),
                          batch=str(a["batch"]) if "batch" in a else None, age=a.get("age"), faction=a.get("faction"),
                          magic=a.get("magic", False), rig=a.get("rig", False), height_m=float(a.get("height_m", 1.7)),
                          polycount=a.get("polycount"), framing=a.get("framing"))
            # A name is a ledger history and an installed file: two assets
            # sharing one would pay twice and overwrite each other.
            if asset.key in names:
                raise MeshyError(f"{asset.key}: two assets in requests.json have this name")
            names.add(asset.key)
            if variant in retired:  # made once, taken out of the game: never made again
                continue
            if (only is None or a["id"] in only or asset.key in only) and (batch is None or asset.batch == batch):
                assets.append(asset)
    return assets


def refine_target(assets: list[Asset], name: str) -> Asset:
    """The one asset `refine <name>` or `pick <name>` means: the asset whose
    id_variant is `name`, or the only live variant of the id `name`.
    Retired variants are not in `assets`, so they are never made again."""
    matches = [a for a in assets if a.key == name] or [a for a in assets if a.id == name]
    if len(matches) > 1:
        raise MeshyError(f"{name} could be {', '.join(a.key for a in matches)}: name one")
    if not matches:
        raise MeshyError(f"{name}: no such asset in art/meshy/requests.json (retired variants are never made again)")
    return matches[0]


def text_to_3d_assets(assets: list[Asset]) -> list[Asset]:
    """The assets text to 3D makes: units and props are concept only."""
    others = [a.key for a in assets if a.kind not in TEXT_TO_3D_KINDS]
    if others:
        print(f"skipping {', '.join(others)}: units and props are made with concepts, pick and models")
    return [a for a in assets if a.kind in TEXT_TO_3D_KINDS]


def polycount(asset: Asset, style: dict) -> int:
    if asset.polycount is not None:
        return asset.polycount
    counts = style.get("polycount") or {}
    if asset.kind not in counts:
        raise MeshyError(f"{asset.key}: style.json polycount has no {asset.kind} (or give the asset a polycount)")
    return counts[asset.kind]


def prompt_suffix(asset: Asset, style: dict) -> str:
    return asset.prompt_suffix or style["prompt_suffix"][asset.kind]


def texture_suffix(asset: Asset, style: dict) -> str:
    return asset.texture_suffix or style["texture_suffix"][asset.kind]


def full_prompt(asset: Asset, style: dict) -> str:
    text = f"{asset.prompt}. {prompt_suffix(asset, style)}"
    if len(text) > 800:
        raise MeshyError(f"{asset.key}: prompt is {len(text)} characters (max 800)")
    return text


def prompt_hash(*parts: str) -> str:
    return hashlib.sha256("\x1f".join(parts).encode("utf-8")).hexdigest()[:16]


def preview_hash(asset: Asset, style: dict) -> str:
    return prompt_hash(full_prompt(asset, style), style["ai_model"], str(polycount(asset, style)))


def refine_hash(asset: Asset, style: dict, preview_id: str) -> str:
    return prompt_hash(preview_id, texture_suffix(asset, style), style["texture_resolution"], str(style["enable_pbr"]))


def preview_body(asset: Asset, style: dict) -> dict:
    return {
        "mode": "preview",
        "prompt": full_prompt(asset, style),
        "ai_model": style["ai_model"],
        "should_remesh": True,
        "topology": "triangle",
        "target_polycount": polycount(asset, style),
        "origin_at": "bottom",
        "target_formats": ["glb"],
    }


def refine_body(preview_id: str, asset: Asset, style: dict) -> dict:
    return {
        "mode": "refine",
        "preview_task_id": preview_id,
        "enable_pbr": style["enable_pbr"],
        "texture_resolution": style["texture_resolution"],
        "remove_lighting": style["remove_lighting"],
        "target_formats": ["glb"],
        "texture_prompt": f"{asset.prompt}. {texture_suffix(asset, style)}"[:800],
    }


# --------------------------------------------------------------------------
# Concept-first config: concept images, image to 3D, rigging
# --------------------------------------------------------------------------

def concept_style(style: dict) -> dict:
    """style.json's concept section and the texts concept prompts use, checked."""
    c = style.get("concept")
    if not isinstance(c, dict):
        raise MeshyError("style.json has no concept section (concepts need concept.ai_model, candidates and aspect)")
    model = c.get("ai_model")
    if not isinstance(model, str):
        raise MeshyError("style.json concept.ai_model must name an image model, such as nano-banana-pro")
    image_cost(TEXT_TO_IMAGE, model, False)  # a model whose price is known
    if not is_int(c.get("candidates", 2)) or c.get("candidates", 2) < 1:
        raise MeshyError("style.json concept.candidates must be a whole number of at least 1")
    allowed = GPT_ASPECTS if model.startswith("gpt-image-2") else NANO_ASPECTS
    aspect = c.get("aspect")
    if not isinstance(aspect, dict) or any(aspect.get(f) not in allowed for f in FRAMINGS):
        raise MeshyError(f"style.json concept.aspect must give {', '.join(FRAMINGS)} each one of {', '.join(allowed)} ({model})")
    if not isinstance(c.get("remove_background", True), bool):
        raise MeshyError("style.json concept.remove_background must be true or false")
    framing = style.get("framing")
    if not isinstance(framing, dict) or not all(isinstance(framing.get(f), str) and framing[f].strip() for f in FRAMINGS):
        raise MeshyError(f"style.json framing must give a text for each of {', '.join(FRAMINGS)}")
    if not isinstance(style.get("look", ""), str):
        raise MeshyError("style.json look must be text")
    for section in ("ages", "factions", "magic", "kinds"):
        texts = style.get(section, {})
        if not isinstance(texts, dict) or not all(isinstance(v, str) for v in texts.values()):
            raise MeshyError(f"style.json {section} must map names to texts")
    return c


def model_style(style: dict) -> dict:
    """style.json's model section (image to 3D), checked."""
    m = style.get("model")
    if not isinstance(m, dict):
        raise MeshyError("style.json has no model section (models need model.ai_model, texture_resolution, remove_lighting and enable_pbr)")
    if m.get("ai_model") not in MODEL_AI:
        raise MeshyError(f"style.json model.ai_model must be one of {', '.join(MODEL_AI)} (meshy.py knows their price), not {m.get('ai_model')!r}")
    if m.get("texture_resolution") not in ("2k", "4k", "8k"):
        raise MeshyError("style.json model.texture_resolution must be 2k, 4k or 8k")
    for flag in ("remove_lighting", "enable_pbr", "image_enhancement"):
        if not isinstance(m.get(flag, True), bool) or (flag != "image_enhancement" and flag not in m):
            raise MeshyError(f"style.json model.{flag} must be true or false")
    return m


def sentence(text: str) -> str:
    text = " ".join(text.split())
    return text if text[-1] in ".!?" else text + "."


def style_text(style: dict, section: str, name: str, asset: Asset) -> str:
    text = (style.get(section) or {}).get(name)
    if not isinstance(text, str):
        raise MeshyError(f"{asset.key}: style.json {section} has no {name}")
    return text


def concept_prompt(asset: Asset, style: dict, references: bool) -> str:
    """Framing, subject, faction (and its magic), age, kind and look, as
    sentences. The asset's prompt_suffix, as for text to 3D, replaces the
    shared text of its kind (a desert palm is not a northern forest)."""
    parts = [STYLE_ONLY] if references else []
    parts += [style_text(style, "framing", asset.framing, asset), f"Subject: {asset.prompt}"]
    if asset.faction:
        parts.append(style_text(style, "factions", asset.faction, asset))
        if asset.magic:
            parts.append(style_text(style, "magic", asset.faction, asset))
    if asset.age:
        parts.append(style_text(style, "ages", str(asset.age), asset))
    parts.append(asset.prompt_suffix or (style.get("kinds") or {}).get(asset.kind, ""))
    parts.append(style.get("look", ""))
    text = " ".join(sentence(p) for p in parts if p and p.strip())
    if len(text) > MAX_CONCEPT_PROMPT:
        raise MeshyError(f"{asset.key}: concept prompt is {len(text)} characters (max {MAX_CONCEPT_PROMPT}); shorten its prompt or the style texts")
    return text


def reference_files(kind: str, style: dict) -> list[Path]:
    """The style reference images for a kind (style.json references), checked."""
    refs = (style.get("references") or {}).get(kind) or []
    if not isinstance(refs, list) or not all(isinstance(r, str) and r for r in refs):
        raise MeshyError(f"style.json references.{kind} must be a list of image paths")
    if len(refs) > MAX_REFERENCES:
        raise MeshyError(f"style.json references.{kind} lists {len(refs)} images; image to image takes at most {MAX_REFERENCES}")
    paths = []
    for r in refs:
        p = ROOT / r
        if p.suffix.lower() not in IMAGE_TYPES:
            raise MeshyError(f"style.json references.{kind}: {r} is not a {', '.join(IMAGE_TYPES)} image")
        if not p.is_file():
            raise MeshyError(f"style.json references.{kind}: {r} does not exist")
        if p.stat().st_size > MAX_REFERENCE_BYTES:
            raise MeshyError(f"style.json references.{kind}: {r} is over 20 MB")
        paths.append(p)
    return paths


_digests: dict[tuple, str] = {}


def file_digest(path: Path) -> str:
    st = path.stat()
    key = (str(path), st.st_mtime_ns, st.st_size)
    if key not in _digests:
        _digests[key] = hashlib.sha256(path.read_bytes()).hexdigest()
    return _digests[key]


def data_uri(path: Path) -> str:
    return f"data:{IMAGE_TYPES[path.suffix.lower()]};base64,{base64.b64encode(path.read_bytes()).decode('ascii')}"


@dataclass(frozen=True)
class ConceptRequest:
    """One concept image task: where it goes, what it asks, what it costs."""

    path: str  # TEXT_TO_IMAGE or IMAGE_TO_IMAGE
    body: dict  # without the reference images, which are only read to send
    references: tuple[Path, ...]
    hash: str
    cost: int

    def full_body(self) -> dict:
        if not self.references:
            return dict(self.body)
        return {**self.body, "reference_image_urls": [data_uri(p) for p in self.references]}


def concept_request(asset: Asset, style: dict, candidate: int) -> ConceptRequest:
    c = concept_style(style)
    refs = reference_files(asset.kind, style)
    path = IMAGE_TO_IMAGE if refs else TEXT_TO_IMAGE
    body = {
        "ai_model": c["ai_model"],
        "prompt": concept_prompt(asset, style, bool(refs)),
        "aspect_ratio": c["aspect"][asset.framing],
        "remove_background": c.get("remove_background", True),
    }
    if not refs and asset.framing == "character":
        body["pose_mode"] = "a-pose"  # not documented for image to image
    # The same prompt, model, aspect, references (their bytes) and candidate
    # number is the same concept: a rerun finds it instead of paying again.
    phash = prompt_hash(path, json.dumps(body, sort_keys=True), *(file_digest(p) for p in refs), str(candidate))
    return ConceptRequest(path, body, tuple(refs), phash, image_cost(path, c["ai_model"], "pose_mode" in body))


def model_body(asset: Asset, style: dict, concept_id: str) -> dict:
    """Image to 3D from a concept task. No texture_prompt: the texture
    follows the concept image (and a texture prompt costs extra)."""
    m = model_style(style)
    body = {
        "input_task_id": concept_id,
        "ai_model": m["ai_model"],
        "should_texture": True,
        "enable_pbr": m["enable_pbr"],
        "texture_resolution": m["texture_resolution"],
        "remove_lighting": m["remove_lighting"],
        "should_remesh": True,
        "topology": "triangle",
        "target_polycount": polycount(asset, style),
        "target_formats": ["glb"],
        "image_enhancement": m.get("image_enhancement", True),
    }
    if asset.framing == "character":
        body["pose_mode"] = "a-pose"
    return body


def model_hash(asset: Asset, style: dict, concept_id: str) -> str:
    return prompt_hash("image-to-3d", json.dumps(model_body(asset, style, concept_id), sort_keys=True))


def rig_body(asset: Asset, model_id: str) -> dict:
    return {"input_task_id": model_id, "height_meters": asset.height_m}


def rig_hash(asset: Asset, model_id: str) -> str:
    return prompt_hash("rigging", json.dumps(rig_body(asset, model_id), sort_keys=True))


# --------------------------------------------------------------------------
# Secrets and HTTP
# --------------------------------------------------------------------------

def api_key() -> str:
    key = os.environ.get("MESHY_API_KEY", "").strip()
    env = ROOT / ".env"
    if not key and env.exists():
        for line in env.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line.startswith("MESHY_API_KEY="):
                key = line.split("=", 1)[1].strip().strip('"').strip("'")
    if not key:
        raise MeshyError("no MESHY_API_KEY (set it in the environment or in .env at the repo root)")
    return key


Http = Callable[[str, str, dict | None], dict]


def make_http(key: str, sleep: Callable[[float], None] = time.sleep) -> Http:
    def call(method: str, path: str, body: dict | None) -> dict:
        data = json.dumps(body).encode("utf-8") if body is not None else None
        req = urllib.request.Request(API + path, data=data, method=method)
        req.add_header("Authorization", "Bearer " + key)
        if data is not None:
            req.add_header("Content-Type", "application/json")
        last_error = "no response"
        for attempt in range(6):
            try:
                with urllib.request.urlopen(req, timeout=60) as r:
                    return json.load(r)
            except urllib.error.HTTPError as e:
                detail = e.read()[:500].decode("utf-8", "replace").replace(key, "<key>")
                if e.code == 429 and ("NoMorePendingTasks" in detail or "NoMoreConcurrentTasks" in detail):
                    # A full task queue: retrying now cannot help; wait for a task to finish.
                    raise QueueFull(f"{method} {path}: the account's task queue is full") from None
                if e.code == 429 or e.code >= 500:
                    last_error = f"HTTP {e.code}: {detail[:120]}"
                    retry_after = e.headers.get("Retry-After") if e.headers else None
                    sleep(float(retry_after) if retry_after else 2 ** attempt * 2)
                    continue
                hint = {401: " (the API key was rejected)", 402: " (not enough credits)"}.get(e.code, "")
                raise MeshyError(f"{method} {path}: HTTP {e.code}{hint}: {detail}") from None
            except urllib.error.URLError as e:
                last_error = str(e.reason)
                sleep(2 ** attempt * 2)
        raise MeshyError(f"{method} {path}: gave up after retries ({last_error})")

    return call


def download(url: str, to: Path) -> None:
    to.parent.mkdir(parents=True, exist_ok=True)
    tmp = to.with_suffix(to.suffix + ".part")
    try:
        with urllib.request.urlopen(url, timeout=300) as r, open(tmp, "wb") as f:
            shutil.copyfileobj(r, f)
    # URLError and HTTPError (e.g. an expired link) are OSErrors; a
    # connection dropped mid-file is an HTTPException (IncompleteRead).
    except (OSError, HTTPException) as e:
        tmp.unlink(missing_ok=True)
        raise MeshyError(f"downloading {to.name} failed ({e})") from None
    tmp.replace(to)


# --------------------------------------------------------------------------
# Ledger
# --------------------------------------------------------------------------

class Ledger:
    """Every task ever submitted: what it was for, its state and its cost."""

    def __init__(self, path: Path = LEDGER):
        self.path = path
        self.tasks: list[dict] = json.loads(path.read_text(encoding="utf-8"))["tasks"] if path.exists() else []

    def save(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp = self.path.with_suffix(".tmp")
        tmp.write_text(json.dumps({"tasks": self.tasks}, indent=2) + "\n", encoding="utf-8")
        tmp.replace(self.path)

    def add(self, **entry) -> dict:
        self.tasks.append(entry)
        self.save()
        return entry

    def get(self, task_id: str) -> dict:
        return next(t for t in self.tasks if t["task_id"] == task_id)

    def update(self, task_id: str, **fields) -> dict:
        t = self.get(task_id)
        t.update(fields)
        self.save()
        return t

    def find(self, key: str, stage: str, phash: str) -> list[dict]:
        return [t for t in self.tasks if t["asset"] == key and t["stage"] == stage and t["hash"] == phash and t["status"] not in FINAL_FAILURES]

    def spent(self) -> int:
        return sum(t.get("credits", 0) or 0 for t in self.tasks)

    def picked(self, key: str) -> dict | None:
        """The asset's picked concept task, if any."""
        return next((t for t in reversed(self.tasks) if t["asset"] == key and t["stage"] == "concept" and t.get("picked") and t["status"] == "SUCCEEDED"), None)

    def pick(self, task_id: str) -> dict:
        """Make a concept task its asset's pick (the only one)."""
        chosen = self.get(task_id)
        for t in self.tasks:
            if t["asset"] == chosen["asset"] and t["stage"] == "concept":
                t.pop("picked", None)
        chosen["picked"] = True
        self.save()
        return chosen


class Budget:
    def __init__(self, cap: int):
        self.cap = cap
        self.used = 0

    def can(self, credits: int) -> bool:
        return self.used + credits <= self.cap

    def take(self, credits: int, what: str) -> None:
        if not self.can(credits):
            raise MeshyError(f"stopping before {what}: it would take {self.used + credits} credits, over --max-credits {self.cap}")
        self.used += credits


# --------------------------------------------------------------------------
# Jobs: chains of paid tasks
# --------------------------------------------------------------------------

@dataclass(frozen=True)
class Stage:
    """One kind of paid task in a job's chain. The job holds its task (the
    ledger entry) in the attribute named `name`, which is also the ledger's
    stage. When the queue has room, higher ranks start first."""

    name: str
    word: str  # in messages
    rank: int
    verb: str = "installed"  # what finishing the job's last stage does


PREVIEW = Stage("preview", "mesh", 0)
REFINE = Stage("refine", "texture", 1)
CONCEPT = Stage("concept", "concept", 0, "downloaded")
MODEL = Stage("model", "model", 1)
RIG = Stage("rig", "rig", 2)


class Pipeline:
    """What the scheduler needs from a job beyond its own stages: each
    subclass gives `stages`, `label`, `path`, `stage_cost`, `request`,
    `finish` (after a task succeeds; the last stage sets `done`) and
    `from_disk` (install the last stage again from art/meshy/out/)."""

    def task(self, stage: Stage) -> dict | None:
        return getattr(self, stage.name)

    @property
    def final(self) -> Stage:
        return self.stages[-1]

    @property
    def finished(self) -> bool:
        return self.error is not None or self.done is not None

    def next_stage(self) -> Stage | None:
        """The first stage with no task yet, once the stage before it succeeded."""
        before = None
        for stage in self.stages:
            task = self.task(stage)
            if task is None:
                return stage if before is None or before["status"] == "SUCCEEDED" else None
            before = task
        return None

    def cost(self, style: dict) -> int:
        """Credits this job still has to pay."""
        return sum(self.stage_cost(s, style) for s in self.stages if self.task(s) is None)

    def result_fields(self, stage: Stage, result: dict) -> dict:
        return {"thumbnail": unsigned(result.get("thumbnail_url"))}

    def needs_recovery(self) -> bool:
        task = self.task(self.final)
        return not self.finished and task is not None and task["status"] == "SUCCEEDED"

    def recover(self, http: Http, ledger: Ledger) -> None:
        """Install the last task when it finished on an earlier run but its
        installed file is gone (deleted or moved), without paying again:
        from its download in art/meshy/out/ if that is still there, else
        fetched again. A task that cannot be fetched any more is the job's
        error."""
        stage = self.final
        task_id = self.task(stage)["task_id"]
        try:
            if not self.from_disk(ledger):
                result = http("GET", f"{self.path(stage)}/{task_id}", None)
                if result.get("status") != "SUCCEEDED":
                    raise MeshyError(f"it is now {result.get('status')}")
                self.finish(stage, result, ledger)
            print(f"{self.label}: re{stage.verb} {rel(self.done)} from {stage.word} task {task_id} (no new credits)", flush=True)
        except MeshyError as e:
            self.error = f"{self.label}: {stage.word} task {task_id} finished on an earlier run but cannot be {stage.verb} again: {e}"


@dataclass
class Job(Pipeline):
    """One text-to-3D model: its preview (mesh) task, then its refine
    (texture) task; `texture` False stops after the mesh (candidates)."""

    asset: Asset
    candidate: int = 0
    texture: bool = True
    preview: dict | None = None
    refine: dict | None = None
    done: Path | None = None
    error: str | None = None
    printed: dict = field(default_factory=dict)

    @property
    def label(self) -> str:
        return self.asset.key if self.texture else f"{self.asset.key}#{self.candidate}"

    @property
    def stages(self) -> tuple[Stage, ...]:
        return (PREVIEW, REFINE) if self.texture else (PREVIEW,)

    @property
    def finished(self) -> bool:
        return self.error is not None or self.done is not None or (not self.texture and self.preview is not None and self.preview["status"] == "SUCCEEDED")

    def path(self, stage: Stage) -> str:
        return TEXT_TO_3D

    def stage_cost(self, stage: Stage, style: dict) -> int:
        return preview_cost(style["ai_model"]) if stage == PREVIEW else refine_cost(style["texture_resolution"])

    def request(self, stage: Stage, style: dict) -> tuple[dict, str, dict]:
        if stage == PREVIEW:
            return preview_body(self.asset, style), preview_hash(self.asset, style), {"candidate": self.candidate}
        mesh = self.preview["task_id"]
        return refine_body(mesh, self.asset, style), refine_hash(self.asset, style, mesh), {"preview": mesh}

    def finish(self, stage: Stage, result: dict, ledger: Ledger) -> None:
        if stage == PREVIEW:
            thumbnail(self, result, OUT / self.asset.key / f"preview_{self.candidate}_{self.preview['task_id']}.png")
        else:
            self.done = install(self, result, ledger)

    def from_disk(self, ledger: Ledger) -> bool:
        raw = OUT / self.asset.key / f"{self.refine['task_id']}.glb"
        if not raw.exists():
            return False
        self.done = place(self, raw, ledger)
        return True


@dataclass
class ConceptJob(Pipeline):
    """One concept image candidate: one text-to-image (or image-to-image)
    task with one image, downloaded to look at and pick from."""

    asset: Asset
    candidate: int
    req: ConceptRequest
    concept: dict | None = None
    done: Path | None = None
    error: str | None = None
    printed: dict = field(default_factory=dict)
    stages = (CONCEPT,)

    @property
    def label(self) -> str:
        return f"{self.asset.key}#{self.candidate}"

    def image(self) -> Path:
        return OUT / self.asset.key / f"concept_{self.candidate}_{self.concept['task_id']}.png"

    def path(self, stage: Stage) -> str:
        return self.req.path

    def stage_cost(self, stage: Stage, style: dict) -> int:
        return self.req.cost

    def request(self, stage: Stage, style: dict) -> tuple[dict, str, dict]:
        fields = {"candidate": self.candidate, "api": self.req.path.rsplit("/", 1)[-1], "ai_model": self.req.body["ai_model"]}
        if self.req.references:
            fields["references"] = [rel(p) for p in self.req.references]
        return self.req.full_body(), self.req.hash, fields

    def result_fields(self, stage: Stage, result: dict) -> dict:
        urls = result.get("image_urls") or []
        return {"image": unsigned(urls[0]) if urls else None}

    def finish(self, stage: Stage, result: dict, ledger: Ledger) -> None:
        urls = result.get("image_urls") or []
        if not urls:
            raise MeshyError("the finished task has no image")
        download(urls[0], self.image())
        self.done = self.image()

    def from_disk(self, ledger: Ledger) -> bool:
        if not self.image().exists():
            return False
        self.done = self.image()
        return True


@dataclass
class ModelJob(Pipeline):
    """One model from a picked concept: its image-to-3D task, then (for a
    rigged unit) its rigging task, then installed."""

    asset: Asset
    concept: dict  # the picked concept task
    model: dict | None = None
    rig: dict | None = None
    done: Path | None = None
    error: str | None = None
    printed: dict = field(default_factory=dict)

    @property
    def label(self) -> str:
        return self.asset.key

    @property
    def stages(self) -> tuple[Stage, ...]:
        return (MODEL, RIG) if self.asset.rig else (MODEL,)

    def path(self, stage: Stage) -> str:
        return IMAGE_TO_3D if stage == MODEL else RIGGING

    def stage_cost(self, stage: Stage, style: dict) -> int:
        return model_cost(model_style(style)["texture_resolution"]) if stage == MODEL else RIG_COST

    def request(self, stage: Stage, style: dict) -> tuple[dict, str, dict]:
        if stage == MODEL:
            concept = self.concept["task_id"]
            return model_body(self.asset, style, concept), model_hash(self.asset, style, concept), {"concept": concept}
        model = self.model["task_id"]
        return rig_body(self.asset, model), rig_hash(self.asset, model), {"model": model, "height_m": self.asset.height_m}

    def result_fields(self, stage: Stage, result: dict) -> dict:
        return super().result_fields(stage, result) if stage == MODEL else {}

    def raw(self, stage: Stage, suffix: str = "") -> Path:
        return OUT / self.asset.key / f"{stage.name}_{self.task(stage)['task_id']}{suffix}.glb"

    def made(self) -> bool:
        """Installed from this job's last task, and still there."""
        task = self.task(self.final)
        if task is None or task["status"] != "SUCCEEDED" or task.get("installed") != rel(self.asset.target()) or not self.asset.target().exists():
            return False
        return all((ROOT / a["installed"]).exists() for a in (task.get("animations") or {}).values())

    def finish(self, stage: Stage, result: dict, ledger: Ledger) -> None:
        if stage == MODEL:
            glb = (result.get("model_urls") or {}).get("glb")
            raw = self.raw(MODEL)
            if stage != self.final:
                # Rigged next: the bare model is only kept to look at.
                if glb:
                    keep(self, glb, raw, "model")
                thumbnail(self, result, raw.with_suffix(".png"))
                return
            if not glb:
                raise MeshyError("the finished task has no GLB")
            download(glb, raw)
            self.done = put(self.asset, raw, self.model["task_id"], ledger)
            thumbnail(self, result, raw.with_suffix(".png"))
            return
        self.finish_rig(result, ledger)

    def finish_rig(self, result: dict, ledger: Ledger) -> None:
        """Download the rigged character and its basic animations, then
        install them. The animations installed are the armature-only GLBs
        (the unit pipeline retargets them onto the character); the with-skin
        ones stand in when those are missing, and no animations at all is
        only a warning (the pipeline falls back to retargeting)."""
        rigged = result.get("result") or {}
        glb = rigged.get("rigged_character_glb_url")
        if not glb:
            raise MeshyError("the finished rig has no rigged_character_glb_url")
        download(glb, self.raw(RIG))
        basic = rigged.get("basic_animations") or {}
        if not basic:
            print(f"  {self.label}: the rig came with no basic animations (only the character is installed)", flush=True)
        animations = {}
        for name in ANIMATIONS:
            source = next((s for s in (f"{name}_armature", name) if basic.get(f"{s}_glb_url")), None)
            if source is None:
                if basic:
                    print(f"  {self.label}: the rig has no {name} animation", flush=True)
                continue
            if source == name:
                print(f"  {self.label}: no armature-only {name} GLB; installing the with-skin one", flush=True)
            download(basic[f"{source}_glb_url"], self.raw(RIG, f"_{source}"))
            animations[name] = {"from": source, "installed": rel(self.asset.animation_target(name))}
        # Recorded once every file is here: a rerun installs from art/meshy/out/.
        ledger.update(self.rig["task_id"], animations=animations)
        self.done = self.install_rig(ledger)

    def install_rig(self, ledger: Ledger) -> Path:
        for name, a in self.rig["animations"].items():
            target = self.asset.animation_target(name)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(self.raw(RIG, f"_{a['from']}"), target)
        return put(self.asset, self.raw(RIG), self.rig["task_id"], ledger)

    def from_disk(self, ledger: Ledger) -> bool:
        if not self.raw(self.final).exists():
            return False
        if self.final == RIG:
            animations = self.rig.get("animations")
            if animations is None or not all(self.raw(RIG, f"_{a['from']}").exists() for a in animations.values()):
                return False
            self.done = self.install_rig(ledger)
        else:
            self.done = put(self.asset, self.raw(MODEL), self.model["task_id"], ledger)
        return True


def attach_existing(job: Job, ledger: Ledger, style: dict) -> None:
    """Pick up the tasks a previous run already started for this job. A
    model takes the latest texture started on any of its asset's current
    meshes (`refine` may have picked another candidate than this job's),
    so a model already paid for is never paid for again."""
    if not any(t["asset"] == job.asset.key and t["stage"] in ("preview", "refine") for t in ledger.tasks):
        # Nothing to pick up. This also spares a concept-path asset the text
        # to 3D prompt limit (800 characters), which its concept never meets.
        return
    previews = ledger.find(job.asset.key, "preview", preview_hash(job.asset, style))
    if job.texture:
        meshes = {refine_hash(job.asset, style, p["task_id"]): p for p in previews}
        refines = [t for t in ledger.tasks if t["asset"] == job.asset.key and t["stage"] == "refine" and t["hash"] in meshes and t["status"] not in FINAL_FAILURES]
        if refines:
            job.refine = refines[-1]
            job.preview = meshes[job.refine["hash"]]
            return
    previews = [t for t in previews if t.get("candidate", 0) == job.candidate]
    if previews:
        job.preview = previews[-1]


def concept_jobs(asset: Asset, style: dict, ledger: Ledger, candidates: int) -> list[ConceptJob]:
    """The asset's concept candidates 0..candidates-1, with the tasks
    earlier runs made for them; one whose image is downloaded is done."""
    jobs = []
    for n in range(candidates):
        req = concept_request(asset, style, n)
        job = ConceptJob(asset, n, req)
        found = ledger.find(asset.key, "concept", req.hash)
        if found:
            job.concept = found[-1]
            if job.concept["status"] == "SUCCEEDED" and job.image().exists():
                job.done = job.image()
        jobs.append(job)
    return jobs


def model_job(asset: Asset, pick: dict, style: dict, ledger: Ledger) -> ModelJob:
    """The model of the asset's picked concept, with the tasks earlier runs
    made for it (the same concept and settings; the rig of that model)."""
    job = ModelJob(asset, pick)
    models = ledger.find(asset.key, "model", model_hash(asset, style, pick["task_id"]))
    job.model = models[-1] if models else None
    if asset.rig and job.model is not None:
        rigs = ledger.find(asset.key, "rig", rig_hash(asset, job.model["task_id"]))
        job.rig = rigs[-1] if rigs else None
    return job


def install(job: Job, result: dict, ledger: Ledger) -> Path:
    glb = (result.get("model_urls") or {}).get("glb")
    if not glb:
        raise MeshyError("the finished task has no GLB")
    task_id = job.refine["task_id"]
    raw = OUT / job.asset.key / f"{task_id}.glb"
    download(glb, raw)
    target = place(job, raw, ledger)
    thumbnail(job, result, OUT / job.asset.key / f"{task_id}.png")
    return target


def unsigned(url: str | None) -> str | None:
    """A Meshy asset link without its signature (the query string), for the
    ledger, which is committed: the signed part is a time-limited access
    token, and downloads always use the fresh link from the task."""
    return url.split("?", 1)[0] if url else url


def error_text(result: dict) -> str | None:
    """A task's error message (task_error is an object with a message)."""
    e = result.get("task_error")
    if isinstance(e, dict):
        e = e.get("message")
    return e or None


def keep(job, url: str, to: Path, what: str) -> None:
    """Download a file that is only for looking at: one that fails is a
    warning, not a reason to stop a paid run."""
    try:
        download(url, to)
    except MeshyError as e:
        print(f"  {job.label}: no {what} ({e})", flush=True)


def thumbnail(job, result: dict, to: Path) -> None:
    if result.get("thumbnail_url"):
        keep(job, result["thumbnail_url"], to, "thumbnail")


def put(asset: Asset, raw: Path, task_id: str, ledger: Ledger) -> Path:
    """Copy a downloaded model to where its pipeline reads it, and record
    that on the task in the ledger."""
    target = asset.target()
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(raw, target)
    ledger.update(task_id, installed=rel(target))
    return target


def place(job: Job, raw: Path, ledger: Ledger) -> Path:
    return put(job.asset, raw, job.refine["task_id"], ledger)


def describe(job: Pipeline) -> str:
    """The job's tasks and their states, for error messages."""
    return ", ".join(f"{s.word} {t['status']} (task {t['task_id']})" if (t := job.task(s)) else f"no {s.word} task" for s in job.stages)


# --------------------------------------------------------------------------
# The scheduler
# --------------------------------------------------------------------------

def run_jobs(http: Http, ledger: Ledger, style: dict, jobs: list[Pipeline], queue_limit: int, sleep: Callable[[float], None] = time.sleep) -> None:
    """Drive every job to the end, keeping at most `queue_limit` tasks in
    flight across all jobs and stages: later stages start first (rigs, then
    models, then concepts; textures, then meshes), so finished work moves on
    while the queue stays full. A round that can neither start, install nor
    wait for anything fails every unfinished job instead of polling forever.
    A finished job's tasks (one that timed out) are left alone: a rerun
    resumes them."""
    if queue_limit < 1:
        raise MeshyError(f"queue_limit is {queue_limit} in style.json: nothing could ever start")
    waiting = False
    while not all(j.finished for j in jobs):
        progressed = queue_full = False
        # 1. Last tasks an earlier run finished whose installed file is
        # gone: install them again from what was already paid for.
        for j in jobs:
            if j.needs_recovery():
                j.recover(http, ledger)
                progressed = True
        in_flight = sum(1 for j in jobs if not j.finished for s in j.stages if (t := j.task(s)) is not None and t["status"] in IN_FLIGHT)
        # 2. Start what can start, later stages first. A full queue (tasks
        # from elsewhere count too) ends this round; the next tries again.
        ready = sorted(((j, s) for j in jobs if not j.finished for s in (j.next_stage(),) if s is not None), key=lambda js: -js[1].rank)
        try:
            for j, s in ready:
                if in_flight >= queue_limit:
                    break
                body, phash, fields = j.request(s, style)
                r = http("POST", j.path(s), body)
                entry = {"asset": j.asset.key, "stage": s.name}
                if "candidate" in fields:
                    entry["candidate"] = fields.pop("candidate")
                entry.update(task_id=r["result"], **fields, hash=phash, status="PENDING", credits=j.stage_cost(s, style), created=int(time.time()))
                setattr(j, s.name, ledger.add(**entry))
                in_flight += 1
                progressed = True
                print(f"  {j.label}: {s.word} started", flush=True)
        except QueueFull:
            queue_full = True
            if not waiting:
                print("  the account's task queue is full: waiting for a task to finish", flush=True)
        waiting = queue_full
        # 3. Check everything in flight.
        for j in jobs:
            if j.finished:
                continue
            for s in j.stages:
                task = j.task(s)
                if task is None or task["status"] not in IN_FLIGHT:
                    continue
                progressed = True
                result = http("GET", f"{j.path(s)}/{task['task_id']}", None)
                status = result.get("status", task["status"])
                progress = result.get("progress", 0)
                if j.printed.get(s.name) != (status, progress // 25):
                    j.printed[s.name] = (status, progress // 25)
                    print(f"  {j.label} {s.word}: {status} {progress}%", flush=True)
                if status in IN_FLIGHT:
                    if time.time() - task.get("created", time.time()) > TASK_TIMEOUT_SECONDS:
                        j.error = f"{j.label}: {s.word} still {status} after {TASK_TIMEOUT_SECONDS // 60} minutes (task {task['task_id']}; rerun to resume)"
                    continue
                # Failed tasks are refunded: the ledger keeps what Meshy says was consumed.
                ledger.update(task["task_id"], status=status, credits=result.get("consumed_credits", task["credits"]), error=error_text(result), **j.result_fields(s, result))
                if status != "SUCCEEDED":
                    j.error = f"{j.label}: {s.word} {status} ({error_text(result)})"
                    continue
                try:
                    j.finish(s, result, ledger)
                    if s == j.final and j.done is not None:
                        print(f"{j.label}: {s.verb} {rel(j.done)}", flush=True)
                except MeshyError as e:  # paid for: a rerun installs it without paying again
                    j.error = f"{j.label}: {s.word} task {task['task_id']} finished but was not {s.verb} ({e}); rerun to try again"
        # 4. Nothing started, installed or in flight, and no full queue to
        # wait out: no later round can differ, so fail loudly, not hang.
        if not progressed and not queue_full:
            for j in jobs:
                if not j.finished:
                    j.error = f"{j.label}: stuck with {describe(j)}; nothing to start or wait for (meshy.py does not handle this state)"
        if not all(j.finished for j in jobs):
            sleep(POLL_SECONDS)


def reserve(job: Pipeline, style: dict, budget: Budget, jobs: list, skipped: list[str]) -> None:
    """Take the job if the budget can finish it (its whole remaining cost is
    reserved up front), else list it as not started."""
    cost = job.cost(style)
    if budget.can(cost):
        budget.take(cost, job.label)
        jobs.append(job)
    else:
        skipped.append(job.label)


def plan_jobs(assets: list[Asset], style: dict, ledger: Ledger, budget: Budget, texture: bool, candidates: int = 1) -> tuple[list[Job], list[str]]:
    """Jobs for the models not made yet, in order, as many as the budget can
    finish (each job's whole remaining cost is reserved up front)."""
    jobs, skipped = [], []
    for a in assets:
        if texture and a.target().exists():
            print(f"{a.key}: already made")
            continue
        if texture and ledger.picked(a.key):
            print(f"{a.key}: has a picked concept: `models` makes it")
            continue
        for c in range(candidates):
            job = Job(a, candidate=c, texture=texture)
            attach_existing(job, ledger, style)
            reserve(job, style, budget, jobs, skipped)
    return jobs, skipped


def plan_concepts(assets: list[Asset], style: dict, ledger: Ledger, budget: Budget, candidates: int, named: set[str]) -> tuple[list[ConceptJob], list[str]]:
    """Concept candidates not made yet. An asset already picked or installed
    gets none unless `named` (--only) names it."""
    jobs, skipped = [], []
    for a in assets:
        if not (a.id in named or a.key in named):
            pick = ledger.picked(a.key)
            if pick is not None:
                print(f"{a.key}: concept {pick['task_id']} is picked; no new candidates (name it in --only for more)")
                continue
            if a.target().exists():
                print(f"{a.key}: already made ({rel(a.target())}); no concepts (name it in --only for some)")
                continue
        for job in concept_jobs(a, style, ledger, candidates):
            reserve(job, style, budget, jobs, skipped)
    return jobs, skipped


def plan_models(assets: list[Asset], style: dict, ledger: Ledger, budget: Budget) -> tuple[list[ModelJob], list[str], list[str]]:
    """Models (and rigs) of picked concepts not made yet; and the assets
    with no pick."""
    jobs, skipped, unpicked = [], [], []
    for a in assets:
        pick = ledger.picked(a.key)
        if pick is None:
            unpicked.append(a.key)
            continue
        job = model_job(a, pick, style, ledger)
        if job.made():
            print(f"{a.key}: already made from concept {pick['task_id']}")
            continue
        reserve(job, style, budget, jobs, skipped)
    return jobs, skipped, unpicked


# --------------------------------------------------------------------------
# Commands
# --------------------------------------------------------------------------

def cmd_balance(http: Http) -> None:
    b = http("GET", "/openapi/v1/balance", None)
    resets = b.get("monthly_resets_at")
    when = time.strftime("%Y-%m-%d", time.localtime(resets / 1000)) if resets else "never"
    print(f"balance {b['balance']} credits ({b.get('monthly_credits', 0)} monthly, reset {when}; {b.get('permanent_credits', 0)} permanent)")


def cmd_plan(assets: list[Asset], style: dict, ledger: Ledger, http: Http | None) -> int:
    """Per asset, the stages still to pay and their credits; totals per
    stage and overall; then the balance."""
    per_stage: dict[str, int] = {}
    unpicked = []
    concepts = "concept" in style
    print(f"text to 3D: {style['ai_model']}, texture {style['texture_resolution']}, pbr {style['enable_pbr']}; queue {style.get('queue_limit', 10)}")
    if concepts:
        c, m = concept_style(style), model_style(style)
        candidates = c.get("candidates", 2)
        print(f"concepts: {c['ai_model']}, {candidates} candidates per asset; models: {m['ai_model']}, texture {m['texture_resolution']}, pbr {m['enable_pbr']}")
    for a in assets:
        steps: list[tuple[str, int, str]] = []  # stage, credits still to pay, words
        note = ""
        pick = ledger.picked(a.key)
        if pick is not None:
            job = model_job(a, pick, style, ledger)
            if job.made():
                print(f"  {a.key:<24}       done ({rel(a.target())}, from concept {pick['task_id']})")
                continue
            for s in job.stages:
                t = job.task(s)
                steps.append((s.name, 0, f"{s.word} paid") if t else (s.name, job.stage_cost(s, style), f"{s.word} {job.stage_cost(s, style)}"))
            final = job.task(job.final)
            note = " (already paid; models reinstalls it)" if final and final["status"] == "SUCCEEDED" else ""
        elif a.target().exists():
            print(f"  {a.key:<24}       done ({rel(a.target())})")
            continue
        else:
            job = Job(a)
            if a.kind in TEXT_TO_3D_KINDS:
                attach_existing(job, ledger, style)
            if a.kind in TEXT_TO_3D_KINDS and (not concepts or job.preview or job.refine):
                cost = job.cost(style)
                steps.append(("text-to-3d", cost, f"text to 3D {cost}"))
                note = (" (already paid; make reinstalls it)" if job.refine and job.refine["status"] == "SUCCEEDED"
                        else " (already paid; make resumes it)" if job.refine else " (mesh already paid)" if job.preview else "")
            else:
                if not concepts:
                    concept_style(style)  # says what is missing
                cjobs = concept_jobs(a, style, ledger, candidates)
                missing = [j for j in cjobs if j.concept is None]
                each = cjobs[0].req.cost
                steps.append(("concept", len(missing) * each, f"concepts {len(missing)}x{each}" if missing else f"concepts paid ({len(cjobs)})"))
                steps.append(("pick", 0, "pick"))
                steps.append(("model", model_cost(m["texture_resolution"]), f"model {model_cost(m['texture_resolution'])}"))
                if a.rig:
                    steps.append(("rig", RIG_COST, f"rig {RIG_COST}"))
                unpicked.append(a.key)
        cost = sum(credits for _, credits, _ in steps)
        for stage, credits, _ in steps:
            if stage != "pick":
                per_stage[stage] = per_stage.get(stage, 0) + credits
        print(f"  {a.key:<24} {cost:>4} credits  {a.kind:<9} {', '.join(words for _, _, words in steps)}{note}")
    total = sum(per_stage.values())
    print("per stage: " + (", ".join(f"{stage} {credits}" for stage, credits in per_stage.items()) or "nothing to pay"))
    if unpicked:
        print(f"{len(unpicked)} assets have no pick yet (concepts, then meshy.py pick <key> <n>): {', '.join(unpicked)}")
    print(f"total {total} credits to finish {len(assets)} assets (ledger so far: {ledger.spent()})")
    if http is not None:
        cmd_balance(http)
    return total


def problems(jobs: list[Pipeline], skipped: list[str], budget: Budget) -> None:
    found = [j.error for j in jobs if j.error] + ([f"not started (over --max-credits {budget.cap}): {', '.join(skipped)}"] if skipped else [])
    if found:
        raise MeshyError("; ".join(found))


def report(jobs: list[Job], skipped: list[str], budget: Budget, ledger: Ledger) -> None:
    made = [j for j in jobs if j.done]
    meshes = [j for j in jobs if not j.texture and j.preview and j.preview["status"] == "SUCCEEDED"]
    print(f"{len(made)} models made, {len(meshes)} candidate meshes; {budget.used} credits reserved this run (ledger total {ledger.spent()})")
    problems(jobs, skipped, budget)


def cmd_make(http: Http, assets: list[Asset], style: dict, ledger: Ledger, budget: Budget, sleep: Callable[[float], None] = time.sleep) -> None:
    jobs, skipped = plan_jobs(assets, style, ledger, budget, texture=True)
    run_jobs(http, ledger, style, jobs, style.get("queue_limit", 10), sleep)
    report(jobs, skipped, budget, ledger)


def cmd_preview(http: Http, assets: list[Asset], style: dict, ledger: Ledger, budget: Budget, candidates: int, sleep: Callable[[float], None] = time.sleep) -> None:
    jobs, skipped = plan_jobs(assets, style, ledger, budget, texture=False, candidates=candidates)
    run_jobs(http, ledger, style, jobs, style.get("queue_limit", 10), sleep)
    for j in jobs:
        if j.preview and j.preview["status"] == "SUCCEEDED":
            print(f"{j.label}: preview task {j.preview['task_id']} (thumbnail in {(OUT / j.asset.key).relative_to(ROOT)})")
    report(jobs, skipped, budget, ledger)


def cmd_refine(http: Http, asset: Asset, style: dict, ledger: Ledger, budget: Budget, preview_id: str, sleep: Callable[[float], None] = time.sleep) -> None:
    preview = next((t for t in ledger.tasks if t["task_id"] == preview_id), None)
    if preview is None or preview["status"] != "SUCCEEDED":
        raise MeshyError(f"{preview_id}: not a finished preview in the ledger")
    if preview.get("stage") != "preview" or preview.get("asset") != asset.key:
        raise MeshyError(f"{preview_id}: a {preview.get('stage')} of {preview.get('asset')}, not a preview of {asset.key}")
    job = Job(asset, preview=preview)
    refines = ledger.find(asset.key, "refine", refine_hash(asset, style, preview_id))
    job.refine = refines[-1] if refines else None
    if job.refine is None:
        budget.take(refine_cost(style["texture_resolution"]), f"{asset.key} texture")
    run_jobs(http, ledger, style, [job], style.get("queue_limit", 10), sleep)
    report([job], [], budget, ledger)


def cmd_concepts(http: Http, assets: list[Asset], style: dict, ledger: Ledger, budget: Budget, candidates: int | None = None,
                 named: set[str] | None = None, sleep: Callable[[float], None] = time.sleep) -> None:
    candidates = concept_style(style).get("candidates", 2) if candidates is None else candidates
    if candidates < 1:
        raise MeshyError("--candidates must be at least 1")
    jobs, skipped = plan_concepts(assets, style, ledger, budget, candidates, named or set())
    run_jobs(http, ledger, style, jobs, style.get("queue_limit", 10), sleep)
    for j in jobs:
        if j.done:
            print(f"{j.label}: concept task {j.concept['task_id']} -> {rel(j.done)}")
    made = sum(1 for j in jobs if j.done)
    print(f"{made} concept images; {budget.used} credits reserved this run (ledger total {ledger.spent()}); pick one per asset with: meshy.py pick <key> <n>")
    problems(jobs, skipped, budget)


def cmd_pick(ledger: Ledger, asset: Asset, choice: str) -> dict:
    """Record the concept `models` makes for the asset: a candidate number
    (its latest finished concept with that number) or a concept task id."""
    if choice.isdigit():
        found = [t for t in ledger.tasks if t["asset"] == asset.key and t["stage"] == "concept" and t.get("candidate") == int(choice) and t["status"] == "SUCCEEDED"]
        if not found:
            raise MeshyError(f"{asset.key}: no finished concept candidate {choice} in the ledger (meshy.py status lists them)")
        chosen = found[-1]
    else:
        chosen = next((t for t in ledger.tasks if t["task_id"] == choice), None)
        if chosen is None:
            raise MeshyError(f"{choice}: no such task in the ledger")
        if chosen["stage"] != "concept" or chosen["asset"] != asset.key:
            raise MeshyError(f"{choice}: a {chosen['stage']} of {chosen['asset']}, not a concept of {asset.key}")
        if chosen["status"] != "SUCCEEDED":
            raise MeshyError(f"{choice}: a concept that is {chosen['status']}, not finished")
    before = ledger.picked(asset.key)
    ledger.pick(chosen["task_id"])
    was = f" (was {before['task_id']})" if before is not None and before is not chosen else ""
    print(f"{asset.key}: picked concept {chosen['task_id']} (candidate {chosen.get('candidate')}){was}; `models` makes it")
    return chosen


def cmd_models(http: Http, assets: list[Asset], style: dict, ledger: Ledger, budget: Budget, sleep: Callable[[float], None] = time.sleep) -> None:
    model_style(style)
    jobs, skipped, unpicked = plan_models(assets, style, ledger, budget)
    if unpicked:
        print(f"no concept picked (skipped; meshy.py pick <key> <n>): {', '.join(unpicked)}")
    run_jobs(http, ledger, style, jobs, style.get("queue_limit", 10), sleep)
    made = [j for j in jobs if j.done]
    print(f"{len(made)} models made ({sum(1 for j in made if j.asset.rig)} rigged); {budget.used} credits reserved this run (ledger total {ledger.spent()})")
    problems(jobs, skipped, budget)


def cmd_status(ledger: Ledger) -> None:
    for t in ledger.tasks:
        stage = f"{t['stage']}#{t.get('candidate')}" if t["stage"] == "concept" else t["stage"]
        note = t.get("installed") or ("picked" if t.get("picked") else "")
        print(f"{t['asset']:<24} {stage:<10} {t['status']:<10} {t.get('credits', 0):>3}  {t['task_id']}  {note}")
    print(f"{len(ledger.tasks)} tasks, {ledger.spent()} credits")


def preflight(cmd: str, assets: list[Asset], style: dict) -> None:
    """What a paid command needs from style.json and the references, checked
    before the key is read: a bad config costs nothing."""
    if cmd == "concepts":
        for a in assets:
            concept_request(a, style, 0)
    elif cmd == "models":
        model_style(style)
        for a in assets:
            polycount(a, style)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("balance")
    sub.add_parser("status")
    for name in ("plan", "make", "preview", "refine", "concepts", "models"):
        s = sub.add_parser(name)
        s.add_argument("--only", help="comma-separated asset ids or ids_variant")
        if name in ("plan", "concepts", "models"):
            s.add_argument("--batch", help="only the assets of this batch (requests.json \"batch\")")
        if name != "plan":
            s.add_argument("--max-credits", type=int, required=True)
        if name == "preview":
            s.add_argument("--candidates", type=int, default=2)
        if name == "concepts":
            s.add_argument("--candidates", type=int, help="concept images per asset (default: style.json concept.candidates)")
        if name == "refine":
            s.add_argument("asset", help="id or id_variant")
            s.add_argument("preview_task")
    s = sub.add_parser("pick")
    s.add_argument("asset", help="id or id_variant")
    s.add_argument("concept", help="a concept candidate number or concept task id")
    args = p.parse_args(argv)
    try:
        ledger = Ledger(LEDGER)
        if args.cmd == "status":
            cmd_status(ledger)
            return 0
        if args.cmd == "pick":  # no API call, no key
            cmd_pick(ledger, refine_target(load_assets(), args.asset), args.concept)
            return 0
        if args.cmd != "balance":
            # Settle what to make before the key is read: a bad name costs nothing.
            style = load_style()
            only = set(args.only.split(",")) if args.only else None
            if args.cmd == "refine":
                assets = [refine_target(load_assets(style=style), args.asset)]
                if assets[0].kind not in TEXT_TO_3D_KINDS:
                    raise MeshyError(f"{assets[0].key}: units and props are made with concepts, pick and models")
            else:
                assets = load_assets(only, getattr(args, "batch", None), style)
                if args.cmd in ("make", "preview"):
                    assets = text_to_3d_assets(assets)
                if not assets:
                    raise MeshyError("no matching assets in art/meshy/requests.json")
            if args.cmd == "concepts" and args.candidates is not None and args.candidates < 1:
                raise MeshyError("--candidates must be at least 1")
            preflight(args.cmd, assets, style)
        http = make_http(api_key())
        if args.cmd == "balance":
            cmd_balance(http)
        elif args.cmd == "plan":
            cmd_plan(assets, style, ledger, http)
        elif args.cmd == "make":
            cmd_make(http, assets, style, ledger, Budget(args.max_credits))
        elif args.cmd == "preview":
            cmd_preview(http, assets, style, ledger, Budget(args.max_credits), args.candidates)
        elif args.cmd == "refine":
            cmd_refine(http, assets[0], style, ledger, Budget(args.max_credits), args.preview_task)
        elif args.cmd == "concepts":
            cmd_concepts(http, assets, style, ledger, Budget(args.max_credits), args.candidates, only or set())
        elif args.cmd == "models":
            cmd_models(http, assets, style, ledger, Budget(args.max_credits))
        return 0
    except MeshyError as e:
        print(f"meshy: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
