"""Generate game models with the Tripo3D API (v3), in the style Meshy set.

Tripo takes over from Meshy (tools/meshy/meshy.py) now that the Meshy
credits are spent, with the same guarantees: every paid task is recorded in
the committed art/tripo/ledger.json the moment it starts, keyed by a hash of
exactly what was asked, so an interrupted run resumes instead of paying
twice; every paid command takes --max-credits, and an asset's whole
remaining cost is reserved before its task starts; a made model whose
installed file was deleted is installed again without paying; retired
variants are never made; the API key is never printed. The commands, what
they cost and the Tripo Studio route by hand: tools/tripo/README.md.

What it makes:

* The assets in art/tripo/requests.json (the art/meshy/requests.json schema
  plus the kind "decor": landscape scenery that is not a game entity,
  installed to art/static/source/decor/<key>.glb), in the shared style of
  art/tripo/style.json.
* Batch "meshy-finish": every Meshy asset whose concept was picked (in
  art/meshy/ledger.json) but never made into a model. Its picked Meshy
  concept image (art/meshy/out/<key>/concept_<n>_<task>.png) is uploaded and
  made into a model, installed where meshy.py would have put it.

    python tools/tripo/tripo.py balance | check | status | usage
    python tools/tripo/tripo.py plan [--batch P0] [--only a,b] [--bodies]
    python tools/tripo/tripo.py concepts --batch P0 --max-credits 1000 [--candidates 2]
    python tools/tripo/tripo.py pick mountain_peak_a 1     (a candidate number or a concept task id)
    python tools/tripo/tripo.py models --batch P0 --max-credits 1000
    python tools/tripo/tripo.py make --batch P1 --max-credits 60
    python tools/tripo/tripo.py resolve <key> <task_id | none>
    python tools/tripo/tripo.py kit --batch meshy-finish
    python tools/tripo/tripo.py import-studio --batch meshy-finish

1. `concepts` makes concept images, N candidates per asset (style.json
   concept.candidates, or --candidates), each its own task: text to image,
   or image to image when style.json lists style references for the
   asset's kind (uploaded once per run; they set the art style only).
   Images land in art/tripo/out/<key>/concept_<n>_<task>.png (git-ignored).
2. `pick` records the chosen concept in the ledger (no API call, no key).
3. `models` makes each picked concept (a Tripo pick, else a Meshy pick)
   into a textured model (image to model) and installs it: units in
   art/units/source/<key>.glb, props in art/units/source/props/, buildings,
   nodes and decor in art/static/source/<kind>/<key>.glb. Every download is
   also kept in art/tripo/out/<key>/.
4. `make` makes the "route": "text" assets (cheap fillers such as stumps)
   straight from text (text to model); --only can name another node,
   building or decor asset with no pick.

Concurrency: at most --concurrency tasks in flight (default style.json
concurrency, the owner's limit of 100), submitted by a pool of workers as
fast as Tripo's rate limits allow (HTTP 429 with code 1007 is waited out
from Retry-After / X-RateLimit-Reset, then retried; nothing was created).
A category Tripo says is full (code 2000) waits until one of its tasks
finishes. Later stages start first (models before concepts). Tasks are
polled together through POST /tasks/list, 100 at a time. A finished task's
files are downloaded at once (Tripo's links expire), retried, and the task
is queried again for fresh links when a download fails.

A task-creating request whose answer is lost (a 5xx or a dropped
connection) is never retried: Tripo may have made (and charged for) the
task. It is recorded as "uncertain" and that request is not sent again
until `resolve` says which task it made (see `usage`) or that it made none.

`kit` and `import-studio` are the manual route through the Tripo Studio web
app (whose membership credits are not API credits): `kit` writes one folder
per model to make under art/tripo/studio/<batch>/ (git-ignored) with the
input image, the prompt and the Studio settings; `import-studio` installs
the GLB dropped into each folder and records it in the ledger (0 credits).

The API key comes from the TRIPO_API_KEY environment variable or the
git-ignored .env file at the repo root. It is never printed.
"""

from __future__ import annotations

import argparse
import concurrent.futures as cf
import hashlib
import json
import os
import re
import shutil
import socket
import sys
import threading
import time
import urllib.error
import urllib.request
import uuid
from dataclasses import dataclass, field
from http.client import HTTPException
from pathlib import Path
from typing import Callable

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / "art" / "tripo"
LEDGER = ART / "ledger.json"
OUT = ART / "out"
STUDIO = ART / "studio"
MESHY = ROOT / "art" / "meshy"  # read only: its requests, ledger and concept images
MESHY_OUT = MESHY / "out"
SOURCE = ROOT / "art" / "static" / "source"
UNITS = ROOT / "art" / "units" / "source"
# The international API. The docs' openapi.tripo3d.com is the China site,
# which rejects international keys.
API = "https://openapi.tripo3d.ai/v3"

TEXT_TO_IMAGE = "/generation/text-to-image"
IMAGE_TO_IMAGE = "/generation/image-to-image"
TEXT_TO_MODEL = "/generation/text-to-model"
IMAGE_TO_MODEL = "/generation/image-to-model"
TASKS_LIST = "/tasks/list"
FILES = "/files"
BALANCE = "/account/balance"
USAGE = "/account/usage"
# Each category has its own concurrency pool at Tripo (docs: rate limits).
CATEGORY = {TEXT_TO_IMAGE: "image generation", IMAGE_TO_IMAGE: "image generation",
            TEXT_TO_MODEL: "3D generation", IMAGE_TO_MODEL: "3D generation"}
API_NAME = {path: path.rsplit("/", 1)[-1] for path in CATEGORY}

POLL_SECONDS = 3.0
TASK_TIMEOUT_SECONDS = 45 * 60
QUEUE_RETRY_SECONDS = 10.0  # a full category with none of our tasks in it: try again after this
MAX_BATCH_QUERY = 100  # task ids per POST /tasks/list
IO_WORKERS = 16  # threads that submit, upload and download
HTTP_TRIES = 8
HTTP_TIMEOUT = 60
MAX_WAIT = 120.0  # longest single rate-limit wait
DOWNLOAD_TRIES = 3  # per link
REFETCHES = 3  # fresh links asked for after the downloads of a link all failed
CONCEPT_TASK_HOURS = 20  # an older concept is uploaded from its download, not referenced by task id

QUEUED, RUNNING, SUCCEEDED = "queued", "running", "success"
IN_FLIGHT = (QUEUED, RUNNING)
UNCERTAIN = "uncertain"  # a create whose answer was lost: maybe made, maybe not
NOT_CREATED = "not_created"

KINDS = ("nodes", "buildings", "units", "props", "decor")
TEXT_KINDS = ("nodes", "buildings", "decor")  # what `make` may make from text; units and props need a concept
FRAMINGS = ("character", "object", "prop", "scenery")
ROUTES = ("concept", "text")
MESHY_BATCH = "meshy-finish"
IMAGE_TYPES = {".png": "image/png", ".jpg": "image/jpeg", ".jpeg": "image/jpeg"}
MAX_IMAGE_BYTES = 20 * 1024 * 1024
MAX_IMAGE_PROMPT = 1800
MAX_TEXT_PROMPT = 1024
MAX_NEGATIVE_PROMPT = 255
STYLE_ONLY = "The reference images show the target art style and rendering only: do not copy their subjects, shapes or composition."

# Credits. A model, the docs' v3.1 price table: from text 10 / 20 / 30, from
# an image 20 / 30 / 40 (untextured / standard texture / detailed texture);
# "fast" costs the same as standard.
MODEL_PRICE = {
    TEXT_TO_MODEL: {"none": 10, "standard": 20, "fast": 20, "detailed": 30},
    IMAGE_TO_MODEL: {"none": 20, "standard": 30, "fast": 30, "detailed": 40},
}
# Add-ons the docs say cost extra without saying how much: reserved high.
# The ledger records what Tripo really charged (credits_consumed), and a
# charge above its reservation is reported after the run.
EXTREME_EXTRA = 20  # texture_quality extreme, over detailed
PBR_EXTRA = 10
ULTRA_EXTRA = 20  # geometry_quality detailed
# One image at low or medium quality: the docs give no table; 20 is the
# credits_consumed of their text-to-image and image-to-image examples.
IMAGE_PRICE = 20
IMAGE_MODELS = ("banana", "banana_pro", "banana2", "seedream_v4", "seedream_v5",
                "chat_image_1.5", "chat_image_2", "chat_image_2.5_flare", "chat_image_2.5_sunburst")
BANANA = ("banana", "banana_pro", "banana2")
BANANA_ASPECTS = ("1:1", "2:3", "3:2", "3:4", "4:3", "4:5", "5:4", "9:16", "16:9", "21:9")
MAX_INPUTS = {"banana": 10, "banana_pro": 10, "banana2": 10, "seedream_v5": 4}  # chat_image: 16
MODEL_VERSIONS = ("v3.1-20260211", "v3.0-20250812")  # H series with texture_quality (v2.5 has none)
TEXTURE_VERSIONS = ("v3.5-20260815", "v3.0-20250812")
TEXTURE_QUALITIES = ("standard", "detailed", "extreme", "fast")
# style.json model / text_model keys sent as they are (anything else is refused).
MODEL_OPTIONS = ("model", "texture", "pbr", "texture_quality", "texture_version", "delight", "geometry_quality",
                 "orientation", "texture_alignment", "enable_image_autofix", "auto_size", "export_uv", "compress")
TEXT_OPTIONS = ("model", "texture", "pbr", "texture_quality", "texture_version", "delight", "geometry_quality",
                "auto_size", "export_uv", "compress")


class TripoError(Exception):
    pass


class QueueFull(TripoError):
    """Tripo refused a task because its category is at its concurrency limit
    (code 2000). Nothing was created."""

    def __init__(self, message: str, retry_after: float | None = None):
        super().__init__(message)
        self.retry_after = retry_after


class NoCredits(TripoError):
    """Code 2010: the balance cannot pay for the task. Nothing was created."""


class Uncertain(TripoError):
    """A task-creating request whose answer was lost: Tripo may have made it."""


class DownloadError(TripoError):
    pass


def num(x) -> int | float:
    """A credit amount as Tripo sends it (40.00), stored as 40."""
    x = float(x)
    return int(x) if x.is_integer() else x


def is_int(v) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def rel(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT.resolve()).as_posix()
    except ValueError:
        return path.as_posix()


# --------------------------------------------------------------------------
# Assets
# --------------------------------------------------------------------------

@dataclass(frozen=True)
class Asset:
    id: str
    kind: str  # nodes | buildings | units | props | decor
    variant: str  # "" for a single model
    prompt: str
    source: str = "tripo"  # tripo: art/tripo/requests.json; meshy: a Meshy pick never made into a model
    batch: str | None = None
    age: int | None = None
    faction: str | None = None
    magic: bool = False
    rig: bool = False  # Meshy humanoids only: tripo.py does not rig
    face_limit: int | None = None  # None: style.json face_limit[kind]
    framing: str | None = None
    prompt_suffix: str | None = None  # replaces the kind's text, as in meshy.py
    route: str = "concept"
    meshy_concept: str | None = None  # source meshy: the picked Meshy concept task
    meshy_candidate: int | None = None

    def __post_init__(self):
        if self.framing is None:
            framing = ("character" if self.rig else "prop" if self.kind == "props"
                       else "scenery" if self.kind == "decor" else "object")
            object.__setattr__(self, "framing", framing)

    @property
    def key(self) -> str:
        return f"{self.id}_{self.variant}" if self.variant else self.id

    def target(self) -> Path:
        """Where the pipelines read it (meshy.py's places, plus decor)."""
        if self.kind == "units":
            return UNITS / f"{self.key}.glb"
        if self.kind == "props":
            return UNITS / "props" / f"{self.key}.glb"
        return SOURCE / self.kind / f"{self.key}.glb"


def load_style() -> dict:
    return json.loads((ART / "style.json").read_text(encoding="utf-8"))


ASSET_FIELDS = ("id", "kind", "prompt", "batch", "age", "faction", "magic", "framing", "variants", "retired",
                "prompt_suffix", "face_limit", "polycount", "route")
MESHY_ONLY = ("rig", "height_m", "texture_suffix")


def check_asset(a, style: dict) -> None:
    """Refuse a malformed requests.json entry, before anything is paid."""
    if not isinstance(a, dict) or not isinstance(a.get("id"), str) or not re.fullmatch(r"[a-z0-9_]+", a["id"]):
        raise TripoError(f"requests.json: every asset needs an \"id\" of lower-case letters, digits and _ (got {str(a)[:80]})")

    def bad(what: str):
        raise TripoError(f"{a['id']}: {what} (art/tripo/requests.json)")

    meshy_only = sorted(k for k in a if k in MESHY_ONLY)
    if meshy_only:
        bad(f"{', '.join(meshy_only)}: Meshy only (tripo.py makes no rigs; humanoids to rig go through meshy.py)")
    unknown = sorted(k for k in a if k not in ASSET_FIELDS and not k.startswith("_"))
    if unknown:
        bad(f"unknown field {', '.join(unknown)} (known: {', '.join(ASSET_FIELDS)})")
    if a.get("kind") not in KINDS:
        bad(f"kind must be one of {', '.join(KINDS)}, not {a.get('kind')!r}")
    if not isinstance(a.get("prompt"), str) or not a["prompt"].strip():
        bad("prompt must describe the subject")
    if "batch" in a and not (isinstance(a["batch"], str) and a["batch"]):
        bad(f"batch must be a name such as \"P0\", not {a['batch']!r}")
    if a.get("batch") == MESHY_BATCH:
        bad(f"batch {MESHY_BATCH} is the Meshy picks never made into models")
    if "age" in a:
        if not is_int(a["age"]) or not 1 <= a["age"] <= 5:
            bad(f"age must be a whole number from 1 to 5, not {a['age']!r}")
        if not isinstance((style.get("ages") or {}).get(str(a["age"])), str):
            bad(f"age {a['age']} has no text in style.json ages")
    factions = style.get("factions") or {}
    if "faction" in a and not (isinstance(a["faction"], str) and isinstance(factions.get(a["faction"]), str)):
        bad(f"faction {a['faction']!r} is not in style.json factions ({', '.join(factions) or 'none'})")
    if "magic" in a and not isinstance(a["magic"], bool):
        bad(f"magic must be true or false, not {a['magic']!r}")
    if a.get("magic"):
        if "faction" not in a:
            bad("magic needs a faction (style.json magic is per faction)")
        if not isinstance((style.get("magic") or {}).get(a["faction"]), str):
            bad(f"magic: faction {a['faction']} has no text in style.json magic")
    if "framing" in a and a["framing"] not in FRAMINGS:
        bad(f"framing must be one of {', '.join(FRAMINGS)}, not {a['framing']!r}")
    if "face_limit" in a and "polycount" in a:
        bad("give face_limit or polycount (its Meshy name), not both")
    for name in ("face_limit", "polycount"):
        if name in a and not (is_int(a[name]) and 500 <= a[name] <= 300_000):
            bad(f"{name} must be a whole number from 500 to 300000, not {a[name]!r}")
    if "route" in a and a["route"] not in ROUTES:
        bad(f"route must be one of {', '.join(ROUTES)}, not {a['route']!r}")
    if a.get("route") == "text" and a.get("kind") not in TEXT_KINDS:
        bad(f"route text is for {', '.join(TEXT_KINDS)}: units and props are made from a concept")
    variants = a.get("variants")
    if variants is not None and not (isinstance(variants, dict) and variants
                                     and all(isinstance(k, str) and re.fullmatch(r"[a-z0-9_]+", k) and isinstance(v, str) for k, v in variants.items())):
        bad("variants must map variant names (lower-case letters, digits and _) to extra descriptions")
    if "prompt_suffix" in a and not isinstance(a["prompt_suffix"], str):
        bad("prompt_suffix must be text")


def meshy_keys(meshy_root: Path | None = None) -> set[str]:
    """Every Meshy asset name, retired variants included: a Tripo asset may
    not take one (one name is one installed file, and retired stays retired)."""
    spec = json.loads(((meshy_root or MESHY) / "requests.json").read_text(encoding="utf-8"))
    keys = set()
    for a in spec["assets"]:
        for variant in a.get("variants") or {"": ""}:
            keys.add(f"{a['id']}_{variant}" if variant else a["id"])
    return keys


def load_requests(style: dict, meshy_root: Path | None = None) -> list[Asset]:
    """Every live asset of art/tripo/requests.json, checked."""
    spec = json.loads((ART / "requests.json").read_text(encoding="utf-8"))
    if not isinstance(spec.get("assets"), list):
        raise TripoError("requests.json: \"assets\" must be a list")
    taken = meshy_keys(meshy_root)
    assets, names = [], set()
    for a in spec["assets"]:
        check_asset(a, style)
        variants = a.get("variants") or {"": a["prompt"]}
        retired = a.get("retired", [])
        if not isinstance(retired, list) or not all(isinstance(r, str) and r for r in retired):
            raise TripoError(f"{a['id']}: retired must be a list of variant names")
        unknown = set(retired) - set(variants)
        if unknown:
            raise TripoError(f"{a['id']}: retired {', '.join(sorted(unknown))} is not a variant")
        for variant, extra in variants.items():
            prompt = a["prompt"] if not variant else f"{a['prompt']}, {extra}"
            asset = Asset(a["id"], a["kind"], variant, prompt, batch=a.get("batch"), age=a.get("age"),
                          faction=a.get("faction"), magic=a.get("magic", False),
                          face_limit=a.get("face_limit", a.get("polycount")), framing=a.get("framing"),
                          prompt_suffix=a.get("prompt_suffix"), route=a.get("route", "concept"))
            if asset.key in names:
                raise TripoError(f"{asset.key}: two assets in art/tripo/requests.json have this name")
            if asset.key in taken:
                raise TripoError(f"{asset.key}: a Meshy asset has this name (art/meshy/requests.json, retired variants "
                                 "included): give the Tripo asset a name of its own")
            names.add(asset.key)
            if variant not in retired:  # made once, taken out of the game: never made again
                assets.append(asset)
    return assets


def meshy_finish(meshy_root: Path | None = None) -> list[Asset]:
    """Every Meshy asset whose concept was picked but never made into a model
    (no successful Meshy model task from that pick), in requests.json order."""
    root = meshy_root or MESHY
    spec = json.loads((root / "requests.json").read_text(encoding="utf-8"))
    tasks = json.loads((root / "ledger.json").read_text(encoding="utf-8"))["tasks"]
    picks = {}
    for t in tasks:  # meshy.py: the latest picked, finished concept
        if t.get("stage") == "concept" and t.get("picked") and t.get("status") == "SUCCEEDED":
            picks[t["asset"]] = t
    made = {(t["asset"], t.get("concept")) for t in tasks if t.get("stage") == "model" and t.get("status") == "SUCCEEDED"}
    found = []
    for a in spec["assets"]:
        retired = set(a.get("retired", []))
        for variant, extra in (a.get("variants") or {"": a["prompt"]}).items():
            if variant in retired:
                continue
            key = f"{a['id']}_{variant}" if variant else a["id"]
            pick = picks.get(key)
            if pick is None or (key, pick["task_id"]) in made:
                continue
            prompt = a["prompt"] if not variant else f"{a['prompt']}, {extra}"
            found.append(Asset(a["id"], a["kind"], variant, prompt, source="meshy", batch=MESHY_BATCH,
                               age=a.get("age"), faction=a.get("faction"), magic=a.get("magic", False),
                               rig=a.get("rig", False), face_limit=a.get("polycount"), framing=a.get("framing"),
                               prompt_suffix=None, meshy_concept=pick["task_id"], meshy_candidate=pick.get("candidate", 0)))
    return found


def load_assets(only: set[str] | None = None, batch: str | None = None, style: dict | None = None,
                meshy_root: Path | None = None) -> list[Asset]:
    """The requested Tripo assets and the unfinished Meshy picks (batch
    meshy-finish), filtered by --batch and --only (ids or keys)."""
    style = load_style() if style is None else style
    assets = load_requests(style, meshy_root)
    tripo_keys = {a.key for a in assets}
    for a in meshy_finish(meshy_root):
        if a.key in tripo_keys:  # load_requests refuses Meshy names, so this cannot happen
            raise TripoError(f"{a.key}: both a Tripo request and a Meshy pick")
        assets.append(a)
    picked = [a for a in assets if (batch is None or a.batch == batch) and (only is None or a.id in only or a.key in only)]
    if only:
        unmatched = sorted(n for n in only if not any(n in (a.id, a.key) for a in picked))
        if unmatched:
            where = f" in batch {batch}" if batch else ""
            raise TripoError(f"--only {', '.join(unmatched)}: no such asset{where} (art/tripo/requests.json or batch {MESHY_BATCH})")
    return picked


def find_asset(assets: list[Asset], name: str) -> Asset:
    """The one asset `name` means: its key, or the only live variant of an id."""
    matches = [a for a in assets if a.key == name] or [a for a in assets if a.id == name]
    if len(matches) > 1:
        raise TripoError(f"{name} could be {', '.join(a.key for a in matches)}: name one")
    if not matches:
        raise TripoError(f"{name}: no such asset (art/tripo/requests.json or batch {MESHY_BATCH}; retired variants are never made again)")
    return matches[0]


# --------------------------------------------------------------------------
# Style and prompts
# --------------------------------------------------------------------------

def sentence(text: str) -> str:
    text = " ".join(text.split())
    return text if text[-1] in ".!?" else text + "."


def style_text(style: dict, section: str, name: str, asset: Asset) -> str:
    text = (style.get(section) or {}).get(name)
    if not isinstance(text, str):
        raise TripoError(f"{asset.key}: style.json {section} has no {name}")
    return text


def layers(asset: Asset, style: dict) -> list[str]:
    """Faction, its magic, age: as meshy.py layers them."""
    parts = []
    if asset.faction:
        parts.append(style_text(style, "factions", asset.faction, asset))
        if asset.magic:
            parts.append(style_text(style, "magic", asset.faction, asset))
    if asset.age:
        parts.append(style_text(style, "ages", str(asset.age), asset))
    return parts


def kind_text(asset: Asset, style: dict) -> str:
    return asset.prompt_suffix or style_text(style, "kinds", asset.kind, asset)


def look(asset: Asset, style: dict) -> str:
    """The global look, or the kind's own (decor drops "muted")."""
    return (style.get("kind_look") or {}).get(asset.kind) or style.get("look", "")


def join(parts: list[str]) -> str:
    return " ".join(sentence(p) for p in parts if p and p.strip())


def concept_prompt(asset: Asset, style: dict, references: bool) -> str:
    """Framing, subject, faction (and its magic), age, kind and look, as
    sentences, exactly as meshy.py builds them."""
    parts = [STYLE_ONLY] if references else []
    parts += [style_text(style, "framing", asset.framing, asset), f"Subject: {asset.prompt}"]
    parts += layers(asset, style) + [kind_text(asset, style), look(asset, style)]
    text = join(parts)
    if len(text) > MAX_IMAGE_PROMPT:
        raise TripoError(f"{asset.key}: concept prompt is {len(text)} characters (max {MAX_IMAGE_PROMPT}); shorten its prompt or the style texts")
    return text


def text_prompt(asset: Asset, style: dict, trim: bool = False) -> str:
    """The text-to-model prompt: subject, faction, magic, age, kind and the
    short text look (Tripo takes at most 1024 characters). With `trim`, the
    look, kind, age, magic and faction layers are left out, in that order,
    until it fits (for a Studio kit's fallback prompt)."""
    t = style.get("text_model") or {}
    parts = [asset.prompt] + layers(asset, style) + [kind_text(asset, style), t.get("look") or look(asset, style)]
    text = join(parts)
    while trim and len(text) > MAX_TEXT_PROMPT and len(parts) > 1:
        parts.pop()
        text = join(parts)
    if len(text) > MAX_TEXT_PROMPT:
        if trim:
            return text[:MAX_TEXT_PROMPT].rsplit(" ", 1)[0]
        raise TripoError(f"{asset.key}: text-to-model prompt is {len(text)} characters (max {MAX_TEXT_PROMPT}); shorten its prompt")
    return text


def checked_options(section: dict, allowed: tuple, name: str) -> dict:
    unknown = sorted(k for k in section if k not in allowed and not k.startswith("_") and k not in ("negative_prompt", "look"))
    if unknown:
        raise TripoError(f"style.json {name}: unknown option {', '.join(unknown)} (tripo.py knows {', '.join(allowed)})")
    if section.get("model") not in MODEL_VERSIONS:
        raise TripoError(f"style.json {name}.model must be one of {', '.join(MODEL_VERSIONS)}, not {section.get('model')!r}")
    for flag in ("texture", "pbr", "delight", "enable_image_autofix", "auto_size", "export_uv"):
        if flag in section and not isinstance(section[flag], bool):
            raise TripoError(f"style.json {name}.{flag} must be true or false")
    if section.get("texture_quality", "standard") not in TEXTURE_QUALITIES:
        raise TripoError(f"style.json {name}.texture_quality must be one of {', '.join(TEXTURE_QUALITIES)}")
    if "texture_version" in section and section["texture_version"] not in TEXTURE_VERSIONS:
        raise TripoError(f"style.json {name}.texture_version must be one of {', '.join(TEXTURE_VERSIONS)}")
    if section.get("texture_quality") == "fast" and section.get("texture_version") != "v3.5-20260815":
        raise TripoError(f"style.json {name}: texture_quality fast needs texture_version v3.5-20260815")
    if section.get("geometry_quality", "standard") not in ("standard", "detailed"):
        raise TripoError(f"style.json {name}.geometry_quality must be standard or detailed")
    if section.get("orientation", "default") not in ("default", "align_image"):
        raise TripoError(f"style.json {name}.orientation must be default or align_image")
    if section.get("texture_alignment", "original_image") not in ("original_image", "geometry"):
        raise TripoError(f"style.json {name}.texture_alignment must be original_image or geometry")
    if section.get("compress", "geometry") != "geometry":
        raise TripoError(f"style.json {name}.compress can only be geometry")
    return section


def model_style(style: dict) -> dict:
    m = style.get("model")
    if not isinstance(m, dict):
        raise TripoError("style.json has no model section (image to model: model, texture_quality...)")
    return checked_options(m, MODEL_OPTIONS, "model")


def text_style(style: dict) -> dict:
    t = style.get("text_model")
    if not isinstance(t, dict):
        raise TripoError("style.json has no text_model section (text to model)")
    checked_options(t, TEXT_OPTIONS, "text_model")
    if len(t.get("negative_prompt", "")) > MAX_NEGATIVE_PROMPT:
        raise TripoError(f"style.json text_model.negative_prompt is over {MAX_NEGATIVE_PROMPT} characters")
    return t


def concept_style(style: dict) -> dict:
    c = style.get("concept")
    if not isinstance(c, dict):
        raise TripoError("style.json has no concept section (concepts need concept.model, candidates and aspect)")
    if c.get("model") not in IMAGE_MODELS:
        raise TripoError(f"style.json concept.model must be one of {', '.join(IMAGE_MODELS)}, not {c.get('model')!r}")
    if c.get("quality", "low") not in ("low", "medium"):
        raise TripoError("style.json concept.quality: only low and medium have a known price (the base price)")
    if "quality" in c and not c["model"].startswith("chat_image_2"):
        raise TripoError("style.json concept.quality is only for chat_image_2 models (others reject it)")
    if not is_int(c.get("candidates", 2)) or c.get("candidates", 2) < 1:
        raise TripoError("style.json concept.candidates must be a whole number of at least 1")
    if c["model"] in BANANA:
        aspect = c.get("aspect")
        if not isinstance(aspect, dict) or any(aspect.get(f) not in BANANA_ASPECTS for f in FRAMINGS):
            raise TripoError(f"style.json concept.aspect must give {', '.join(FRAMINGS)} each one of {', '.join(BANANA_ASPECTS)}")
    elif "aspect" in c:
        raise TripoError("style.json concept.aspect is only for the banana models (the others size by size)")
    if "size" in c and not isinstance(c["size"], str):
        raise TripoError("style.json concept.size must be text such as 2K")
    framing = style.get("framing")
    if not isinstance(framing, dict) or not all(isinstance(framing.get(f), str) and framing[f].strip() for f in FRAMINGS):
        raise TripoError(f"style.json framing must give a text for each of {', '.join(FRAMINGS)}")
    for section in ("ages", "factions", "magic", "kinds", "kind_look"):
        texts = style.get(section, {})
        if not isinstance(texts, dict) or not all(isinstance(v, str) for k, v in texts.items() if not k.startswith("_")):
            raise TripoError(f"style.json {section} must map names to texts")
    return c


def face_limit(asset: Asset, style: dict) -> int:
    if asset.face_limit is not None:
        return asset.face_limit
    limits = style.get("face_limit") or {}
    if not is_int(limits.get(asset.kind)):
        raise TripoError(f"{asset.key}: style.json face_limit has no {asset.kind} (or give the asset a face_limit)")
    return limits[asset.kind]


def model_cost(path: str, m: dict) -> int:
    """Credits one model costs with these settings (reserved before it starts)."""
    textured = m.get("texture", True) or m.get("pbr", True)
    quality = m.get("texture_quality", "standard") if textured else "none"
    cost = MODEL_PRICE[path]["detailed" if quality == "extreme" else quality]
    if quality == "extreme":
        cost += EXTREME_EXTRA
    if m.get("pbr", True):
        cost += PBR_EXTRA
    if m.get("geometry_quality", "standard") == "detailed":
        cost += ULTRA_EXTRA
    return cost


def reference_files(kind: str, style: dict, model: str) -> list[Path]:
    """The style reference images for a kind (style.json references), checked."""
    refs = (style.get("references") or {}).get(kind) or []
    if not isinstance(refs, list) or not all(isinstance(r, str) and r for r in refs):
        raise TripoError(f"style.json references.{kind} must be a list of image paths")
    most = MAX_INPUTS.get(model, 16 if model.startswith("chat_image") else 0)
    if len(refs) > most:
        raise TripoError(f"style.json references.{kind} lists {len(refs)} images; {model} image to image takes at most {most}")
    paths = []
    for r in refs:
        p = ROOT / r
        if p.suffix.lower() not in IMAGE_TYPES:
            raise TripoError(f"style.json references.{kind}: {r} is not a {', '.join(IMAGE_TYPES)} image")
        if not p.is_file():
            raise TripoError(f"style.json references.{kind}: {r} does not exist")
        if p.stat().st_size > MAX_IMAGE_BYTES:
            raise TripoError(f"style.json references.{kind}: {r} is over 20 MB")
        paths.append(p)
    return paths


_digests: dict[tuple, str] = {}


def file_digest(path: Path) -> str:
    st = path.stat()
    key = (str(path), st.st_mtime_ns, st.st_size)
    if key not in _digests:
        _digests[key] = hashlib.sha256(path.read_bytes()).hexdigest()
    return _digests[key]


def prompt_hash(*parts: str) -> str:
    return hashlib.sha256("\x1f".join(parts).encode("utf-8")).hexdigest()[:16]


# --------------------------------------------------------------------------
# Requests: what one paid task asks for, and what it costs
# --------------------------------------------------------------------------

UPLOAD = re.compile(r"upload:(\d+)")


@dataclass(frozen=True)
class Request:
    """One paid task. `body` is what is posted, except that "upload:<n>"
    stands for the file token of uploads[n], uploaded just before (the hash
    names the files by their bytes, so a new upload is the same request)."""

    stage: str  # concept | model | text_model
    path: str
    body: dict
    uploads: tuple[Path, ...]
    hash: str
    cost: int
    fields: dict  # what else the ledger entry records

    @property
    def rank(self) -> int:
        """Later stages start first when the queue has room."""
        return 0 if self.stage == "concept" else 1

    def filled(self, tokens: list[str]) -> dict:
        def fill(v):
            m = UPLOAD.fullmatch(v) if isinstance(v, str) else None
            return tokens[int(m.group(1))] if m else v
        return {k: [fill(x) for x in v] if isinstance(v, list) else fill(v) for k, v in self.body.items()}

    def shown(self) -> dict:
        return self.filled([f"<file_token of {rel(p)}>" for p in self.uploads])


def concept_request(asset: Asset, style: dict, candidate: int) -> Request:
    c = concept_style(style)
    refs = reference_files(asset.kind, style, c["model"])
    path = IMAGE_TO_IMAGE if refs else TEXT_TO_IMAGE
    if path == IMAGE_TO_IMAGE and c["model"] == "seedream_v4":
        raise TripoError("style.json concept.model seedream_v4 cannot do image to image (references): use seedream_v5 or banana_pro")
    body = {"model": c["model"], "prompt": concept_prompt(asset, style, bool(refs))}
    if len(refs) == 1:
        body["input"] = "upload:0"
    elif refs:
        body["inputs"] = [f"upload:{i}" for i in range(len(refs))]
    if c.get("size"):
        body["size"] = c["size"]
    if c["model"] in BANANA:
        body["aspect_ratio"] = c["aspect"][asset.framing]
    if c.get("quality"):
        body["quality"] = c["quality"]
    body["output_format"] = "png"
    # The same prompt, model, settings, reference bytes and candidate number
    # is the same concept: a rerun finds it instead of paying again.
    phash = prompt_hash(path, json.dumps(body, sort_keys=True), *(file_digest(p) for p in refs), str(candidate))
    fields = {"candidate": candidate, "api": API_NAME[path], "model": c["model"]}
    if refs:
        fields["references"] = [rel(p) for p in refs]
    return Request("concept", path, body, tuple(refs), phash, IMAGE_PRICE, fields)


def concept_image(key: str, candidate, task_id: str) -> Path:
    return OUT / key / f"concept_{candidate}_{task_id}.png"


def meshy_image(asset: Asset, meshy_out: Path | None = None) -> Path:
    return (meshy_out or MESHY_OUT) / asset.key / f"concept_{asset.meshy_candidate}_{asset.meshy_concept}.png"


def check_image(path: Path, what: str, meshy: bool) -> None:
    if not path.is_file():
        hint = ("Meshy concepts are only in the main checkout's art/meshy/out (git-ignored): run there, or pass --meshy-out"
                if meshy else "its download is gone; `concepts --only <key>` fetches it again without paying")
        raise TripoError(f"{what}: the concept image {rel(path)} is missing ({hint})")
    if path.suffix.lower() not in IMAGE_TYPES or path.stat().st_size > MAX_IMAGE_BYTES:
        raise TripoError(f"{what}: {rel(path)} must be a PNG or JPEG of at most 20 MB")


def effective_pick(asset: Asset, ledger: "Ledger") -> dict | None:
    """The concept `models` makes: a Tripo pick, else the asset's Meshy pick."""
    t = ledger.picked(asset.key)
    if t is not None:
        return {"source": "tripo", "task_id": t["task_id"], "candidate": t.get("candidate"), "created": t.get("created", 0)}
    if asset.meshy_concept:
        return {"source": "meshy", "task_id": asset.meshy_concept, "candidate": asset.meshy_candidate}
    return None


def model_request(asset: Asset, style: dict, pick: dict, meshy_out: Path | None = None, now: float | None = None) -> Request:
    """Image to model from a picked concept. A Tripo concept is passed by its
    task id while it is fresh (else its download is uploaded); a Meshy
    concept image is uploaded. Either way the hash names the concept, not
    how it travels."""
    m = model_style(style)
    if pick["source"] == "meshy":
        image = meshy_image(asset, meshy_out)
        uploads, source = (image,), "upload:0"
        identity = f"meshy-concept:{pick['task_id']}"
        fields = {"concept": pick["task_id"], "concept_from": "meshy", "image": f"art/meshy/out/{asset.key}/{image.name}"}
    else:
        image = concept_image(asset.key, pick["candidate"], pick["task_id"])
        now = time.time() if now is None else now
        old = now - (pick.get("created") or 0) > CONCEPT_TASK_HOURS * 3600
        uploads, source = ((image,), "upload:0") if old and image.is_file() else ((), pick["task_id"])
        identity = f"tripo-concept:{pick['task_id']}"
        fields = {"concept": pick["task_id"], "concept_from": "tripo"}
    body = {"input": source}
    body.update({k: m[k] for k in MODEL_OPTIONS if k in m})
    body["face_limit"] = face_limit(asset, style)
    phash = prompt_hash(IMAGE_TO_MODEL, json.dumps({**body, "input": identity}, sort_keys=True))
    fields.update(api=API_NAME[IMAGE_TO_MODEL], model=m["model"])
    return Request("model", IMAGE_TO_MODEL, body, uploads, phash, model_cost(IMAGE_TO_MODEL, m), fields)


def text_request(asset: Asset, style: dict) -> Request:
    t = text_style(style)
    body = {"prompt": text_prompt(asset, style)}
    if t.get("negative_prompt"):
        body["negative_prompt"] = t["negative_prompt"]
    body.update({k: t[k] for k in TEXT_OPTIONS if k in t})
    body["face_limit"] = face_limit(asset, style)
    phash = prompt_hash(TEXT_TO_MODEL, json.dumps(body, sort_keys=True))
    fields = {"api": API_NAME[TEXT_TO_MODEL], "model": t["model"]}
    return Request("text_model", TEXT_TO_MODEL, body, (), phash, model_cost(TEXT_TO_MODEL, t), fields)


# --------------------------------------------------------------------------
# Secrets and HTTP
# --------------------------------------------------------------------------

def api_key(required: bool = True) -> str:
    key = os.environ.get("TRIPO_API_KEY", "").strip()
    env = ROOT / ".env"
    if not key and env.exists():
        for line in env.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line.startswith("TRIPO_API_KEY="):
                key = line.split("=", 1)[1].strip().strip('"').strip("'")
    if not key and required:
        raise TripoError("no TRIPO_API_KEY (set it in the environment or in .env at the repo root)")
    return key


Http = Callable[..., object]  # http(method, path, body=None, upload=None) -> the answer's data


def urlopen(req, timeout):
    return urllib.request.urlopen(req, timeout=timeout)


def header(headers, name: str) -> float | None:
    value = headers.get(name) if headers is not None else None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


class RateLimits:
    """Per endpoint: no request before the time Tripo's rate-limit headers
    say the window resets (X-RateLimit-Remaining 0, or a 429)."""

    def __init__(self, sleep: Callable[[float], None], clock: Callable[[], float]):
        self.sleep, self.clock = sleep, clock
        self.until: dict[str, float] = {}
        self.lock = threading.Lock()

    def hold(self, bucket: str, until: float) -> None:
        with self.lock:
            self.until[bucket] = max(self.until.get(bucket, 0.0), until)

    def wait(self, bucket: str) -> None:
        with self.lock:
            until = self.until.get(bucket, 0.0)
        delay = min(until - self.clock(), MAX_WAIT)
        if delay > 0:
            self.sleep(delay)

    def seen(self, bucket: str, headers) -> None:
        remaining, reset = header(headers, "X-RateLimit-Remaining"), header(headers, "X-RateLimit-Reset")
        if remaining is not None and remaining <= 0 and reset is not None:
            self.hold(bucket, min(reset, self.clock() + MAX_WAIT))

    def limited(self, bucket: str, headers, attempt: int) -> float:
        """A 429: hold the endpoint for Retry-After, else until X-RateLimit-Reset,
        else 1, 2, 4... 32 seconds (the docs' backoff)."""
        now = self.clock()
        after, reset = header(headers, "Retry-After"), header(headers, "X-RateLimit-Reset")
        wait = after if after is not None else max(reset - now, 1.0) if reset is not None else float(min(2 ** attempt, 32))
        wait = min(max(wait, 0.5), MAX_WAIT)
        self.hold(bucket, now + wait)
        return wait


def multipart(path: Path) -> tuple[bytes, str]:
    boundary = "----tripo" + uuid.uuid4().hex
    ctype = IMAGE_TYPES.get(path.suffix.lower(), "application/octet-stream")
    head = (f"--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{path.name}\"\r\n"
            f"Content-Type: {ctype}\r\n\r\n").encode("utf-8")
    return head + path.read_bytes() + f"\r\n--{boundary}--\r\n".encode("utf-8"), f"multipart/form-data; boundary={boundary}"


def unsent(reason) -> bool:
    """A connection error that proves the request never reached Tripo."""
    return isinstance(reason, (ConnectionRefusedError, socket.gaierror))


def make_http(key: str, sleep: Callable[[float], None] = time.sleep, clock: Callable[[], float] = time.time) -> Http:
    limits = RateLimits(sleep, clock)

    def scrub(text: str) -> str:
        return text.replace(key, "<key>") if key else text

    def call(method: str, path: str, body: dict | None = None, upload: Path | None = None):
        creates = method == "POST" and path not in (TASKS_LIST, FILES)
        bucket = path.split("?", 1)[0]
        if bucket.startswith("/tasks/") and bucket != TASKS_LIST:
            bucket = "/tasks"
        if upload is not None:
            data, ctype = multipart(upload)
        elif body is not None:
            data, ctype = json.dumps(body).encode("utf-8"), "application/json"
        else:
            data, ctype = None, None
        last = "no answer"
        for attempt in range(HTTP_TRIES):
            limits.wait(bucket)
            req = urllib.request.Request(API + path, data=data, method=method)
            req.add_header("Authorization", "Bearer " + key)
            if ctype:
                req.add_header("Content-Type", ctype)
            try:
                with urlopen(req, HTTP_TIMEOUT) as r:
                    limits.seen(bucket, r.headers)
                    status, headers, raw = 200, r.headers, r.read()
            except urllib.error.HTTPError as e:
                status, headers, raw = e.code, e.headers, e.read()
            except (OSError, HTTPException) as e:  # URLError, timeouts, resets, dropped connections
                reason = getattr(e, "reason", e)
                last = scrub(f"{type(reason).__name__}: {reason}")
                if creates and not unsent(reason):
                    raise Uncertain(f"{method} {path}: the connection failed after the request was sent ({last})") from None
                sleep(min(2 ** attempt, 32))
                continue
            try:
                payload = json.loads(raw.decode("utf-8", "replace"))
            except ValueError:
                payload = None
            if not isinstance(payload, dict):
                payload = {}
            code = payload.get("code")
            if status == 200 and code == 0:
                return payload.get("data")
            message = scrub(" ".join(str(payload.get(k)) for k in ("message", "suggestion") if payload.get(k))
                            or raw[:300].decode("utf-8", "replace"))
            if code == 2000:
                raise QueueFull(f"{method} {path}: {message}", header(headers, "Retry-After"))
            if code == 1007 or (status == 429 and code is None):
                # Rate limited: nothing was created, so even a create is safe to send again.
                wait = limits.limited(bucket, headers, attempt)
                last = f"HTTP {status} code {code}: {message} (waited {wait:.0f} s)"
                continue
            if status == 401:
                raise TripoError(f"{method} {path}: HTTP 401: the API key was rejected (keys from developers.tripo3d.ai work on {API})")
            if code == 2010:
                raise NoCredits(f"{method} {path}: not enough credits (code 2010): {message}")
            if status >= 500 or not payload:
                if creates:
                    raise Uncertain(f"{method} {path}: HTTP {status} (code {code}): {message}")
                last = f"HTTP {status}: {message}"
                sleep(min(2 ** attempt, 32))
                continue
            raise TripoError(f"{method} {path}: HTTP {status}, code {code}: {message}")
        raise TripoError(f"{method} {path}: gave up after {HTTP_TRIES} tries ({last})")

    return call


def download(url: str, to: Path) -> None:
    """One try at a signed link (no key goes with it)."""
    to.parent.mkdir(parents=True, exist_ok=True)
    tmp = to.with_suffix(to.suffix + ".part")
    try:
        with urllib.request.urlopen(url, timeout=300) as r, open(tmp, "wb") as f:
            shutil.copyfileobj(r, f)
    # URLError and HTTPError (an expired link: 403) are OSErrors; a connection
    # dropped mid-file is an HTTPException (IncompleteRead).
    except (OSError, HTTPException) as e:
        tmp.unlink(missing_ok=True)
        raise DownloadError(f"downloading {to.name} failed ({e})") from None
    tmp.replace(to)


def fetch(url: str, to: Path, sleep: Callable[[float], None]) -> None:
    for attempt in range(DOWNLOAD_TRIES):
        try:
            download(url, to)
            return
        except DownloadError as e:
            last = e
            if attempt + 1 < DOWNLOAD_TRIES:
                sleep(2 ** attempt)
    raise last


def unsigned(url: str | None) -> str | None:
    """A link without its signature (the query string), for the committed ledger."""
    return url.split("?", 1)[0] if url else url


# --------------------------------------------------------------------------
# Ledger
# --------------------------------------------------------------------------

class Ledger:
    """Every task ever submitted: what it was for, its state and its cost.
    Safe to use from the worker threads."""

    def __init__(self, path: Path = LEDGER):
        self.path = path
        self.tasks: list[dict] = json.loads(path.read_text(encoding="utf-8"))["tasks"] if path.exists() else []
        self.lock = threading.RLock()

    def save(self) -> None:
        with self.lock:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            tmp = self.path.with_suffix(".tmp")
            # One task a line: fast to write after every change, and a task's
            # change is a one-line diff.
            lines = ",\n".join("  " + json.dumps(t, ensure_ascii=False) for t in self.tasks)
            tmp.write_text(f'{{"tasks": [\n{lines}\n]}}\n' if self.tasks else '{"tasks": []}\n', encoding="utf-8")
            for attempt in range(6):  # a sync client (OneDrive) can hold the file a moment
                try:
                    tmp.replace(self.path)
                    return
                except PermissionError:
                    if attempt == 5:
                        raise
                    time.sleep(0.2 * (attempt + 1))

    def add(self, **entry) -> dict:
        with self.lock:
            self.tasks.append(entry)
            self.save()
            return entry

    def get(self, task_id: str) -> dict:
        with self.lock:
            return next(t for t in self.tasks if t["task_id"] == task_id)

    def update(self, task_id: str, **fields) -> dict:
        with self.lock:
            t = self.get(task_id)
            t.update(fields)
            self.save()
            return t

    def find(self, key: str, stage: str, phash: str) -> list[dict]:
        """The tasks of this exact request that are not known to have failed."""
        with self.lock:
            return [t for t in self.tasks if t["asset"] == key and t["stage"] == stage and t["hash"] == phash
                    and (t["status"] in IN_FLIGHT or t["status"] in (SUCCEEDED, UNCERTAIN))]

    def spent(self) -> int | float:
        with self.lock:
            return num(sum(t.get("credits", 0) or 0 for t in self.tasks))

    def picked(self, key: str) -> dict | None:
        with self.lock:
            return next((t for t in reversed(self.tasks) if t["asset"] == key and t["stage"] == "concept"
                         and t.get("picked") and t["status"] == SUCCEEDED), None)

    def pick(self, task_id: str) -> dict:
        with self.lock:
            chosen = self.get(task_id)
            for t in self.tasks:
                if t["asset"] == chosen["asset"] and t["stage"] == "concept":
                    t.pop("picked", None)
            chosen["picked"] = True
            self.save()
            return chosen

    def studio(self, key: str) -> dict | None:
        with self.lock:
            return next((t for t in reversed(self.tasks) if t["asset"] == key and t["stage"] == "studio"), None)


def studio_made(asset: Asset, ledger: Ledger) -> bool:
    """Installed by import-studio, and still there."""
    t = ledger.studio(asset.key)
    return t is not None and t.get("installed") == rel(asset.target()) and asset.target().exists()


class Budget:
    def __init__(self, cap: float, why: str | None = None):
        self.cap = cap
        self.why = why or f"--max-credits {cap}"
        self.used = 0

    def can(self, credits: int) -> bool:
        return self.used + credits <= self.cap

    def take(self, credits: int, what: str) -> None:
        if not self.can(credits):
            raise TripoError(f"stopping before {what}: it would take {self.used + credits} credits, over {self.why}")
        self.used += credits


# --------------------------------------------------------------------------
# Jobs
# --------------------------------------------------------------------------

WORDS = {"concept": "concept", "model": "model", "text_model": "text model"}


@dataclass
class Job:
    """One paid task for one asset (a concept candidate, or its model), and
    what came of it."""

    asset: Asset
    req: Request
    candidate: int = 0
    task: dict | None = None  # its ledger entry once started
    done: Path | None = None
    error: str | None = None
    busy: bool = False  # a worker is submitting it or fetching its files
    refetch: bool = False  # finished, but its files are still to fetch: query it for fresh links
    fetches: int = 0
    printed: tuple | None = None

    @property
    def stage(self) -> str:
        return self.req.stage

    @property
    def word(self) -> str:
        return WORDS[self.stage]

    @property
    def label(self) -> str:
        return f"{self.asset.key}#{self.candidate}" if self.stage == "concept" else self.asset.key

    @property
    def finished(self) -> bool:
        return self.error is not None or self.done is not None

    @property
    def cost(self) -> int:
        """Credits this job still has to pay."""
        return 0 if self.task is not None else self.req.cost

    def raw(self) -> Path:
        tid = self.task["task_id"]
        if self.stage == "concept":
            return concept_image(self.asset.key, self.candidate, tid)
        return OUT / self.asset.key / f"model_{tid}.glb"

    def made(self) -> bool:
        if self.task is None or self.task["status"] != SUCCEEDED:
            return False
        if self.stage == "concept":
            return self.raw().exists()
        return self.task.get("installed") == rel(self.asset.target()) and self.asset.target().exists()

    def describe(self) -> str:
        return f"{self.word} {self.task['status']} (task {self.task['task_id']})" if self.task else f"no {self.word} task"


def attach(job: Job, ledger: Ledger) -> Job:
    """Pick up the task an earlier run started for exactly this request."""
    found = ledger.find(job.asset.key, job.stage, job.req.hash)
    job.task = found[-1] if found else None
    if job.made():
        job.done = job.raw() if job.stage == "concept" else job.asset.target()
    elif job.task is not None and job.task["status"] == UNCERTAIN:
        job.error = uncertain_text(job)
    return job


def uncertain_text(job: Job) -> str:
    when = time.strftime("%Y-%m-%d %H:%M", time.localtime(job.task.get("created", 0)))
    return (f"{job.label}: the {job.word} request of {when} may or may not have made a task ({job.task.get('error')}); "
            f"`tripo.py usage` lists the account's tasks: `tripo.py resolve {job.asset.key} <task_id>` records the one "
            f"it made, `tripo.py resolve {job.asset.key} none` lets the next run make it")


def concept_jobs(asset: Asset, style: dict, ledger: Ledger, candidates: int) -> list[Job]:
    return [attach(Job(asset, concept_request(asset, style, n), candidate=n), ledger) for n in range(candidates)]


def model_job(asset: Asset, pick: dict, style: dict, ledger: Ledger, meshy_out: Path | None = None,
              now: float | None = None) -> Job:
    return attach(Job(asset, model_request(asset, style, pick, meshy_out, now)), ledger)


def text_job(asset: Asset, style: dict, ledger: Ledger) -> Job:
    return attach(Job(asset, text_request(asset, style)), ledger)


def install(job: Job, ledger: Ledger) -> Path:
    """Copy a downloaded model to where its pipeline reads it, and record it."""
    target = job.asset.target()
    target.parent.mkdir(parents=True, exist_ok=True)
    tmp = target.with_suffix(".glb.part")
    shutil.copyfile(job.raw(), tmp)
    tmp.replace(target)
    ledger.update(job.task["task_id"], installed=rel(target))
    return target


def from_disk(job: Job, ledger: Ledger) -> bool:
    """Install (or find) a finished task's file from art/tripo/out/."""
    if not job.raw().exists():
        return False
    job.done = job.raw() if job.stage == "concept" else install(job, ledger)
    return True


# --------------------------------------------------------------------------
# The scheduler
# --------------------------------------------------------------------------

class InlinePool:
    """An executor that runs each call at once (for tests: deterministic)."""

    def submit(self, fn, *args):
        f = cf.Future()
        try:
            f.set_result(fn(*args))
        except Exception as e:  # noqa: BLE001 - handed to the caller as a real pool would
            f.set_exception(e)
        return f

    def shutdown(self, wait=True, cancel_futures=False):
        pass


class Uploads:
    """File tokens for this run: each file uploaded once (POST /files is
    free; Tripo allows about 10 uploads a minute, and 429s are waited out)."""

    def __init__(self, http: Http):
        self.http = http
        self.tokens: dict[Path, str] = {}
        self.locks: dict[Path, threading.Lock] = {}
        self.lock = threading.Lock()

    def get(self, path: Path) -> str:
        with self.lock:
            lock = self.locks.setdefault(path, threading.Lock())
        with lock:
            if path not in self.tokens:
                data = self.http("POST", FILES, None, upload=path) or {}
                token = data.get("file_token")
                if not token:
                    raise TripoError(f"uploading {path.name}: Tripo sent no file_token")
                self.tokens[path] = token
            return self.tokens[path]


def status_of(r: dict) -> str:
    """Tripo's status; anything it does not document counts as failed (the
    migration guide's advice: banned, expired, unknown)."""
    s = str(r.get("status", "")).lower()
    return s if s in IN_FLIGHT + (SUCCEEDED, "failed", "cancelled") else (s or "failed")


def task_error(r: dict) -> str | None:
    if r.get("error_message") or r.get("error_code"):
        return f"{r.get('error_message') or 'error'} (code {r.get('error_code')})"
    return None


class Scheduler:
    """Drives jobs to the end: at most `concurrency` tasks in flight, later
    stages first, submissions and downloads on a pool of workers, every task
    polled through POST /tasks/list. A round that can neither start, fetch
    nor wait for anything fails every unfinished job instead of polling
    forever. A job whose task timed out is left alone: a rerun resumes it."""

    def __init__(self, http: Http, ledger: Ledger, jobs: list[Job], concurrency: int,
                 sleep: Callable[[float], None] = time.sleep, clock: Callable[[], float] = time.time,
                 workers: int = IO_WORKERS):
        if concurrency < 1:
            raise TripoError(f"--concurrency is {concurrency}: nothing could ever start")
        self.http, self.ledger, self.jobs, self.concurrency = http, ledger, jobs, concurrency
        self.sleep, self.clock = sleep, clock
        self.pool = InlinePool() if workers == 0 else cf.ThreadPoolExecutor(max_workers=min(workers, concurrency) or 1)
        self.futures: dict[cf.Future, tuple[str, Job]] = {}
        self.cap: dict[str, int] = {}  # category -> tasks Tripo took at once before refusing more
        self.not_before: dict[str, float] = {}
        self.no_credits: str | None = None
        self.uploads = Uploads(http)
        self.warnings: list[str] = []
        self.said: set[str] = set()
        self.last_poll = float("-inf")

    # -- counting ---------------------------------------------------------

    def in_flight(self, category: str | None = None) -> int:
        """Our tasks in flight, and submissions under way."""
        n = 0
        for j in self.jobs:
            if category is not None and CATEGORY[j.req.path] != category:
                continue
            if j.task is not None and j.task["status"] in IN_FLIGHT and not j.finished:
                n += 1
            elif j.task is None and j.busy:
                n += 1
        return n

    # -- the loop ---------------------------------------------------------

    def run(self) -> None:
        clean = False
        try:
            while not all(j.finished for j in self.jobs):
                progressed = self.collect()
                progressed |= self.recover()
                progressed |= self.start()
                progressed |= self.collect()
                progressed |= self.poll()
                waiting = bool(self.futures) or any(t > self.clock() for t in self.not_before.values())
                if not progressed and not waiting:
                    for j in self.jobs:
                        if not j.finished:
                            j.error = f"{j.label}: stuck with {j.describe()}; nothing to start or wait for (tripo.py does not handle this state)"
                    break
                if not all(j.finished for j in self.jobs):
                    self.wait()
            clean = True
        finally:
            self.pool.shutdown(wait=True, cancel_futures=not clean)

    def wait(self) -> None:
        if self.futures:  # until a worker finishes, or it is time to poll
            cf.wait(list(self.futures), timeout=POLL_SECONDS, return_when=cf.FIRST_COMPLETED)
            return
        if any(j.task is not None and (j.task["status"] in IN_FLIGHT or j.refetch) and not j.finished for j in self.jobs):
            self.sleep(POLL_SECONDS)
            return
        pending = [t - self.clock() for t in self.not_before.values() if t > self.clock()]
        self.sleep(min(pending + [POLL_SECONDS]))

    def collect(self) -> bool:
        """Apply what the workers finished: successes first, so a refusal
        that came beside them counts them."""
        done = [f for f in self.futures if f.done()]
        for f in sorted(done, key=lambda f: f.exception() is not None):
            what, job = self.futures.pop(f)
            job.busy = False
            try:
                value = f.result()
            except QueueFull as e:
                self.refused(job, e)
            except NoCredits as e:
                self.no_credits = str(e)
                job.error = f"{job.label}: not started: {e}"
            except Uncertain:
                job.task = self.ledger.find(job.asset.key, job.stage, job.req.hash)[-1]
                job.error = uncertain_text(job)
            except DownloadError as e:
                job.fetches += 1
                if job.fetches < REFETCHES:
                    print(f"  {job.label}: {e}; asking Tripo for fresh links", flush=True)
                    job.refetch = True
                else:
                    job.error = (f"{job.label}: {job.word} task {job.task['task_id']} finished but was not "
                                 f"{'downloaded' if job.stage == 'concept' else 'installed'} ({e}); rerun to try again (nothing new is paid)")
            except TripoError as e:
                job.error = f"{job.label}: {e}" if what == "submit" else (
                    f"{job.label}: {job.word} task {job.task['task_id']} finished but was not "
                    f"{'downloaded' if job.stage == 'concept' else 'installed'} ({e}); rerun to try again (nothing new is paid)")
            else:
                if what == "submit":
                    job.task = value
                    print(f"  {job.label}: {job.word} started (task {value['task_id']})", flush=True)
                else:
                    job.done = value
                    print(f"{job.label}: {'downloaded' if job.stage == 'concept' else 'installed'} {rel(value)}", flush=True)
        return bool(done)

    def refused(self, job: Job, e: QueueFull) -> None:
        """Code 2000: the category is full. Wait until one of our tasks in it
        finishes (at most as many at once as were running), or, with none of
        ours in it, for Retry-After."""
        category = CATEGORY[job.req.path]
        running = sum(1 for j in self.jobs if CATEGORY[j.req.path] == category and j.task is not None
                      and j.task["status"] in IN_FLIGHT and not j.finished)
        if running:
            self.cap[category] = min(self.cap.get(category, running), running)
        else:
            self.not_before[category] = self.clock() + (e.retry_after or QUEUE_RETRY_SECONDS)
        if category not in self.said:
            self.said.add(category)
            limit = f" (it took {running} at once)" if running else ""
            print(f"  Tripo's {category} queue is full{limit}: waiting for a task to finish", flush=True)

    def recover(self) -> bool:
        """Finished tasks of an earlier run that are not installed (their file
        deleted, or a run that died while fetching): install them from
        art/tripo/out/, else query them again for fresh links."""
        found = False
        for j in self.jobs:
            if j.finished or j.busy or j.refetch or j.task is None or j.task["status"] != SUCCEEDED:
                continue
            found = True
            if from_disk(j, self.ledger):
                print(f"{j.label}: re{'found' if j.stage == 'concept' else 'installed'} {rel(j.done)} from {j.word} task {j.task['task_id']} (no new credits)", flush=True)
            else:
                j.refetch = True
        return found

    def start(self) -> bool:
        """Start what can start, later stages first, within the concurrency
        and what each category takes."""
        ready = [j for j in self.jobs if not j.finished and j.task is None and not j.busy]
        if self.no_credits:
            for j in ready:
                j.error = f"{j.label}: not started: Tripo said the balance is too low ({self.no_credits})"
            return bool(ready)
        ready.sort(key=lambda j: -j.req.rank)
        total = self.in_flight()
        counts: dict[str, int] = {}
        started = False
        for j in ready:
            if total >= self.concurrency:
                break
            category = CATEGORY[j.req.path]
            if self.not_before.get(category, 0.0) > self.clock():
                continue
            if category not in counts:
                counts[category] = self.in_flight(category)
            if category in self.cap and counts[category] >= self.cap[category]:
                continue
            j.busy = True
            self.futures[self.pool.submit(self.submit, j)] = ("submit", j)
            total += 1
            counts[category] += 1
            started = True
        return started

    def submit(self, job: Job) -> dict:
        """(A worker.) Upload the request's files, create the task and record
        it in the ledger at once."""
        tokens = [self.uploads.get(p) for p in job.req.uploads]
        body = job.req.filled(tokens)
        fields = {k: v for k, v in job.req.fields.items() if k != "candidate"}
        head = {"asset": job.asset.key, "stage": job.stage}
        if job.stage == "concept":
            head["candidate"] = job.candidate
        try:
            data = self.http("POST", job.req.path, body)
            task_id = (data or {}).get("task_id")
            if not task_id:
                raise Uncertain(f"POST {job.req.path}: the answer had no task_id ({str(data)[:120]})")
        except Uncertain as e:
            self.ledger.add(**head, task_id=f"uncertain-{uuid.uuid4().hex[:12]}", **fields, hash=job.req.hash, status=UNCERTAIN,
                            credits=job.req.cost, reserved=job.req.cost, created=int(self.clock()), error=str(e))
            raise
        return self.ledger.add(**head, task_id=task_id, **fields, hash=job.req.hash, status=QUEUED,
                               credits=job.req.cost, reserved=job.req.cost, created=int(self.clock()))

    def poll(self) -> bool:
        """Every task in flight, and every finished one needing fresh links,
        through POST /tasks/list, 100 at a time."""
        watch = {j.task["task_id"]: j for j in self.jobs if not j.finished and not j.busy and j.task is not None
                 and (j.task["status"] in IN_FLIGHT or j.refetch)}
        if not watch:
            return False
        if self.futures and self.clock() - self.last_poll < POLL_SECONDS:
            return False  # workers are busy: poll once every POLL_SECONDS, not after each of them
        self.last_poll = self.clock()
        ids = list(watch)
        for i in range(0, len(ids), MAX_BATCH_QUERY):
            chunk = ids[i:i + MAX_BATCH_QUERY]
            data = self.http("POST", TASKS_LIST, {"task_ids": chunk}) or {}
            tasks, missed = data.get("tasks") or {}, set(data.get("missed") or [])
            for tid in chunk:
                job = watch[tid]
                if tid in missed or tid not in tasks:
                    job.error = f"{job.label}: Tripo no longer knows {job.word} task {tid} (expired?); its ledger entry is kept"
                    continue
                self.update(job, tasks[tid])
        return True

    def update(self, job: Job, r: dict) -> None:
        tid = job.task["task_id"]
        status, progress = status_of(r), r.get("progress") or 0
        if job.task["status"] in IN_FLIGHT and job.printed != (status, int(progress) // 25):
            job.printed = (status, int(progress) // 25)
            print(f"  {job.label} {job.word}: {status} {progress}%", flush=True)
        if status in IN_FLIGHT:
            if status != job.task["status"]:  # queued -> running: saved with the next change, no rewrite of its own
                with self.ledger.lock:
                    job.task["status"] = status
            if self.clock() - job.task.get("created", self.clock()) > TASK_TIMEOUT_SECONDS:
                job.error = f"{job.label}: {job.word} still {status} after {TASK_TIMEOUT_SECONDS // 60} minutes (task {tid}; rerun to resume)"
            return
        if job.task["status"] in IN_FLIGHT:
            # Failed and cancelled tasks are refunded: the ledger keeps what Tripo says was consumed.
            consumed = r.get("credits_consumed")
            credits = num(consumed) if consumed is not None else (job.task.get("reserved", job.req.cost) if status == SUCCEEDED else 0)
            fields = {"status": status, "credits": credits, "error": task_error(r)}
            if consumed is not None:
                fields["credits_consumed"] = num(consumed)
                if float(consumed) > job.task.get("reserved", job.req.cost):
                    self.warnings.append(f"{job.label}: Tripo charged {num(consumed)} credits for a {job.word}, more than the "
                                         f"{job.task.get('reserved')} reserved: raise its price in tripo.py so --max-credits holds")
            out = r.get("output") or {}
            if job.stage == "concept":
                fields["image"] = unsigned(out.get("generated_image_url"))
            else:
                fields["rendered"] = unsigned(out.get("rendered_image_url"))
            self.ledger.update(tid, **fields)
        if status != SUCCEEDED:
            job.error = f"{job.label}: {job.word} {status} ({task_error(r)})"
            return
        job.refetch = False
        job.busy = True
        self.futures[self.pool.submit(self.finish, job, r)] = ("finish", job)

    def finish(self, job: Job, r: dict) -> Path:
        """(A worker.) Download a finished task's files at once (the links
        expire) and install the model."""
        out = r.get("output") or {}
        if job.stage == "concept":
            url = out.get("generated_image_url")
            if not url:
                raise TripoError("the finished task has no generated_image_url")
            fetch(url, job.raw(), self.sleep)
            return job.raw()
        url = out.get("model_url")
        if not url:
            raise TripoError("the finished task has no model_url")
        fetch(url, job.raw(), self.sleep)
        shown = out.get("rendered_image_url")
        if shown:  # only to look at: a failure is a warning
            suffix = Path(unsigned(shown)).suffix.lower()
            try:
                fetch(shown, job.raw().with_suffix(suffix if suffix in (".png", ".jpg", ".jpeg", ".webp") else ".png"), self.sleep)
            except DownloadError as e:
                print(f"  {job.label}: no preview image ({e})", flush=True)
        return install(job, self.ledger)


def run_jobs(http: Http, ledger: Ledger, jobs: list[Job], concurrency: int, sleep: Callable[[float], None] = time.sleep,
             clock: Callable[[], float] = time.time, workers: int = IO_WORKERS) -> Scheduler:
    s = Scheduler(http, ledger, jobs, concurrency, sleep, clock, workers)
    s.run()
    return s


def reserve(job: Job, budget: Budget, jobs: list, skipped: list[str]) -> None:
    """Take the job if the budget can pay for it (reserved up front), else
    list it as not started."""
    if budget.can(job.cost):
        budget.take(job.cost, job.label)
        jobs.append(job)
    else:
        skipped.append(job.label)


def plan_concepts(assets: list[Asset], style: dict, ledger: Ledger, budget: Budget, candidates: int,
                  named: set[str]) -> tuple[list[Job], list[str]]:
    """Concept candidates not made yet. An asset already picked, made, or
    made from text gets none unless --only names it."""
    jobs, skipped = [], []
    for a in assets:
        if a.rig:
            print(f"{a.key}: a rigged humanoid: meshy.py makes it")
            continue
        if not (a.id in named or a.key in named):
            pick = effective_pick(a, ledger)
            if pick is not None:
                where = "in Meshy" if pick["source"] == "meshy" else ""
                print(f"{a.key}: concept {pick['task_id']} is picked {where}; no new candidates (name it in --only for more)".replace("  ", " "))
                continue
            if a.target().exists():
                print(f"{a.key}: already made ({rel(a.target())}); no concepts (name it in --only for some)")
                continue
            if a.route == "text":
                print(f"{a.key}: made from text (`make`); no concepts (name it in --only for some)")
                continue
        for job in concept_jobs(a, style, ledger, candidates):
            if not job.finished:
                reserve(job, budget, jobs, skipped)
            elif job.error:
                jobs.append(job)
    return jobs, skipped


def plan_models(assets: list[Asset], style: dict, ledger: Ledger, budget: Budget, meshy_out: Path | None = None,
                now: float | None = None) -> tuple[list[Job], list[str], list[str]]:
    """Models of picked concepts not made yet; and the assets with no pick."""
    jobs, skipped, unpicked = [], [], []
    for a in assets:
        if a.rig:
            print(f"{a.key}: a rigged humanoid: meshy.py makes it (tripo.py makes no rigs)")
            continue
        if studio_made(a, ledger):
            print(f"{a.key}: made in Tripo Studio ({rel(a.target())})")
            continue
        pick = effective_pick(a, ledger)
        if pick is None:
            if a.route != "text":
                unpicked.append(a.key)
            continue
        job = model_job(a, pick, style, ledger, meshy_out, now)
        if job.done is not None:
            print(f"{a.key}: already made from concept {pick['task_id']}")
            continue
        if placed_elsewhere(job, pick):
            print(f"{a.key}: already there ({rel(a.target())}, made elsewhere); delete it to make it here")
            continue
        if job.task is None and pick["source"] == "meshy":
            try:
                check_image(meshy_image(a, meshy_out), a.key, meshy=True)
            except TripoError as e:
                job.error = str(e)  # this one is not started; the others go on
        if job.error:
            jobs.append(job)
            continue
        reserve(job, budget, jobs, skipped)
    return jobs, skipped, unpicked


def plan_text(assets: list[Asset], style: dict, ledger: Ledger, budget: Budget, named: set[str]) -> tuple[list[Job], list[str]]:
    """Text-to-model jobs: the "route": "text" assets, and assets --only names
    (nodes, buildings, decor) that have no pick."""
    jobs, skipped = [], []
    for a in assets:
        if a.route != "text" and not (a.id in named or a.key in named):
            continue
        if a.kind not in TEXT_KINDS:
            print(f"{a.key}: {a.kind} are made from a concept (concepts, pick, models)")
            continue
        if effective_pick(a, ledger) is not None:
            print(f"{a.key}: has a picked concept: `models` makes it")
            continue
        if studio_made(a, ledger):
            print(f"{a.key}: made in Tripo Studio ({rel(a.target())})")
            continue
        job = text_job(a, style, ledger)
        if job.done is not None:
            print(f"{a.key}: already made from text")
            continue
        if placed_elsewhere(job, None):
            print(f"{a.key}: already there ({rel(a.target())}, made elsewhere); delete it to make it from text")
            continue
        if job.error:
            jobs.append(job)
            continue
        reserve(job, budget, jobs, skipped)
    return jobs, skipped


def placed_elsewhere(job: Job, pick: dict | None) -> bool:
    """A model this tool has paid nothing for is already installed (made by
    hand, by Meshy or in Studio): a Meshy pick or a text filler is then
    never made again. (A Tripo pick is: picking again means a new model.)"""
    return (job.task is None and job.asset.target().exists()
            and (pick is None or pick["source"] == "meshy"))


# --------------------------------------------------------------------------
# Commands
# --------------------------------------------------------------------------

def balance(http: Http) -> float:
    b = http("GET", BALANCE) or {}
    return float(b.get("balance", 0) or 0)


def cmd_balance(http: Http) -> None:
    b = http("GET", BALANCE) or {}
    print(f"balance {num(b.get('balance', 0))} credits ({num(b.get('frozen', 0))} frozen in tasks running)")


def cmd_check(http: Http, image: Path) -> None:
    """Free calls only: the balance, and one upload (POST /files)."""
    cmd_balance(http)
    data = http("POST", FILES, None, upload=image) or {}
    token = data.get("file_token") or ""
    if not token:
        raise TripoError("the upload answered without a file_token")
    print(f"upload works: {rel(image)} -> a file token ({token[:5]}..., {len(token)} characters); nothing was charged")


def cmd_usage(http: Http, ledger: Ledger) -> None:
    rows = http("GET", USAGE + "?limit=200&offset=0") or []
    known = {t["task_id"] for t in ledger.tasks}
    strays = 0
    for r in rows:
        mine = r.get("task_id") in known
        strays += not mine
        print(f"{r.get('created_at', ''):<26} {r.get('type', ''):<16} {str(r.get('status', '')):<10} "
              f"{num(r.get('credits_consumed') or 0):>6}  {r.get('task_id')}  {'' if mine else 'NOT IN THE LEDGER'}")
    print(f"{len(rows)} recent tasks; {strays} not in the ledger")


def cmd_status(ledger: Ledger) -> None:
    for t in ledger.tasks:
        stage = f"{t['stage']}#{t.get('candidate')}" if t["stage"] == "concept" else t["stage"]
        note = t.get("installed") or ("picked" if t.get("picked") else "")
        print(f"{t['asset']:<26} {stage:<11} {t['status']:<10} {num(t.get('credits', 0)):>5}  {t['task_id']}  {note}")
    print(f"{len(ledger.tasks)} tasks, {ledger.spent()} credits")


def cmd_pick(ledger: Ledger, asset: Asset, choice: str) -> dict:
    """Record the concept `models` makes: a candidate number (its latest
    finished concept with that number) or a concept task id."""
    if choice.isdigit():
        found = [t for t in ledger.tasks if t["asset"] == asset.key and t["stage"] == "concept"
                 and t.get("candidate") == int(choice) and t["status"] == SUCCEEDED]
        if not found:
            raise TripoError(f"{asset.key}: no finished concept candidate {choice} in the ledger (tripo.py status lists them)")
        chosen = found[-1]
    else:
        chosen = next((t for t in ledger.tasks if t["task_id"] == choice), None)
        if chosen is None:
            raise TripoError(f"{choice}: no such task in the ledger")
        if chosen["stage"] != "concept" or chosen["asset"] != asset.key:
            raise TripoError(f"{choice}: a {chosen['stage']} of {chosen['asset']}, not a concept of {asset.key}")
        if chosen["status"] != SUCCEEDED:
            raise TripoError(f"{choice}: a concept that is {chosen['status']}, not finished")
    before = ledger.picked(asset.key)
    ledger.pick(chosen["task_id"])
    was = f" (was {before['task_id']})" if before is not None and before is not chosen else ""
    image = concept_image(asset.key, chosen.get("candidate"), chosen["task_id"])
    print(f"{asset.key}: picked concept {chosen['task_id']} ({rel(image)}){was}; `models` makes it")
    return chosen


def cmd_resolve(http: Http | None, ledger: Ledger, key: str, choice: str) -> dict:
    """Settle an uncertain request: the task it made (found with `usage`), or
    "none" when it made none (the next run then makes it)."""
    open_ = [t for t in ledger.tasks if t["asset"] == key and t["status"] == UNCERTAIN]
    if not open_:
        raise TripoError(f"{key}: no uncertain task in the ledger")
    entry = open_[-1]
    if choice == "none":
        ledger.update(entry["task_id"], status=NOT_CREATED, credits=0)
        print(f"{key}: the {entry['stage']} request made no task; the next run makes it")
        return entry
    if any(t["task_id"] == choice for t in ledger.tasks):
        raise TripoError(f"{choice}: that task is already in the ledger")
    if http is None:
        raise TripoError("resolve needs the API key to look the task up")
    data = http("POST", TASKS_LIST, {"task_ids": [choice]}) or {}
    r = (data.get("tasks") or {}).get(choice)
    if r is None:
        raise TripoError(f"{choice}: Tripo does not know this task")
    with ledger.lock:
        entry["task_id"] = choice
        entry["status"] = status_of(r) if status_of(r) != SUCCEEDED else RUNNING  # the next run fetches its files
        entry["error"] = None
        ledger.save()
    print(f"{key}: recorded task {choice} ({status_of(r)}); the next run of the same command finishes it without paying")
    return entry


def show_jobs(jobs: list[Job]) -> None:
    for j in jobs:
        if j.task is None and not j.finished:
            print(f"{j.label}: POST {j.req.path} ({j.req.cost} credits)")
            print("  " + json.dumps(j.req.shown(), indent=2).replace("\n", "\n  "))


def problems(jobs: list[Job], skipped: list[str], budget: Budget, warnings: list[str]) -> None:
    for w in warnings:
        print(f"WARNING {w}")
    found = [j.error for j in jobs if j.error] + ([f"not started (over {budget.why}): {', '.join(skipped)}"] if skipped else [])
    if found:
        raise TripoError("\n  ".join([""] + found))


def execute(http: Http, ledger: Ledger, jobs: list[Job], skipped: list[str], budget: Budget, concurrency: int,
            what: str, sleep: Callable[[float], None] = time.sleep, clock: Callable[[], float] = time.time,
            workers: int = IO_WORKERS) -> list[Job]:
    s = run_jobs(http, ledger, jobs, concurrency, sleep, clock, workers) if any(not j.finished for j in jobs) else None
    made = [j for j in jobs if j.done]
    print(f"{len(made)} {what}; {budget.used} credits reserved this run (ledger total {ledger.spent()})")
    problems(jobs, skipped, budget, s.warnings if s else [])
    return made


def cmd_concepts(http: Http, assets: list[Asset], style: dict, ledger: Ledger, budget: Budget, candidates: int | None = None,
                 named: set[str] | None = None, concurrency: int = 100, dry_run: bool = False, **run) -> list[Job]:
    candidates = concept_style(style).get("candidates", 2) if candidates is None else candidates
    if candidates < 1:
        raise TripoError("--candidates must be at least 1")
    jobs, skipped = plan_concepts(assets, style, ledger, budget, candidates, named or set())
    if dry_run:
        return dry(jobs, skipped, budget)
    made = execute(http, ledger, jobs, skipped, budget, concurrency, "concept images", **run)
    print("pick one per asset with: tripo.py pick <key> <n>")
    return made


def cmd_models(http: Http, assets: list[Asset], style: dict, ledger: Ledger, budget: Budget, meshy_out: Path | None = None,
               concurrency: int = 100, dry_run: bool = False, **run) -> list[Job]:
    model_style(style)
    jobs, skipped, unpicked = plan_models(assets, style, ledger, budget, meshy_out, (run.get("clock") or time.time)())
    if unpicked:
        print(f"no concept picked (skipped; tripo.py pick <key> <n>): {', '.join(unpicked)}")
    if dry_run:
        return dry(jobs, skipped, budget)
    return execute(http, ledger, jobs, skipped, budget, concurrency, "models made", **run)


def cmd_make(http: Http, assets: list[Asset], style: dict, ledger: Ledger, budget: Budget, named: set[str] | None = None,
             concurrency: int = 100, dry_run: bool = False, **run) -> list[Job]:
    text_style(style)
    jobs, skipped = plan_text(assets, style, ledger, budget, named or set())
    if dry_run:
        return dry(jobs, skipped, budget)
    return execute(http, ledger, jobs, skipped, budget, concurrency, "models made from text", **run)


def dry(jobs: list[Job], skipped: list[str], budget: Budget) -> list[Job]:
    show_jobs(jobs)
    for j in jobs:
        if j.error:
            print(f"! {j.error}")
    paid = [j for j in jobs if j.task is None and not j.finished]
    print(f"dry run: {len(paid)} tasks would start for {budget.used} credits; "
          f"{len([j for j in jobs if j.task is not None and not j.finished])} paid earlier would be resumed")
    if skipped:
        print(f"not started (over {budget.why}): {', '.join(skipped)}")
    return []


def cmd_plan(assets: list[Asset], style: dict, ledger: Ledger, http: Http | None, meshy_out: Path | None = None,
             bodies: bool = False, now: float | None = None) -> int:
    """Per asset, what is still to pay and its credits; totals per stage;
    then the balance (when a key is set). With `bodies`, every request as
    it would be posted."""
    c, m, t = concept_style(style), model_style(style), text_style(style)
    candidates = c.get("candidates", 2)
    image_model, text_model = model_cost(IMAGE_TO_MODEL, m), model_cost(TEXT_TO_MODEL, t)
    print(f"concepts: {c['model']}, {candidates} candidates, {IMAGE_PRICE} credits each (unverified: the docs' example charge)")
    print(f"models: image to model {m['model']}, texture {m.get('texture_quality')}, pbr {m.get('pbr')}: {image_model} credits; "
          f"text to model {t.get('texture_quality')}: {text_model} credits (the docs' v3.1 prices)")
    per_stage: dict[str, int] = {}
    unpicked, missing_images = [], []
    for a in assets:
        steps: list[tuple[str, int, str]] = []
        shown: list[Job] = []
        note = ""
        pick = effective_pick(a, ledger)
        if a.rig:
            print(f"  {a.key:<26}       a rigged humanoid: meshy.py makes it")
            continue
        if studio_made(a, ledger):
            print(f"  {a.key:<26}       done ({rel(a.target())}, Tripo Studio)")
            continue
        if pick is not None:
            job = model_job(a, pick, style, ledger, meshy_out, now)
            if job.done is not None:
                print(f"  {a.key:<26}       done ({rel(a.target())}, from concept {pick['task_id']})")
                continue
            if placed_elsewhere(job, pick):
                print(f"  {a.key:<26}       done ({rel(a.target())}, made elsewhere)")
                continue
            steps.append(("model", job.cost, f"model {job.cost}" if job.task is None else f"model paid ({job.task['status']})"))
            shown.append(job)
            if pick["source"] == "meshy":
                note = " (from its Meshy concept)"
                if job.task is None and not meshy_image(a, meshy_out).is_file():
                    missing_images.append(a.key)
        elif a.route == "text":
            job = text_job(a, style, ledger)
            if job.done is not None or placed_elsewhere(job, None):
                print(f"  {a.key:<26}       done ({rel(a.target())}, {'from text' if job.done else 'made elsewhere'})")
                continue
            steps.append(("text_model", job.cost, f"text model {job.cost}" if job.task is None else f"text model paid ({job.task['status']})"))
            shown.append(job)
        elif a.target().exists():
            print(f"  {a.key:<26}       done ({rel(a.target())})")
            continue
        else:
            cjobs = concept_jobs(a, style, ledger, candidates)
            missing = [j for j in cjobs if j.task is None]
            each = cjobs[0].req.cost
            steps.append(("concept", len(missing) * each, f"concepts {len(missing)}x{each}" if missing else f"concepts paid ({len(cjobs)})"))
            steps.append(("pick", 0, "pick"))
            steps.append(("model", image_model, f"model {image_model}"))
            shown += missing
            unpicked.append(a.key)
        cost = sum(credits for _, credits, _ in steps)
        for stage, credits, _ in steps:
            if stage != "pick":
                per_stage[stage] = per_stage.get(stage, 0) + credits
        print(f"  {a.key:<26} {cost:>4} credits  {a.kind:<9} {a.batch or '-':<12} {', '.join(w for _, _, w in steps)}{note}")
        if bodies:
            for j in shown:
                if j.task is None:
                    print(f"      POST {j.req.path}" + (f" (candidate {j.candidate})" if j.stage == "concept" else ""))
                    print("      " + json.dumps(j.req.shown(), indent=2).replace("\n", "\n      "))
    total = sum(per_stage.values())
    print("per stage: " + (", ".join(f"{stage} {credits}" for stage, credits in per_stage.items()) or "nothing to pay"))
    if unpicked:
        print(f"{len(unpicked)} assets have no pick yet (concepts, then tripo.py pick <key> <n>): {', '.join(unpicked)}")
    if missing_images:
        print(f"Meshy concept images missing (run in the main checkout or pass --meshy-out): {', '.join(missing_images)}")
    print(f"total {total} credits to finish {len(assets)} assets (ledger so far: {ledger.spent()})")
    if http is not None:
        cmd_balance(http)
    return total


# --------------------------------------------------------------------------
# Tripo Studio by hand: kit and import
# --------------------------------------------------------------------------

STUDIO_QUALITY = {"standard": "Standard", "detailed": "HD (detailed)", "extreme": "8K (extreme)", "fast": "Fast"}
# Tripo Studio membership credits (not API credits), as the owner's Studio
# showed them: image to 3D with H3.1 costs 55 (65 with 8K textures); its
# image generator (Nano Banana Pro, 4 images a prompt) and exports are free.
STUDIO_IMAGE_TO_3D = 55


def studio_image(asset: Asset, ledger: Ledger, meshy_out: Path | None) -> Path | None:
    pick = effective_pick(asset, ledger)
    if pick is None:
        return None
    if pick["source"] == "meshy":
        return meshy_image(asset, meshy_out)
    return concept_image(asset.key, pick["candidate"], pick["task_id"])


def studio_route(asset: Asset, image: Path | None) -> str:
    """concept: image to 3D from its picked concept; images: Studio's free
    image generator first, then image to 3D; text: text to 3D."""
    return "concept" if image is not None else "text" if asset.route == "text" else "images"


def studio_settings(asset: Asset, style: dict, route: str, batch: str) -> str:
    m = model_style(style)
    kind = {"units": "unit", "props": "prop", "buildings": "building", "nodes": "resource node", "decor": "decor (scenery)"}[asset.kind]
    refs = (style.get("references") or {}).get(asset.kind) or []
    if route == "concept":
        steps = ["1. Image to 3D: upload concept.png from this folder (its picked concept; no prompt needed)."]
    elif route == "images":
        steps = ["1. Concept first, as the API route does: in Studio's image generator (Nano Banana Pro; free with the",
                 "   membership, 4 images a prompt) paste prompt.txt. Pick the image that is one whole object, closest",
                 "   to the style of the old models, and save it here as concept.png (for the record). Then Image to 3D",
                 "   from it."]
        if refs:
            steps += ["   If the generator takes reference images, attach the ones in ../_references/" + asset.kind + "/ and start",
                      f"   the prompt with: {STYLE_ONLY}"]
        steps += ["   (Text to 3D from text_prompt.txt also works, with less control over the look.)"]
    else:
        steps = ["1. Text to 3D: paste prompt.txt (a cheap filler).",
                 "   (Or the image route: Studio's free image generator from image_prompt.txt, then Image to 3D.)"]
    lines = [
        f"{asset.key}: {kind}, {asset.prompt[:100]}{'...' if len(asset.prompt) > 100 else ''}",
        f"Installs to: {rel(asset.target())}",
        "",
        "In Tripo Studio (studio.tripo3d.ai):",
        *steps,
        f"2. Model: {m['model']} (H3.1, the newest H model; not a P-series / low-poly model).",
        f"3. Texture: on, quality {STUDIO_QUALITY.get(m.get('texture_quality', 'standard'))}, not 8K. PBR: "
        f"{'on' if m.get('pbr') else 'off (the game keeps only the base colour)'}.",
        f"4. Face limit / polycount: {face_limit(asset, style)} (triangles; no quad, no smart low poly, no parts).",
        f"5. Geometry quality: {'Ultra (detailed)' if m.get('geometry_quality') == 'detailed' else 'standard (not Ultra)'}. Auto size: off.",
        f"6. Generate (image to 3D: {STUDIO_IMAGE_TO_3D} credits). Check it is one whole object like the picture, with no base plate.",
        f"7. Export (free): format GLB, texture resolution 2K, file name {asset.key} (any name works). One export runs at a time.",
        "8. Drop the .glb file into this folder (exactly one .glb).",
        f"9. When done: python tools/tripo/tripo.py import-studio --batch {batch}",
    ]
    return "\n".join(lines) + "\n"


def kit_folder(batch: str) -> Path:
    if not re.fullmatch(r"[A-Za-z0-9_.-]+", batch):
        raise TripoError(f"batch {batch!r}: a kit folder name needs letters, digits, - _ or .")
    return STUDIO / batch


def cmd_kit(batch: str, assets: list[Asset], style: dict, ledger: Ledger, meshy_out: Path | None = None,
            only: set[str] | None = None) -> Path:
    """Write art/tripo/studio/<batch>/<NN>_<key>/ for every model of the batch
    still to make, with settings.txt (the Studio choices) and what to start
    from: its picked concept as concept.png; or prompt.txt for Studio's
    image generator (text_prompt.txt for text to 3D instead); or, for a
    "route": "text" filler, prompt.txt for text to 3D (image_prompt.txt for
    the image route). The style references go to _references/<kind>/. Plus
    CHECKLIST.md. Numbers follow the batch's full order, so they stay put as
    models arrive; files dropped into a folder are never touched."""
    if not assets:
        raise TripoError(f"batch {batch}: no assets")
    model_style(style)
    root = kit_folder(batch)
    lines, todo, credits, kinds = [], 0, 0, set()
    for n, a in enumerate(assets, 1):
        if only and not (a.id in only or a.key in only):
            continue
        name = f"{n:02d}_{a.key}"
        if a.rig:
            lines.append(f"- [ ] `{name}`: skipped, a rigged humanoid (meshy.py makes it)")
            continue
        if a.target().exists():
            lines.append(f"- [x] `{name}`: done ({rel(a.target())})")
            continue
        image = studio_image(a, ledger, meshy_out)
        if image is not None:
            check_image(image, a.key, meshy=a.meshy_concept is not None and ledger.picked(a.key) is None)
        route = studio_route(a, image)
        folder = root / name
        folder.mkdir(parents=True, exist_ok=True)
        if image is not None:
            shutil.copyfile(image, folder / "concept.png")
        texts = {
            "concept": {},
            "images": {"prompt.txt": concept_prompt(a, style, False), "text_prompt.txt": text_prompt(a, style, trim=True)},
            "text": {"prompt.txt": text_prompt(a, style, trim=True), "image_prompt.txt": concept_prompt(a, style, False)},
        }[route]
        for file in ("prompt.txt", "text_prompt.txt", "image_prompt.txt"):
            if file in texts:
                (folder / file).write_text(texts[file] + "\n", encoding="utf-8")
            elif (folder / file).exists():
                (folder / file).unlink()
        (folder / "settings.txt").write_text(studio_settings(a, style, route, batch), encoding="utf-8")
        if route == "images":
            kinds.add(a.kind)
        how = {"concept": "image to 3D from concept.png",
               "images": "image generator from prompt.txt (free), pick one, image to 3D",
               "text": "text to 3D from prompt.txt"}[route]
        credits += STUDIO_IMAGE_TO_3D if route != "text" else 0
        lines.append(f"- [ ] `{name}`: {how}, face limit {face_limit(a, style)}: {a.prompt[:80]}{'...' if len(a.prompt) > 80 else ''}")
        todo += 1
    for kind in sorted(kinds):
        for ref in reference_files(kind, style, concept_style(style)["model"]):
            (root / "_references" / kind).mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ref, root / "_references" / kind / ref.name)
    m = model_style(style)
    texts = sum(1 for line in lines if "text to 3D from prompt.txt" in line)
    head = [
        f"# Tripo Studio kit: {batch}",
        "",
        f"{todo} models to make by hand in Tripo Studio (studio.tripo3d.ai), one folder each. Image to 3D with "
        f"H3.1 costs {STUDIO_IMAGE_TO_3D} Studio credits (65 with 8K textures, not needed): about {credits} credits here"
        + (f", plus {texts} text-to-3D models at Studio's price" if texts else "") + ". Studio's image generator "
        "(Nano Banana Pro, 4 images a prompt; 1000 free images a month) and exports are free.",
        "",
        "For each folder, in order:",
        "",
        "1. Read its `settings.txt`: what to start from and the Studio choices (model "
        f"{m['model']}, texture {STUDIO_QUALITY.get(m.get('texture_quality', 'standard'))}, PBR off, its face limit).",
        "2. Generate in Studio: image to 3D from `concept.png`; or first concept images from `prompt.txt` in the image "
        "generator, then image to 3D from the best one; or text to 3D from `prompt.txt` where settings.txt says so.",
        "3. Export as GLB with 2K textures (one export runs at a time, about 15-20 s) and drop the file into that "
        "folder (any file name, exactly one .glb per folder).",
        "",
        "Several generations can run while you export the last one. Then install them all (it says what is still "
        "missing, and can be run again any time):",
        "",
        f"    python tools/tripo/tripo.py import-studio --batch {batch}",
        "",
    ]
    (root / "CHECKLIST.md").write_text("\n".join(head + lines) + "\n", encoding="utf-8")
    print(f"{rel(root)}: {todo} folders to fill; see {rel(root / 'CHECKLIST.md')}")
    return root


def cmd_import(batch: str, assets: list[Asset], ledger: Ledger, only: set[str] | None = None,
               clock: Callable[[], float] = time.time) -> list[Asset]:
    """Install the GLB dropped into each kit folder of the batch, record it in
    the ledger (source studio, 0 credits) and report what is still missing.
    A folder with more than one GLB, or a file that is not a GLB, is refused."""
    root = kit_folder(batch)
    if not root.is_dir():
        raise TripoError(f"no Studio kit at {rel(root)}: run `tripo.py kit --batch {batch}` first")
    by_key = {a.key: a for a in assets}
    imported, same, refused, strays, seen = [], [], [], [], set()
    for d in sorted(p for p in root.iterdir() if p.is_dir() and not p.name.startswith("_")):
        m = re.fullmatch(r"\d+_(.+)", d.name)
        a = by_key.get(m.group(1) if m else d.name)
        if a is None:
            strays.append(f"{rel(d)}: names no asset of batch {batch}")
            continue
        if only and not (a.id in only or a.key in only):
            continue
        seen.add(a.key)
        glbs = sorted(f for f in d.iterdir() if f.is_file() and f.suffix.lower() == ".glb")
        if not glbs:
            continue
        if len(glbs) > 1:
            refused.append(f"{rel(d)}: {len(glbs)} GLB files ({', '.join(f.name for f in glbs)}); leave only the one to install")
            continue
        blob = glbs[0].read_bytes()
        if blob[:4] != b"glTF":
            refused.append(f"{rel(glbs[0])}: not a GLB file (no glTF header); export the model as GLB")
            continue
        sha = hashlib.sha256(blob).hexdigest()
        target = a.target()
        prev = ledger.studio(a.key)
        if (prev is not None and prev.get("sha256") == sha and prev.get("installed") == rel(target)
                and target.exists() and hashlib.sha256(target.read_bytes()).hexdigest() == sha):
            same.append(a.key)
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        tmp = target.with_suffix(".glb.part")
        tmp.write_bytes(blob)
        tmp.replace(target)
        if prev is not None and prev.get("sha256") == sha:
            ledger.update(prev["task_id"], installed=rel(target))
        else:
            ledger.add(asset=a.key, stage="studio", task_id=f"studio-{sha[:16]}", source="studio", file=glbs[0].name,
                       sha256=sha, hash=sha[:16], status=SUCCEEDED, credits=0, created=int(clock()), installed=rel(target))
        imported.append(a)
        print(f"{a.key}: installed {rel(target)} from {glbs[0].name} (Tripo Studio)")
    missing = [a.key for a in assets if (not only or a.id in only or a.key in only)
               and not a.rig and not a.target().exists()]
    print(f"{len(imported)} imported, {len(same)} already imported; {len(missing)} still to make"
          + (f": {', '.join(missing)}" if missing else ""))
    if refused or strays:
        raise TripoError("\n  ".join([""] + refused + strays))
    return imported


# --------------------------------------------------------------------------
# Command line
# --------------------------------------------------------------------------

def preflight(cmd: str, assets: list[Asset], style: dict) -> None:
    """What a paid command needs from style.json and the references, checked
    before the key is read: a bad config costs nothing."""
    if cmd == "concepts":
        for a in assets:
            concept_request(a, style, 0)
    elif cmd == "models":
        model_style(style)
        for a in assets:
            face_limit(a, style)
    elif cmd == "make":
        text_style(style)


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    for name in ("balance", "check", "status", "usage"):
        sub.add_parser(name)
    for name in ("plan", "concepts", "models", "make", "kit", "import-studio"):
        s = sub.add_parser(name)
        s.add_argument("--only", help="comma-separated asset ids or keys (id_variant)")
        s.add_argument("--batch", required=name in ("kit", "import-studio"),
                       help=f"only this batch (requests.json \"batch\", or {MESHY_BATCH})")
        if name in ("plan", "models", "kit"):
            s.add_argument("--meshy-out", type=Path, help="where the Meshy concept images are (default art/meshy/out)")
        if name == "plan":
            s.add_argument("--bodies", action="store_true", help="print every request as it would be posted")
        if name in ("concepts", "models", "make"):
            s.add_argument("--max-credits", type=int, required=True)
            s.add_argument("--concurrency", type=int, help="tasks in flight at once (default style.json concurrency)")
            s.add_argument("--dry-run", action="store_true", help="print what would be asked and its cost; no API call")
        if name == "concepts":
            s.add_argument("--candidates", type=int, help="concept images per asset (default style.json concept.candidates)")
    s = sub.add_parser("pick")
    s.add_argument("asset", help="key (id_variant) or id")
    s.add_argument("concept", help="a concept candidate number or concept task id")
    s = sub.add_parser("resolve")
    s.add_argument("asset", help="key (id_variant)")
    s.add_argument("task", help="the task id the uncertain request made (see `usage`), or none")
    args = p.parse_args(argv)
    try:
        ledger = Ledger(LEDGER)
        if args.cmd == "status":
            cmd_status(ledger)
            return 0
        if args.cmd in ("balance", "check", "usage"):
            http = make_http(api_key())
            if args.cmd == "balance":
                cmd_balance(http)
            elif args.cmd == "check":
                cmd_check(http, ROOT / "art" / "meshy" / "references" / "pine.png")
            else:
                cmd_usage(http, ledger)
            return 0
        if args.cmd == "resolve":
            key = api_key(required=False)
            cmd_resolve(make_http(key) if key and args.task != "none" else None, ledger, args.asset, args.task)
            return 0
        # Settle what to make before the key is read: a bad name costs nothing.
        style = load_style()
        if args.cmd == "pick":
            cmd_pick(ledger, find_asset(load_assets(style=style), args.asset), args.concept)
            return 0
        only = set(args.only.split(",")) if args.only else None
        if args.cmd in ("kit", "import-studio"):
            batch_assets = load_assets(None, args.batch, style)
            if only:
                load_assets(only, args.batch, style)  # every name in --only is in the batch
            if args.cmd == "kit":
                cmd_kit(args.batch, batch_assets, style, ledger, args.meshy_out, only)
            else:
                cmd_import(args.batch, batch_assets, ledger, only)
            return 0
        assets = load_assets(only, args.batch, style)
        if not assets:
            raise TripoError("no matching assets")
        if args.cmd == "plan":
            key = api_key(required=False)
            cmd_plan(assets, style, ledger, make_http(key) if key else None, args.meshy_out, args.bodies)
            if not key:
                print("(no TRIPO_API_KEY: balance not shown)")
            return 0
        if args.cmd == "concepts" and args.candidates is not None and args.candidates < 1:
            raise TripoError("--candidates must be at least 1")
        preflight(args.cmd, assets, style)
        concurrency = args.concurrency or style.get("concurrency", 100)
        if args.dry_run:
            http, budget = None, Budget(args.max_credits)
        else:
            http = make_http(api_key())
            available = balance(http)
            cap = min(args.max_credits, int(available))
            why = f"--max-credits {args.max_credits}" + (f" and the balance of {num(available)}" if available < args.max_credits else "")
            budget = Budget(cap, why)
            print(f"balance {num(available)} credits; spending at most {cap}")
        common = dict(concurrency=concurrency, dry_run=args.dry_run)
        if args.cmd == "concepts":
            cmd_concepts(http, assets, style, ledger, budget, args.candidates, only or set(), **common)
        elif args.cmd == "models":
            cmd_models(http, assets, style, ledger, budget, getattr(args, "meshy_out", None), **common)
        elif args.cmd == "make":
            cmd_make(http, assets, style, ledger, budget, only or set(), **common)
        return 0
    except TripoError as e:
        print(f"tripo: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
