"""Tests for tripo.py against a fake Tripo (no network, no credits).

    python tools/tripo/test_tripo.py        (or: python -m unittest discover tools/tripo)

The fake checks every request body against the parameters the Tripo v3
docs list for its endpoint; the HTTP layer is tested against a fake urlopen.
"""

import contextlib
import dataclasses
import io
import json
import os
import shutil
import socket
import sys
import tempfile
import threading
import time
import unittest
import urllib.error
from email.message import Message
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import tripo  # noqa: E402

REAL_ROOT = tripo.ROOT
ELEVEN = {"dune_sandskiff", "dune_sand_engine", "ironhold_forgefather", "starborn_orbital_lance", "starborn_archon",
          "steppe_skyrider", "steppe_jet_lancer", "steppe_war_ordu", "sylvan_starstag_rider", "sylvan_wisp_swarm",
          "tide_bucentaur"}

STYLE = {
    "concurrency": 100,
    "concept": {"model": "banana_pro", "candidates": 2, "size": "2K",
                "aspect": {"character": "3:4", "object": "1:1", "prop": "1:1", "scenery": "4:3"}},
    "model": {"model": "v3.1-20260211", "texture": True, "pbr": False, "texture_quality": "detailed",
              "texture_version": "v3.5-20260815", "delight": True, "geometry_quality": "standard", "orientation": "default",
              "texture_alignment": "original_image", "enable_image_autofix": False, "auto_size": False},
    "text_model": {"model": "v3.1-20260211", "texture": True, "pbr": False, "texture_quality": "standard",
                   "negative_prompt": "base plate, text", "look": "Painted miniature."},
    "face_limit": {"units": 30000, "buildings": 30000, "nodes": 8000, "props": 3000, "decor": 10000},
    "references": {},
    "framing": {"character": "One character in an A-pose", "object": "One object", "prop": "One prop", "scenery": "One piece of scenery"},
    "look": "Painterly game art, muted colours",
    "kind_look": {"decor": "Painterly game art, vibrant colours"},
    "ages": {str(i): f"Age {i} materials" for i in range(1, 6)},
    "factions": {"dune": "Desert scholars", "steppe": "Horse clans"},
    "magic": {"dune": "Sun-glass sorcery"},
    "kinds": {"units": "A game unit", "props": "A hand-held item", "buildings": "A building", "nodes": "A natural resource",
              "decor": "Landscape scenery"},
}
MODEL_BODY = {"model": "v3.1-20260211", "texture": True, "pbr": False, "texture_quality": "detailed", "texture_version": "v3.5-20260815",
              "delight": True, "geometry_quality": "standard", "orientation": "default", "texture_alignment": "original_image",
              "enable_image_autofix": False, "auto_size": False}

# What the Tripo v3 docs list per endpoint (generation-*.md), independent of tripo.py.
DOCUMENTED = {
    "text-to-image": {"model", "prompt", "size", "quality", "background", "aspect_ratio", "output_format", "watermark", "template"},
    "image-to-image": {"model", "input", "inputs", "prompt", "size", "quality", "background", "aspect_ratio", "template", "output_format"},
    "image-to-model": {"input", "model", "enable_image_autofix", "texture_alignment", "orientation", "face_limit", "texture", "pbr",
                       "texture_seed", "texture_quality", "texture_version", "delight", "geometry_quality", "auto_size", "quad",
                       "smart_low_poly", "generate_parts", "compress", "export_orientation", "model_seed", "export_uv"},
    "text-to-model": {"prompt", "model", "negative_prompt", "model_seed", "image_seed", "face_limit", "texture", "pbr", "texture_seed",
                      "texture_quality", "texture_version", "delight", "geometry_quality", "auto_size", "quad", "smart_low_poly",
                      "generate_parts", "compress", "export_orientation", "export_uv"},
}
IMAGE_MODELS = {"seedream_v4", "seedream_v5", "banana", "banana_pro", "banana2", "chat_image_1", "chat_image_1.5", "chat_image_2",
                "chat_image_2.5_flare", "chat_image_2.5_sunburst"}
BANANA_ASPECTS = {"1:1", "2:3", "3:2", "3:4", "4:3", "4:5", "5:4", "9:16", "16:9", "21:9"}
TYPES = {"text-to-image": "text_to_image", "image-to-image": "image_to_image", "image-to-model": "image_to_model", "text-to-model": "text_to_model"}
CATEGORY = {"text-to-image": "image generation", "image-to-image": "image generation", "image-to-model": "3D generation", "text-to-model": "3D generation"}
PRICE = {"text-to-image": 20, "image-to-image": 20, "image-to-model": 40, "text-to-model": 20}
SIGNED = "?Expires=1&Signature=s"


class Crash(Exception):
    """The process dies mid-run (Ctrl+C, a power cut)."""


class FakeTripo:
    """A Tripo v3 with a concurrency pool per category (creating a task in a
    full one is refused with code 2000), a balance (code 2010), tasks that
    finish after `polls` batch queries and fresh links every time a finished
    task is queried again. Tasks of the `fail` endpoints fail (refunded).
    `busy`: categories each holding one task from elsewhere for a while.
    `crash` = (method, path prefix): the next such call dies, once."""

    made = 0

    def __init__(self, limits=None, polls=2, busy=(), fail=(), balance=10**6, charge=None, uncertain=0):
        FakeTripo.made += 1
        self.prefix = f"t{FakeTripo.made}-"
        self.limits = {"image generation": 10**6, "3D generation": 10**6, **(limits or {})}
        self.polls, self.fail, self.balance = polls, set(fail), balance
        self.charge = {**PRICE, **(charge or {})}
        self.calls, self.tasks, self.n = [], {}, 0
        self.created, self.bodies, self.uploaded, self.queries = [], [], [], []
        self.tokens: dict[str, Path] = {}
        self.peak: dict[str, int] = {}
        self.peak_total = 0
        self.crash = None
        self.uncertain = uncertain  # the next N creates make a task but the answer is lost
        self.lock = threading.Lock()
        for i, category in enumerate(busy):
            self.tasks[f"other{i}"] = {"api": "text-to-model", "category": category, "left": 6, "other": True, "version": 0}

    def in_flight(self, category=None):
        return sum(1 for t in self.tasks.values() if t["left"] > 0 and (category is None or t["category"] == category))

    def seed(self, task_id, api="text-to-image"):
        """A task an earlier run (another fake) made and finished."""
        self.tasks[task_id] = {"api": api, "category": CATEGORY[api], "left": 0, "version": 0}

    def __call__(self, method, path, body=None, upload=None):
        with self.lock:
            if self.crash and method == self.crash[0] and path.startswith(self.crash[1]):
                self.crash = None
                raise Crash(f"{method} {path}")
            self.calls.append((method, path, upload.name if upload is not None else body))
            for t in self.tasks.values():  # tasks from elsewhere end on their own
                if t.get("other"):
                    t["left"] = max(0, t["left"] - 1)
            if method == "GET":
                if path == tripo.BALANCE:
                    return {"balance": float(self.balance), "frozen": 0.0}
                if path.startswith(tripo.USAGE):
                    return [{"task_id": tid, "type": TYPES[t["api"]], "status": "success", "credits_consumed": 40.0,
                             "created_at": "2026-10-22T10:00:00Z"} for tid, t in self.tasks.items() if not t.get("other")]
                raise AssertionError(f"GET {path}: tasks are polled through POST /tasks/list")
            assert method == "POST", method
            if path == tripo.FILES:
                assert upload is not None and upload.is_file() and body is None, "an upload is one multipart file"
                token = f"file_{len(self.uploaded)}"
                self.uploaded.append(upload.name)
                self.tokens[token] = upload
                return {"file_token": token}
            if path == tripo.TASKS_LIST:
                return self.query(body)
            return self.create(path, body)

    def create(self, path, body):
        api = path.rsplit("/", 1)[-1]
        assert path == f"/generation/{api}" and api in DOCUMENTED, path
        self.check(api, body)
        category = CATEGORY[api]
        if self.in_flight(category) >= self.limits[category]:
            raise tripo.QueueFull(f"POST {path}: You have exceeded the limit of generation (code 2000)")
        if self.balance < self.charge[api]:
            raise tripo.NoCredits(f"POST {path}: not enough credits (code 2010)")
        self.balance -= self.charge[api]
        self.n += 1
        task_id = f"{self.prefix}{self.n}"
        self.tasks[task_id] = {"api": api, "category": category, "left": self.polls, "version": 0, "body": body}
        self.created.append(api)
        self.bodies.append((api, body))
        self.peak[category] = max(self.peak.get(category, 0), self.in_flight(category))
        self.peak_total = max(self.peak_total, self.in_flight() - sum(1 for t in self.tasks.values() if t.get("other") and t["left"]))
        if self.uncertain:
            self.uncertain -= 1
            raise tripo.Uncertain(f"POST {path}: HTTP 502 (code None): bad gateway")
        return {"task_id": task_id, "type": TYPES[api], "status": "queued"}

    def check_input(self, value, types):
        assert isinstance(value, str) and not value.startswith("data:"), f"{value[:30]}: base64 inputs are rejected; upload via /files"
        if value.startswith("file_"):
            assert value in self.tokens, f"{value}: not a token this Tripo gave"
            return
        if value.startswith(("http://", "https://")):
            return
        t = self.tasks.get(value)
        if t is not None:  # an unknown id is an earlier run's task
            assert TYPES[t["api"]] in types and t["left"] == 0, f"{value}: not a finished {' or '.join(types)} task"

    def check(self, api, body):
        """The request against the docs (generation-*.md)."""
        assert isinstance(body, dict), body
        unknown = set(body) - DOCUMENTED[api]
        assert not unknown, f"{api}: parameters the docs do not list: {sorted(unknown)}"
        if api in ("text-to-image", "image-to-image"):
            assert isinstance(body.get("prompt"), str) and 0 < len(body["prompt"]) <= 1800, "prompt: 1 to 1800 characters"
            assert body.get("model", "seedream_v4") in IMAGE_MODELS
            if "aspect_ratio" in body:
                assert body["model"] in ("banana", "banana_pro", "banana2"), "aspect_ratio: banana models only"
                assert body["aspect_ratio"] in BANANA_ASPECTS
            if "quality" in body:
                assert body["model"].startswith("chat_image_2"), "quality: chat_image_2 models only"
            assert body.get("output_format", "png") in ("png", "jpeg")
        if api == "text-to-image":
            assert "input" not in body and "inputs" not in body, "text to image takes no images"
        if api == "image-to-image":
            assert ("input" in body) != ("inputs" in body), "exactly one of input and inputs"
            inputs = body.get("inputs") or [body["input"]]
            assert 1 <= len(inputs) <= (10 if body.get("model", "seedream_v5").startswith("banana") else 4)
            for value in inputs:
                self.check_input(value, ("text_to_image", "image_to_image"))
        if api in ("image-to-model", "text-to-model"):
            assert body.get("model") in ("v3.1-20260211", "v3.0-20250812", "v2.5-20250123", "P1-20260311"), "model is required in v3"
            for flag in ("texture", "pbr", "delight", "auto_size", "enable_image_autofix", "export_uv", "quad", "smart_low_poly", "generate_parts"):
                assert flag not in body or isinstance(body[flag], bool), flag
            assert body.get("texture_quality", "standard") in ("standard", "detailed", "extreme", "fast")
            assert body.get("texture_version", "v3.0-20250812") in ("v3.5-20260815", "v3.0-20250812", "v2.5-20250123")
            assert body.get("texture_quality") != "fast" or body.get("texture_version") == "v3.5-20260815"
            assert body.get("geometry_quality", "standard") in ("standard", "detailed")
            assert "face_limit" not in body or (isinstance(body["face_limit"], int) and 0 < body["face_limit"] <= 1_500_000)
        if api == "image-to-model":
            assert body.get("orientation", "default") in ("default", "align_image")
            assert body.get("texture_alignment", "original_image") in ("original_image", "geometry")
            self.check_input(body["input"], ("text_to_image", "image_to_image"))
        if api == "text-to-model":
            assert isinstance(body.get("prompt"), str) and 0 < len(body["prompt"]) <= 1024, "prompt: up to 1024 characters"
            assert len(body.get("negative_prompt", "")) <= 255, "negative_prompt: up to 255 characters"

    def query(self, body):
        ids = body["task_ids"]
        assert isinstance(ids, list) and 1 <= len(ids) <= 100, "POST /tasks/list takes 1 to 100 ids"
        self.queries.append(len(ids))
        tasks, missed = {}, []
        for tid in ids:
            t = self.tasks.get(tid)
            if t is None:
                missed.append(tid)
                continue
            if t["left"] > 0:
                t["left"] -= 1
                if t["left"] > 0:
                    tasks[tid] = {"task_id": tid, "type": TYPES[t["api"]], "status": "running", "progress": 50}
                    continue
            else:
                t["version"] += 1  # queried again after it finished: fresh links
            tasks[tid] = self.result(tid, t)
        return {"tasks": tasks, "missed": missed}

    def result(self, tid, t):
        api, v = t["api"], t["version"]
        if api in self.fail:
            return {"task_id": tid, "type": TYPES[api], "status": "failed", "progress": 0, "error_code": 2008,
                    "error_message": "no luck", "credits_consumed": 0.0}
        done = {"task_id": tid, "type": TYPES[api], "status": "success", "progress": 100, "credits_consumed": float(self.charge[api])}
        if api in ("text-to-image", "image-to-image"):
            return {**done, "output": {"generated_image_url": f"fake://{tid}/v{v}/image.png{SIGNED}"}}
        return {**done, "output": {"model_url": f"fake://{tid}/v{v}/model.glb{SIGNED}",
                                   "rendered_image_url": f"fake://{tid}/v{v}/preview.webp{SIGNED}"}}


def fake_download(url, to):
    """A download whose file says which link it came from."""
    to.parent.mkdir(parents=True, exist_ok=True)
    to.write_bytes(url.split("?", 1)[0].encode())


class TripoTest(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)
        self.patch("ROOT", self.dir)
        self.patch("ART", self.dir / "art" / "tripo")
        self.patch("LEDGER", self.dir / "art" / "tripo" / "ledger.json")
        self.patch("OUT", self.dir / "art" / "tripo" / "out")
        self.patch("STUDIO", self.dir / "art" / "tripo" / "studio")
        self.patch("MESHY", self.dir / "art" / "meshy")
        self.patch("MESHY_OUT", self.dir / "art" / "meshy" / "out")
        self.patch("SOURCE", self.dir / "art" / "static" / "source")
        self.patch("UNITS", self.dir / "art" / "units" / "source")
        self.patch("download", fake_download)
        self.meshy({})
        self.ledger = tripo.Ledger(self.dir / "art" / "tripo" / "ledger.json")
        self.sleeps = 0
        self.out = ""
        FakeTripo.made = 0

    def patch(self, name, value):
        self.addCleanup(setattr, tripo, name, getattr(tripo, name))
        setattr(tripo, name, value)

    def nosleep(self, seconds):
        """No waiting, but a run that keeps polling fails instead of hanging."""
        self.sleeps += 1
        if self.sleeps > 2000:
            raise AssertionError("still polling after 2000 rounds: it would hang")

    def quietly(self, run, *args, **kwargs):
        out = io.StringIO()
        try:
            with contextlib.redirect_stdout(out):
                return run(*args, **kwargs)
        finally:
            self.out += out.getvalue()

    def restart(self):
        """A new process: it only knows the ledger saved on disk."""
        self.ledger = tripo.Ledger(self.ledger.path)

    def stage(self, name, key=None):
        return [t for t in self.ledger.tasks if t["stage"] == name and (key is None or t["asset"] == key)]

    def rel(self, path):
        return path.relative_to(self.dir).as_posix()

    # -- fixtures ----------------------------------------------------------

    def meshy(self, spec, tasks=(), images=()):
        """A Meshy folder: requests (key -> asset fields), ledger tasks, concept images."""
        root = self.dir / "art" / "meshy"
        root.mkdir(parents=True, exist_ok=True)
        assets = [{"id": k, "kind": "units", "prompt": f"a {k}", **v} for k, v in spec.items()]
        (root / "requests.json").write_text(json.dumps({"assets": assets}), encoding="utf-8")
        (root / "ledger.json").write_text(json.dumps({"tasks": list(tasks)}), encoding="utf-8")
        for key, name, data in images:
            (root / "out" / key).mkdir(parents=True, exist_ok=True)
            (root / "out" / key / name).write_bytes(data)

    def requests(self, assets, style=STYLE):
        art = self.dir / "art" / "tripo"
        art.mkdir(parents=True, exist_ok=True)
        (art / "requests.json").write_text(json.dumps({"assets": assets}), encoding="utf-8")
        (art / "style.json").write_text(json.dumps(style), encoding="utf-8")

    def picked_concept(self, asset, created=None):
        """A finished, picked Tripo concept of the asset, as an earlier run left it."""
        task_id = f"{asset.key}_concept"
        self.ledger.add(asset=asset.key, stage="concept", candidate=0, task_id=task_id, api="text-to-image", model="banana_pro",
                        hash="h", status="success", credits=20, reserved=20, created=int(time.time()) if created is None else created)
        self.ledger.pick(task_id)
        return task_id

    def run_cmd(self, cmd, api, cap, assets, style=STYLE, **kwargs):
        return self.quietly(cmd, api, assets, style, self.ledger, tripo.Budget(cap), sleep=self.nosleep, workers=kwargs.pop("workers", 0), **kwargs)

    def concepts(self, api, cap, assets, style=STYLE, **kwargs):
        return self.run_cmd(tripo.cmd_concepts, api, cap, assets, style, **kwargs)

    def models(self, api, cap, assets, style=STYLE, **kwargs):
        return self.run_cmd(tripo.cmd_models, api, cap, assets, style, **kwargs)

    def make(self, api, cap, assets, style=STYLE, **kwargs):
        return self.run_cmd(tripo.cmd_make, api, cap, assets, style, **kwargs)


def unit(name="wolf", **fields):
    return tripo.Asset(name, "units", "", f"a {name}", **fields)


def decor(name="peak", variant="", **fields):
    return tripo.Asset(name, "decor", variant, f"a {name}", **fields)


class ConceptTests(TripoTest):
    def test_concepts_make_candidates_and_download_them(self):
        hall = tripo.Asset("hall", "buildings", "", "a great hall")
        peak = decor()
        rider = unit("rider", faction="steppe", age=5)
        api = FakeTripo()
        self.concepts(api, 6 * 20, [hall, peak, rider])
        self.assertEqual(api.created, ["text-to-image"] * 6, "two candidates each, one image per task")
        self.assertEqual(api.bodies[0][1], {
            "model": "banana_pro", "prompt": tripo.concept_prompt(hall, STYLE, False), "size": "2K", "aspect_ratio": "1:1", "output_format": "png"})
        self.assertEqual(api.bodies[2][1]["aspect_ratio"], "4:3", "scenery is drawn wide")
        self.assertEqual(api.bodies[2][1]["prompt"], "One piece of scenery. Subject: a peak. Landscape scenery. Painterly game art, vibrant colours.")
        self.assertEqual(api.bodies[4][1]["prompt"], "One object. Subject: a rider. Horse clans. Age 5 materials. A game unit. Painterly game art, muted colours.")
        concepts = self.stage("concept")
        self.assertEqual([(t["asset"], t["candidate"]) for t in concepts], [(a.key, n) for a in (hall, peak, rider) for n in (0, 1)])
        for t in concepts:
            self.assertEqual((t["status"], t["api"], t["credits"], t["credits_consumed"]), ("success", "text-to-image", 20, 20))
            image = tripo.OUT / t["asset"] / f"concept_{t['candidate']}_{t['task_id']}.png"
            self.assertEqual(image.read_bytes(), f"fake://{t['task_id']}/v0/image.png".encode())
            self.assertEqual(t["image"], f"fake://{t['task_id']}/v0/image.png", "no signed link in the ledger")
        self.assertEqual(self.ledger.spent(), 120)
        # Again: nothing new. A third candidate is only the third.
        self.concepts(api, 0, [hall, peak, rider])
        self.assertEqual(len(api.created), 6)
        self.concepts(api, 60, [hall, peak, rider], candidates=3)
        self.assertEqual(len(api.created), 9)
        self.assertEqual(sorted(t["candidate"] for t in self.stage("concept", "hall")), [0, 1, 2])

    def test_references_are_uploaded_once_and_sent_as_file_tokens(self):
        (self.dir / "refs").mkdir()
        (self.dir / "refs" / "a.png").write_bytes(b"image a")
        (self.dir / "refs" / "b.jpg").write_bytes(b"image b")
        style = {**STYLE, "references": {"decor": ["refs/a.png", "refs/b.jpg"], "nodes": ["refs/a.png"]}}
        peak, ridge, pine = decor("peak"), decor("ridge"), tripo.Asset("pine", "nodes", "d", "a pine, old")
        api = FakeTripo()
        self.concepts(api, 120, [peak, ridge, pine], style=style)
        self.assertEqual(api.created, ["image-to-image"] * 6)
        self.assertEqual(api.uploaded, ["a.png", "b.jpg"], "each reference uploaded once per run")
        decor_body, node_body = api.bodies[0][1], api.bodies[4][1]
        self.assertEqual(decor_body["inputs"], ["file_0", "file_1"])
        self.assertNotIn("input", decor_body)
        self.assertEqual(node_body["input"], "file_0", "a single reference goes as input")
        self.assertNotIn("inputs", node_body)
        self.assertTrue(decor_body["prompt"].startswith(tripo.STYLE_ONLY))
        entry = self.stage("concept", "peak")[0]
        self.assertEqual((entry["api"], entry["references"]), ("image-to-image", ["refs/a.png", "refs/b.jpg"]))
        self.assertNotIn("file_", self.ledger.path.read_text(encoding="utf-8"), "the ledger keeps paths, not tokens")
        # A reference whose bytes change is a new concept.
        (self.dir / "refs" / "a.png").write_bytes(b"another image a")
        self.assertNotEqual(tripo.concept_request(peak, style, 0).hash, entry["hash"])
        # Too many, missing or GIF references are refused before anything is sent.
        for refs, says in ((["refs/a.png"] * 11, "at most 10"), (["refs/gone.png"], "does not exist"), (["refs/a.gif"], "is not a")):
            api = FakeTripo()
            with self.assertRaises(tripo.TripoError, msg=says) as e:
                self.concepts(api, 100, [peak], style={**STYLE, "references": {"decor": refs}})
            self.assertIn(says, str(e.exception))
            self.assertEqual(api.calls, [])

    def test_concept_prompts_layer_the_style_and_each_layer_is_in_the_hash(self):
        mage = unit("mage", faction="dune", magic=True, age=2)
        self.assertEqual(tripo.concept_prompt(mage, STYLE, False),
                         "One object. Subject: a mage. Desert scholars. Sun-glass sorcery. Age 2 materials. A game unit. Painterly game art, muted colours.")
        plain = tripo.concept_request(mage, STYLE, 0).hash
        for changed in (dataclasses.replace(mage, magic=False), dataclasses.replace(mage, age=3), dataclasses.replace(mage, prompt="a sage")):
            self.assertNotEqual(tripo.concept_request(changed, STYLE, 0).hash, plain)
        self.assertNotEqual(tripo.concept_request(mage, STYLE, 1).hash, plain, "and the candidate number")
        self.assertNotEqual(tripo.concept_request(mage, {**STYLE, "look": "Pixel art"}, 0).hash, plain)
        with self.assertRaises(tripo.TripoError) as e:
            tripo.concept_prompt(unit("giant"), {**STYLE, "look": "x" * 1800}, False)
        self.assertIn("max 1800", str(e.exception))

    def test_concepts_leave_picked_made_and_text_assets_alone_unless_named(self):
        wolf = unit("wolf")
        self.picked_concept(wolf)
        house = tripo.Asset("house", "buildings", "", "a house")
        house.target().parent.mkdir(parents=True)
        house.target().write_bytes(b"made")
        stump = tripo.Asset("pine_stump", "nodes", "", "a stump", route="text")
        api = FakeTripo()
        self.concepts(api, 100, [wolf, house, stump])
        self.assertEqual(api.created, [])
        self.concepts(api, 100, [wolf, house, stump], named={"wolf", "pine_stump"})
        self.assertEqual(api.created, ["text-to-image"] * 4)
        self.assertEqual(self.ledger.picked("wolf")["task_id"], "wolf_concept", "new candidates do not change the pick")


class ModelTests(TripoTest):
    def test_pick_then_models_install_each_kind_in_its_place(self):
        assets = [unit("wolf"), tripo.Asset("spear", "props", "", "a spear"), tripo.Asset("hall", "buildings", "", "a hall"),
                  tripo.Asset("pine", "nodes", "d", "a pine, old"), decor("peak", "a")]
        concepts = {a.key: self.picked_concept(a) for a in assets}
        api = FakeTripo()
        self.models(api, 5 * 40, assets)
        self.assertEqual(api.created, ["image-to-model"] * 5)
        self.assertEqual(api.bodies[0][1], {"input": "wolf_concept", **MODEL_BODY, "face_limit": 30000},
                         "a fresh Tripo concept goes by its task id")
        self.assertEqual([b["face_limit"] for _, b in api.bodies], [30000, 3000, 30000, 8000, 10000])
        self.assertEqual(api.uploaded, [])
        for a, path in zip(assets, ("art/units/source/wolf.glb", "art/units/source/props/spear.glb", "art/static/source/buildings/hall.glb",
                                    "art/static/source/nodes/pine_d.glb", "art/static/source/decor/peak_a.glb")):
            self.assertEqual(self.rel(a.target()), path)
            model = self.stage("model", a.key)[0]
            self.assertEqual(a.target().read_bytes(), f"fake://{model['task_id']}/v0/model.glb".encode())
            self.assertEqual((model["installed"], model["concept"], model["concept_from"], model["credits"]), (path, concepts[a.key], "tripo", 40))
            self.assertEqual(model["rendered"], f"fake://{model['task_id']}/v0/preview.webp")
            self.assertTrue((tripo.OUT / a.key / f"model_{model['task_id']}.glb").exists(), "every download is kept")
            self.assertTrue((tripo.OUT / a.key / f"model_{model['task_id']}.webp").exists())
        self.assertNotIn("Signature", self.ledger.path.read_text(encoding="utf-8"))
        self.assertEqual(self.ledger.spent(), 5 * 20 + 5 * 40)
        # Again: nothing. A new pick is made, and is the only pick.
        self.models(api, 0, assets)
        self.assertEqual(len(api.created), 5)

    def test_an_old_tripo_concept_is_uploaded_but_is_the_same_request(self):
        wolf = unit("wolf")
        task_id = self.picked_concept(wolf, created=1)
        pick = tripo.effective_pick(wolf, self.ledger)
        fresh = tripo.model_request(wolf, STYLE, pick, now=2)
        image = tripo.concept_image("wolf", 0, task_id)
        image.parent.mkdir(parents=True)
        image.write_bytes(b"png")
        old = tripo.model_request(wolf, STYLE, pick, now=10**9)
        self.assertEqual((fresh.body["input"], fresh.uploads), (task_id, ()))
        self.assertEqual((old.body["input"], old.uploads), ("upload:0", (image,)))
        self.assertEqual(fresh.hash, old.hash, "the hash names the concept, not how it travels")
        api = FakeTripo()
        self.models(api, 40, [wolf])
        self.assertEqual((api.uploaded, api.bodies[0][1]["input"]), (["concept_0_wolf_concept.png"], "file_0"))

    def test_meshy_picks_never_made_are_made_from_their_uploaded_concept(self):
        tasks = [
            {"asset": "skiff", "stage": "concept", "candidate": 1, "task_id": "m-skiff", "status": "SUCCEEDED", "picked": True},
            {"asset": "hawk", "stage": "concept", "candidate": 0, "task_id": "m-hawk", "status": "SUCCEEDED", "picked": True},
            {"asset": "wolf", "stage": "concept", "candidate": 0, "task_id": "m-wolf", "status": "SUCCEEDED", "picked": True},
            {"asset": "wolf", "stage": "model", "task_id": "m-wolf-model", "concept": "m-wolf", "status": "SUCCEEDED"},
            {"asset": "monk", "stage": "concept", "candidate": 0, "task_id": "m-monk", "status": "SUCCEEDED", "picked": True},
            {"asset": "bear", "stage": "concept", "candidate": 0, "task_id": "m-bear0", "status": "SUCCEEDED"},
        ]
        images = [("skiff", "concept_1_m-skiff.png", b"skiff png"), ("hawk", "concept_0_m-hawk.png", b"hawk png")]
        self.meshy({"skiff": {"faction": "dune", "age": 5, "polycount": 20000}, "hawk": {}, "wolf": {}, "monk": {"rig": True}, "bear": {}},
                   tasks, images)
        found = tripo.meshy_finish()
        self.assertEqual([a.key for a in found], ["skiff", "hawk", "monk"], "picked, never made: the made wolf and unpicked bear are not")
        skiff, hawk, monk = found
        self.assertEqual((skiff.batch, skiff.source, skiff.meshy_concept, skiff.face_limit, skiff.faction), ("meshy-finish", "meshy", "m-skiff", 20000, "dune"))
        self.assertTrue(monk.rig)
        api = FakeTripo()
        self.models(api, 80, found)
        self.assertIn("monk: a rigged humanoid: meshy.py makes it", self.out)
        self.assertEqual(api.uploaded, ["concept_1_m-skiff.png", "concept_0_m-hawk.png"])
        self.assertEqual(api.bodies[0][1], {"input": "file_0", **MODEL_BODY, "face_limit": 20000})
        self.assertEqual(api.bodies[1][1]["face_limit"], 30000)
        for a in (skiff, hawk):
            self.assertEqual(self.rel(a.target()), f"art/units/source/{a.key}.glb", "where meshy.py would install it")
            self.assertTrue(a.target().exists())
        model = self.stage("model", "skiff")[0]
        self.assertEqual((model["concept"], model["concept_from"], model["image"]), ("m-skiff", "meshy", "art/meshy/out/skiff/concept_1_m-skiff.png"))
        # The hash names the Meshy concept: a rerun finds it without its image.
        (tripo.MESHY_OUT / "skiff" / "concept_1_m-skiff.png").unlink()
        self.models(api, 0, found)
        self.assertEqual(len(api.created), 2)
        # A missing image stops only its own asset, before anything is paid.
        skiff.target().unlink()
        self.ledger.tasks = [t for t in self.ledger.tasks if t["asset"] != "skiff"]
        api = FakeTripo()
        with self.assertRaises(tripo.TripoError) as e:
            self.models(api, 80, found)
        self.assertIn("skiff: the concept image art/meshy/out/skiff/concept_1_m-skiff.png is missing", str(e.exception))
        self.assertEqual(api.created, [])

    def test_a_model_already_there_is_never_paid_for_again(self):
        # The eleven were made in Studio by hand and copied into place: a Meshy
        # pick (or a text filler) whose file is there is done, whoever made it.
        tasks = [{"asset": "skiff", "stage": "concept", "candidate": 0, "task_id": "m-skiff", "status": "SUCCEEDED", "picked": True}]
        self.meshy({"skiff": {}}, tasks, [("skiff", "concept_0_m-skiff.png", b"png")])
        skiff = tripo.meshy_finish()[0]
        stump = tripo.Asset("pine_stump", "nodes", "", "a stump", route="text")
        for a in (skiff, stump):
            a.target().parent.mkdir(parents=True, exist_ok=True)
            a.target().write_bytes(b"glTF made by hand")
        api = FakeTripo()
        self.models(api, 1000, [skiff])
        self.make(api, 1000, [stump])
        self.assertEqual(api.calls, [])
        self.assertIn("skiff: already there (art/units/source/skiff.glb, made elsewhere); delete it to make it here", self.out)
        self.assertIn("pine_stump: already there", self.out)
        self.assertEqual(self.quietly(tripo.cmd_plan, [skiff, stump], STYLE, self.ledger, None), 0)
        self.assertEqual(skiff.target().read_bytes(), b"glTF made by hand")
        # Deleted: made here.
        skiff.target().unlink()
        self.models(api, 40, [skiff])
        self.assertEqual(api.created, ["image-to-model"])

    def test_the_meshy_pick_path_finds_exactly_the_eleven_units_in_the_real_ledger(self):
        found = tripo.meshy_finish(REAL_ROOT / "art" / "meshy")
        self.assertEqual({a.key for a in found}, ELEVEN)
        for a in found:
            self.assertEqual((a.kind, a.rig, a.batch, a.source), ("units", False, "meshy-finish", "meshy"), a.key)
            self.assertEqual(a.target(), tripo.UNITS / f"{a.key}.glb")

    def test_make_makes_text_fillers_and_only_them(self):
        stump = tripo.Asset("pine_stump", "nodes", "", "a pine stump", route="text", face_limit=3000)
        peak = decor("peak", "a")
        wolf = unit("wolf")
        api = FakeTripo()
        self.make(api, 100, [stump, peak, wolf])
        self.assertEqual(api.created, ["text-to-model"])
        self.assertEqual(api.bodies[0][1], {
            "prompt": "a pine stump. A natural resource. Painted miniature.", "negative_prompt": "base plate, text",
            "model": "v3.1-20260211", "texture": True, "pbr": False, "texture_quality": "standard", "face_limit": 3000})
        self.assertEqual(self.rel(stump.target()), "art/static/source/nodes/pine_stump.glb")
        self.assertTrue(stump.target().exists())
        self.assertEqual(self.stage("text_model")[0]["credits"], 20)
        # Named: a concept-route decor with no pick is made from text; a unit never is.
        self.make(api, 100, [stump, peak, wolf], named={"peak", "wolf"})
        self.assertEqual(api.created, ["text-to-model"] * 2)
        self.assertIn("wolf: units are made from a concept", self.out)
        self.assertTrue(peak.target().exists())

    def test_text_prompts_fit_tripos_limit(self):
        long = unit("giant", faction="dune", magic=True, age=3)
        long = dataclasses.replace(long, prompt="a giant " + "with a long description " * 50)
        with self.assertRaises(tripo.TripoError) as e:
            tripo.text_prompt(long, STYLE)
        self.assertIn("max 1024", str(e.exception))
        trimmed = tripo.text_prompt(long, STYLE, trim=True)
        self.assertLessEqual(len(trimmed), 1024)
        self.assertTrue(trimmed.startswith("a giant with"))


class ResumeTests(TripoTest):
    def test_concepts_resume_after_a_crash_without_paying_twice(self):
        assets = [unit("wolf"), tripo.Asset("hall", "buildings", "", "a hall")]
        api = FakeTripo()
        api.crash = ("POST", tripo.TASKS_LIST)
        with self.assertRaises(Crash):
            self.concepts(api, 80, assets, concurrency=3)
        self.assertEqual(api.created, ["text-to-image"] * 3)
        self.restart()
        self.assertEqual([t["status"] for t in self.stage("concept")], ["queued"] * 3, "each task was saved as it started")
        self.concepts(api, 20, assets)  # only the candidate that never started is paid
        self.assertEqual(len(api.created), 4)
        self.assertEqual(len(self.stage("concept")), 4)
        for t in self.stage("concept"):
            self.assertTrue((tripo.OUT / t["asset"] / f"concept_{t['candidate']}_{t['task_id']}.png").exists())
        # An image whose download is gone is fetched again, not paid again.
        image = tripo.OUT / "hall" / f"concept_1_{self.stage('concept', 'hall')[1]['task_id']}.png"
        image.unlink()
        self.concepts(api, 0, assets)
        self.assertTrue(image.exists())
        self.assertEqual(len(api.created), 4)

    def test_models_resume_after_a_crash_without_paying_twice(self):
        assets = [unit("wolf"), unit("bear")]
        for a in assets:
            self.picked_concept(a)
        api = FakeTripo()
        api.crash = ("POST", tripo.TASKS_LIST)
        with self.assertRaises(Crash):
            self.models(api, 80, assets)
        self.assertEqual(api.created, ["image-to-model"] * 2)
        self.restart()
        self.models(api, 0, assets)
        self.assertEqual(len(api.created), 2)
        self.assertTrue(all(a.target().exists() for a in assets))
        self.assertEqual(len(self.stage("model")), 2)

    def test_dying_while_fetching_a_finished_model_costs_nothing(self):
        wolf = unit("wolf")
        self.picked_concept(wolf)

        def dies(url, to):
            raise Crash(url)

        tripo.download = dies
        api = FakeTripo()
        with self.assertRaises(Crash):
            self.models(api, 40, [wolf])
        tripo.download = fake_download
        self.restart()
        self.assertEqual(self.stage("model")[0]["status"], "success")
        self.assertFalse(wolf.target().exists())
        self.models(api, 0, [wolf])
        self.assertEqual(api.created, ["image-to-model"])
        self.assertTrue(wolf.target().exists())

    def test_a_made_model_whose_file_is_gone_is_reinstalled_without_paying(self):
        wolf = unit("wolf")
        self.picked_concept(wolf)
        api = FakeTripo()
        self.models(api, 40, [wolf])
        made = wolf.target().read_bytes()
        # From art/tripo/out/: no API call at all.
        wolf.target().unlink()
        api = FakeTripo()
        self.models(api, 0, [wolf])
        self.assertEqual(api.calls, [])
        self.assertEqual(wolf.target().read_bytes(), made)
        self.assertIn("reinstalled art/units/source/wolf.glb", self.out)
        # Its download gone too: queried for fresh links, still nothing paid.
        wolf.target().unlink()
        shutil.rmtree(tripo.OUT)
        old = FakeTripo()
        old.seed(self.stage("model")[0]["task_id"], "image-to-model")
        self.models(old, 0, [wolf])
        self.assertEqual(old.created, [])
        self.assertEqual([c[1] for c in old.calls], [tripo.TASKS_LIST])
        self.assertTrue(wolf.target().exists())
        # Tripo no longer knows it: an error naming the task, nothing paid.
        wolf.target().unlink()
        shutil.rmtree(tripo.OUT)
        gone = FakeTripo()
        with self.assertRaises(tripo.TripoError) as e:
            self.models(gone, 0, [wolf])
        self.assertIn(f"no longer knows model task {self.stage('model')[0]['task_id']}", str(e.exception))
        self.assertEqual(gone.created, [])

    def test_an_expired_link_is_queried_again_for_a_fresh_one(self):
        wolf = unit("wolf")
        self.picked_concept(wolf)

        def first_links_expired(url, to):
            if "/v0/" in url:
                raise tripo.DownloadError(f"downloading {to.name} failed (HTTP Error 403: Forbidden)")
            fake_download(url, to)

        tripo.download = first_links_expired
        api = FakeTripo()
        self.models(api, 40, [wolf])
        task_id = self.stage("model")[0]["task_id"]
        self.assertEqual(wolf.target().read_bytes(), f"fake://{task_id}/v1/model.glb".encode(), "installed from the fresh link")
        self.assertIn("asking Tripo for fresh links", self.out)
        self.assertEqual(api.created, ["image-to-model"])
        self.assertEqual(self.ledger.spent(), 20 + 40)

    def test_links_that_never_work_leave_it_for_a_rerun_that_pays_nothing(self):
        wolf = unit("wolf")
        self.picked_concept(wolf)

        def broken(url, to):
            raise tripo.DownloadError(f"downloading {to.name} failed (connection reset)")

        tripo.download = broken
        api = FakeTripo()
        with self.assertRaises(tripo.TripoError) as e:
            self.models(api, 40, [wolf])
        self.assertIn("finished but was not installed", str(e.exception))
        self.assertEqual(api.queries.count(1), 2 + tripo.REFETCHES - 1, "polled to the end, then re-queried for fresh links")
        tripo.download = fake_download
        self.models(api, 0, [wolf])
        self.assertEqual(api.created, ["image-to-model"])
        self.assertTrue(wolf.target().exists())

    def test_a_failed_task_is_refunded_and_made_again(self):
        wolf = unit("wolf")
        self.picked_concept(wolf)
        with self.assertRaises(tripo.TripoError) as e:
            self.models(FakeTripo(fail={"image-to-model"}), 40, [wolf])
        self.assertIn("wolf: model failed (no luck (code 2008))", str(e.exception))
        failed = self.stage("model")[0]
        self.assertEqual((failed["status"], failed["credits"], failed["error"]), ("failed", 0, "no luck (code 2008)"))
        api = FakeTripo()
        self.models(api, 40, [wolf])
        self.assertEqual(api.created, ["image-to-model"])
        self.assertTrue(wolf.target().exists())

    def test_an_uncertain_create_is_never_sent_again_until_resolved(self):
        wolf, bear = unit("wolf"), unit("bear")
        for a in (wolf, bear):
            self.picked_concept(a)
        api = FakeTripo(uncertain=1)
        with self.assertRaises(tripo.TripoError) as e:
            self.models(api, 80, [wolf, bear])
        self.assertIn("may or may not have made a task", str(e.exception))
        self.assertIn("tripo.py resolve wolf", str(e.exception))
        self.assertEqual(len(api.created), 2, "sent once, never retried")
        self.assertTrue(bear.target().exists())
        entry = self.stage("model", "wolf")[0]
        self.assertEqual((entry["status"], entry["credits"]), ("uncertain", 40))
        # A rerun does not send it again.
        self.restart()
        with self.assertRaises(tripo.TripoError):
            self.models(api, 80, [wolf, bear])
        self.assertEqual(len(api.created), 2)
        # The task it made (found with `usage`) is recorded and finished without paying.
        made = f"{api.prefix}1"
        self.quietly(tripo.cmd_resolve, api, self.ledger, "wolf", made)
        self.models(api, 0, [wolf, bear])
        self.assertEqual(len(api.created), 2)
        self.assertEqual(wolf.target().read_bytes(), f"fake://{made}/v0/model.glb".encode())
        self.assertEqual(self.stage("model", "wolf")[0]["credits"], 40)
        # Or it made none: the next run makes it.
        other = unit("other")
        self.picked_concept(other)
        with self.assertRaises(tripo.TripoError):
            self.models(FakeTripo(uncertain=1), 40, [other])
        self.quietly(tripo.cmd_resolve, None, self.ledger, "other", "none")
        api = FakeTripo()
        self.models(api, 40, [other])
        self.assertEqual(api.created, ["image-to-model"])
        self.assertTrue(other.target().exists())


class BudgetAndQueueTests(TripoTest):
    def test_the_credit_cap_never_starts_what_it_cannot_finish(self):
        assets = [unit("a"), unit("b"), unit("c")]
        api = FakeTripo()
        with self.assertRaises(tripo.TripoError) as e:
            self.concepts(api, 50, assets)
        self.assertEqual(api.created, ["text-to-image"] * 2, "50 credits are two 20-credit concepts")
        self.assertIn("not started (over --max-credits 50): b#0, b#1, c#0, c#1", str(e.exception))
        for a in assets:
            self.picked_concept(a)
        api = FakeTripo()
        with self.assertRaises(tripo.TripoError) as e:
            self.models(api, 39, assets[:1])
        self.assertEqual(api.calls, [], "a model's 40 credits are reserved before it starts")
        with self.assertRaises(tripo.TripoError):
            self.models(api, 100, assets)
        self.assertEqual(api.created, ["image-to-model"] * 2)

    def test_the_balance_caps_a_run_and_a_balance_error_stops_new_tasks(self):
        self.requests([{"id": "wolf", "kind": "units", "prompt": "a wolf", "batch": "X"},
                       {"id": "bear", "kind": "units", "prompt": "a bear", "batch": "X"}])
        for name in ("wolf", "bear"):
            self.picked_concept(unit(name))
        api = FakeTripo(balance=50)
        self.patch("POLL_SECONDS", 0.01)  # main() runs with real threads and real sleeps
        self.patch("make_http", lambda key, *a, **k: api)
        self.patch("api_key", lambda required=True: "key")
        err = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()) as out, contextlib.redirect_stderr(err):
            self.assertEqual(tripo.main(["models", "--batch", "X", "--max-credits", "1000"]), 1)
        self.assertIn("balance 50 credits; spending at most 50", out.getvalue())
        self.assertIn("not started (over --max-credits 1000 and the balance of 50): bear", err.getvalue())
        self.assertEqual(api.created, ["image-to-model"])
        # Tripo refusing for credits (code 2010) stops every task not started.
        api = FakeTripo(balance=60)
        for name in ("c", "d", "e"):
            self.picked_concept(unit(name))
        with self.assertRaises(tripo.TripoError) as e:
            self.models(api, 1000, [unit("c"), unit("d"), unit("e")], concurrency=1)
        self.assertEqual(api.created, ["image-to-model"])
        self.assertIn("not started: Tripo said the balance is too low", str(e.exception))
        self.assertTrue(unit("c").target().exists())

    def test_full_queues_are_waited_out_per_category(self):
        assets = [unit("a"), unit("b"), tripo.Asset("hall", "buildings", "", "a hall")]
        api = FakeTripo(limits={"image generation": 1}, busy=["image generation"])
        self.concepts(api, 120, assets)
        self.assertEqual(len(self.stage("concept")), 6)
        self.assertLessEqual(api.peak["image generation"], 1)
        self.assertIn("Tripo's image generation queue is full", self.out)
        for a in assets:
            self.quietly(tripo.cmd_pick, self.ledger, a, "0")
        api = FakeTripo(limits={"3D generation": 2})
        self.models(api, 120, assets)
        self.assertTrue(all(a.target().exists() for a in assets))
        self.assertLessEqual(api.peak["3D generation"], 2)

    def test_concurrency_caps_tasks_in_flight(self):
        assets = [decor(f"rock{i}", route="text") for i in range(12)]
        api = FakeTripo(polls=3)
        self.make(api, 1000, assets, concurrency=4)
        self.assertEqual(api.created, ["text-to-model"] * 12)
        self.assertLessEqual(api.peak_total, 4)
        self.assertTrue(all(a.target().exists() for a in assets))

    def test_later_stages_start_first(self):
        wolf, hall = unit("wolf"), tripo.Asset("hall", "buildings", "", "a hall")
        self.picked_concept(wolf)
        jobs = tripo.concept_jobs(hall, STYLE, self.ledger, 1) + [tripo.model_job(wolf, tripo.effective_pick(wolf, self.ledger), STYLE, self.ledger)]
        api = FakeTripo(polls=1)
        self.quietly(tripo.run_jobs, api, self.ledger, jobs, 1, self.nosleep, time.time, 0)
        self.assertEqual(api.created, ["image-to-model", "text-to-image"])

    def test_tasks_are_polled_in_batches_of_100(self):
        assets = [decor(f"rock{i}", route="text") for i in range(250)]
        api = FakeTripo(polls=2)
        self.make(api, 10**6, assets, concurrency=300)
        self.assertEqual(len(api.created), 250)
        self.assertEqual(max(api.queries), 100)
        self.assertEqual(api.queries[:3], [100, 100, 50])
        self.assertFalse([c for c in api.calls if c[0] == "GET"], "no GET per task")

    def test_threads_make_everything_once(self):
        assets = [decor(f"rock{i}", route="text") for i in range(30)]
        api = FakeTripo(polls=3)
        self.make(api, 10**6, assets, concurrency=5, workers=4)
        self.assertEqual(len(api.created), 30)
        self.assertEqual(len({t["asset"] for t in self.stage("text_model")}), 30, "one task per asset")
        self.assertLessEqual(api.peak_total, 5)
        self.assertTrue(all(a.target().exists() for a in assets))

    def test_a_charge_above_the_reservation_is_reported(self):
        wolf = unit("wolf")
        self.picked_concept(wolf)
        self.models(FakeTripo(charge={"image-to-model": 45}), 40, [wolf])
        self.assertIn("WARNING wolf: Tripo charged 45 credits for a model, more than the 40 reserved", self.out)
        self.assertEqual(self.stage("model")[0]["credits"], 45)

    def test_a_dry_run_shows_every_request_and_calls_nothing(self):
        wolf, hall = unit("wolf"), tripo.Asset("hall", "buildings", "", "a hall")
        self.picked_concept(wolf)
        self.models(None, 40, [wolf], dry_run=True)
        self.concepts(None, 20, [hall], dry_run=True)
        self.assertIn('"input": "wolf_concept"', self.out)
        self.assertIn("POST /generation/image-to-model (40 credits)", self.out)
        self.assertIn("hall#0: POST /generation/text-to-image (20 credits)", self.out)
        self.assertIn("not started (over --max-credits 20): hall#1", self.out)
        self.assertEqual(self.stage("model"), [])


class HttpTests(unittest.TestCase):
    """The HTTP layer against a fake urlopen: the key, the URL, the bodies,
    the backoff and which failures are retried."""

    def setUp(self):
        self.answers, self.sent, self.slept = [], [], []
        self.now = 1000.0
        self.addCleanup(setattr, tripo, "urlopen", tripo.urlopen)
        tripo.urlopen = self.urlopen
        self.http = tripo.make_http("sk-SECRET", sleep=self.sleep, clock=lambda: self.now)

    def sleep(self, seconds):
        self.slept.append(round(seconds, 3))
        self.now += seconds

    def answer(self, status=200, body=None, headers=None, error=None):
        self.answers.append((status, body, headers or {}, error))

    def urlopen(self, req, timeout):
        self.sent.append(req)
        status, body, headers, error = self.answers.pop(0)
        if error is not None:
            raise error
        msg = Message()
        for k, v in headers.items():
            msg[k] = str(v)
        raw = json.dumps(body).encode() if not isinstance(body, bytes) else body
        if status != 200:
            raise urllib.error.HTTPError(req.full_url, status, "error", msg, io.BytesIO(raw))
        response = io.BytesIO(raw)
        response.headers = msg
        return response

    def ok(self, data, **kw):
        self.answer(200, {"code": 0, "status": "success", "data": data}, **kw)

    def test_requests_go_to_the_international_api_with_the_key(self):
        self.ok({"task_id": "t1"})
        self.assertEqual(self.http("POST", tripo.TEXT_TO_IMAGE, {"prompt": "x"}), {"task_id": "t1"})
        req = self.sent[0]
        self.assertEqual(req.full_url, "https://openapi.tripo3d.ai/v3/generation/text-to-image")
        self.assertEqual((req.get_method(), req.get_header("Authorization"), req.get_header("Content-type")), ("POST", "Bearer sk-SECRET", "application/json"))
        self.assertEqual(json.loads(req.data), {"prompt": "x"})
        self.ok({"balance": 0.0, "frozen": 0.0})
        self.assertEqual(self.http("GET", tripo.BALANCE), {"balance": 0.0, "frozen": 0.0})
        self.assertIsNone(self.sent[1].data)

    def test_an_upload_is_one_multipart_file(self):
        image = Path(tempfile.mkdtemp()) / "pine.png"
        self.addCleanup(shutil.rmtree, image.parent, ignore_errors=True)
        image.write_bytes(b"\x89PNG fake bytes")
        self.ok({"file_token": "file_abc"})
        self.assertEqual(self.http("POST", tripo.FILES, None, upload=image), {"file_token": "file_abc"})
        req = self.sent[0]
        self.assertEqual(req.full_url, "https://openapi.tripo3d.ai/v3/files")
        ctype = req.get_header("Content-type")
        self.assertTrue(ctype.startswith("multipart/form-data; boundary="), ctype)
        boundary = ctype.split("boundary=", 1)[1].encode()
        self.assertTrue(req.data.startswith(b"--" + boundary + b"\r\n"))
        self.assertIn(b'Content-Disposition: form-data; name="file"; filename="pine.png"\r\nContent-Type: image/png\r\n\r\n\x89PNG fake bytes\r\n', req.data)
        self.assertTrue(req.data.endswith(b"--" + boundary + b"--\r\n"))

    def test_rate_limits_are_waited_out_from_the_headers_then_retried(self):
        limited = {"code": 1007, "message": "Rate limit exceeded"}
        self.answer(429, limited, {"Retry-After": 7})
        self.answer(429, limited, {"X-RateLimit-Reset": 1019})  # now is 1007 after the first wait
        self.answer(429, limited)
        self.answer(429, limited)
        self.ok({"task_id": "t1"})
        self.assertEqual(self.http("POST", tripo.IMAGE_TO_MODEL, {"input": "x"}), {"task_id": "t1"})
        self.assertEqual(self.slept, [7, 12, 4, 8], "Retry-After, then X-RateLimit-Reset, then 2**attempt")
        self.assertEqual(len({r.data for r in self.sent}), 1, "the same request each time: nothing was created")
        # A window used up on a success holds that endpoint (only) until it resets.
        self.slept.clear()
        self.ok({"tasks": {}, "missed": []}, headers={"X-RateLimit-Remaining": 0, "X-RateLimit-Reset": self.now + 5})
        self.ok({"tasks": {}, "missed": []})
        self.ok({"balance": 1.0})
        self.http("POST", tripo.TASKS_LIST, {"task_ids": ["a"]})
        self.http("GET", tripo.BALANCE)
        self.assertEqual(self.slept, [])
        self.http("POST", tripo.TASKS_LIST, {"task_ids": ["a"]})
        self.assertEqual(self.slept, [5])

    def test_a_full_category_and_no_credits_are_never_retried(self):
        self.answer(429, {"code": 2000, "message": "You have exceeded the limit of generation"}, {"Retry-After": 30})
        with self.assertRaises(tripo.QueueFull) as e:
            self.http("POST", tripo.TEXT_TO_IMAGE, {"prompt": "x"})
        self.assertEqual(e.exception.retry_after, 30)
        self.answer(403, {"code": 2010, "message": "You don't have enough credit to create this task"})
        with self.assertRaises(tripo.NoCredits):
            self.http("POST", tripo.TEXT_TO_IMAGE, {"prompt": "x"})
        self.assertEqual((len(self.sent), self.slept), (2, []))

    def test_a_lost_answer_to_a_create_is_uncertain_but_queries_are_retried(self):
        self.answer(502, b"<html>bad gateway</html>")
        with self.assertRaises(tripo.Uncertain):
            self.http("POST", tripo.IMAGE_TO_MODEL, {"input": "x"})
        self.answer(error=urllib.error.URLError(socket.timeout("timed out")))
        with self.assertRaises(tripo.Uncertain):
            self.http("POST", tripo.IMAGE_TO_MODEL, {"input": "x"})
        self.assertEqual(len(self.sent), 2, "never sent twice")
        # A refused connection never reached Tripo: safe to send again.
        self.answer(error=urllib.error.URLError(ConnectionRefusedError("refused")))
        self.ok({"task_id": "t1"})
        self.assertEqual(self.http("POST", tripo.IMAGE_TO_MODEL, {"input": "x"}), {"task_id": "t1"})
        # Queries and uploads are retried on 5xx and timeouts.
        self.answer(500, {"code": 1001, "message": "server"})
        self.answer(error=TimeoutError("timed out"))
        self.ok({"tasks": {}, "missed": []})
        self.assertEqual(self.http("POST", tripo.TASKS_LIST, {"task_ids": ["a"]}), {"tasks": {}, "missed": []})

    def test_the_key_is_never_in_a_message(self):
        self.answer(401, {"code": 2, "message": "bad key sk-SECRET"})
        with self.assertRaises(tripo.TripoError) as e:
            self.http("GET", tripo.BALANCE)
        self.assertIn("the API key was rejected", str(e.exception))
        self.answer(400, {"code": 1004, "message": "parameter echo: sk-SECRET"})
        with self.assertRaises(tripo.TripoError) as e:
            self.http("POST", tripo.TEXT_TO_IMAGE, {"prompt": "x"})
        self.assertNotIn("sk-SECRET", str(e.exception))
        self.assertIn("<key>", str(e.exception))


class CommandLineTests(TripoTest):
    def test_the_key_comes_from_env_or_dotenv_and_is_never_printed(self):
        (self.dir / ".env").write_text('OTHER=1\nTRIPO_API_KEY="tsk_from_dotenv"\n', encoding="utf-8")
        saved = os.environ.pop("TRIPO_API_KEY", None)
        self.addCleanup(lambda: os.environ.__setitem__("TRIPO_API_KEY", saved) if saved is not None else None)
        self.assertEqual(tripo.api_key(), "tsk_from_dotenv")
        os.environ["TRIPO_API_KEY"] = "tsk_from_env"
        self.addCleanup(os.environ.pop, "TRIPO_API_KEY", None)
        self.assertEqual(tripo.api_key(), "tsk_from_env")
        seen = []

        def fake_urlopen(req, timeout):
            seen.append(req.get_header("Authorization"))
            msg = Message()
            raise urllib.error.HTTPError(req.full_url, 401, "no", msg, io.BytesIO(b'{"code": 2, "message": "key tsk_from_env is bad"}'))

        self.patch("urlopen", fake_urlopen)
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            self.assertEqual(tripo.main(["balance"]), 1)
        self.assertEqual(seen, ["Bearer tsk_from_env"])
        self.assertNotIn("tsk_from_env", out.getvalue() + err.getvalue())

    def test_requests_are_checked_against_the_schema(self):
        self.meshy({"oak": {"kind": "nodes", "variants": {"a": "tall", "c": "wide"}, "retired": ["c"]}})
        good = {"id": "peak", "kind": "decor", "prompt": "a peak", "batch": "P0"}
        for change, says in (
            ({"kind": "scenery"}, "kind must be one of"), ({"framing": "portrait"}, "framing must be one of"),
            ({"rig": True}, "Meshy only"), ({"height_m": 2}, "Meshy only"), ({"faction": "romans"}, "is not in style.json factions"),
            ({"age": 6}, "age must be"), ({"magic": True}, "magic needs a faction"), ({"face_limit": 50}, "face_limit"),
            ({"face_limit": 9000, "polycount": 9000}, "not both"), ({"route": "fast"}, "route must be"),
            ({"kind": "units", "route": "text"}, "route text is for"), ({"batch": "meshy-finish"}, "Meshy picks"),
            ({"batch": 2}, "batch must be"), ({"peak": 1}, "unknown field peak"), ({"prompt": ""}, "prompt"),
            ({"id": "Peak"}, "lower-case"), ({"variants": {"A": "x"}}, "variants"),
        ):
            self.requests([{**good, **change}])
            with self.assertRaises(tripo.TripoError, msg=repr(change)) as e:
                tripo.load_assets(style=STYLE)
            self.assertIn(says, str(e.exception))
        # Meshy's names stay Meshy's, retired ones included.
        for name, variants in (("oak", {"c": "wide"}), ("oak", {"a": "tall"})):
            self.requests([{"id": name, "kind": "nodes", "prompt": "an oak", "variants": variants}])
            with self.assertRaises(tripo.TripoError) as e:
                tripo.load_assets(style=STYLE)
            self.assertIn("a Meshy asset has this name", str(e.exception))
        self.requests([{"id": "oak", "kind": "nodes", "prompt": "an oak", "variants": {"d": "forest", "e": "leaning"}, "retired": ["e"], "batch": "P1"},
                       {**good, "variants": {"a": "snowy", "b": "bare"}, "polycount": 20000},
                       {"id": "pine_stump", "kind": "nodes", "prompt": "a stump", "route": "text", "batch": "P1", "_note": "comments are fine"}])
        assets = tripo.load_assets(style=STYLE)
        self.assertEqual([a.key for a in assets], ["oak_d", "peak_a", "peak_b", "pine_stump"], "retired oak_e is never made")
        self.assertEqual((assets[1].framing, assets[1].face_limit, assets[3].route), ("scenery", 20000, "text"))
        self.assertEqual([a.key for a in tripo.load_assets(batch="P1", style=STYLE)], ["oak_d", "pine_stump"])
        self.assertEqual([a.key for a in tripo.load_assets({"peak"}, style=STYLE)], ["peak_a", "peak_b"])
        with self.assertRaises(tripo.TripoError) as e:
            tripo.load_assets({"peak", "pek"}, style=STYLE)
        self.assertIn("--only pek: no such asset", str(e.exception))
        with self.assertRaises(tripo.TripoError) as e:
            tripo.find_asset(assets, "peak")
        self.assertIn("peak_a, peak_b", str(e.exception))
        self.requests([good, good])
        with self.assertRaises(tripo.TripoError) as e:
            tripo.load_assets(style=STYLE)
        self.assertIn("peak: two assets", str(e.exception))

    def test_pick_takes_only_a_finished_concept_of_the_asset(self):
        wolf = unit("wolf")
        add = self.ledger.add
        add(asset="wolf", stage="concept", candidate=0, task_id="wolf0", hash="h", status="success", credits=20, created=1)
        add(asset="wolf", stage="concept", candidate=1, task_id="wolf1", hash="h", status="running", credits=20, created=1)
        add(asset="bear", stage="concept", candidate=0, task_id="bear0", hash="h", status="success", credits=20, created=1)
        add(asset="wolf", stage="model", task_id="wolfmodel", hash="h", status="success", credits=40, created=1)
        for choice, says in (("1", "no finished concept candidate 1"), ("nope", "no such task"), ("bear0", "a concept of bear, not a concept of wolf"),
                             ("wolfmodel", "a model of wolf"), ("wolf1", "running")):
            with self.assertRaises(tripo.TripoError, msg=choice) as e:
                self.quietly(tripo.cmd_pick, self.ledger, wolf, choice)
            self.assertIn(says, str(e.exception))
        self.assertIsNone(self.ledger.picked("wolf"))
        self.quietly(tripo.cmd_pick, self.ledger, wolf, "0")
        self.restart()
        self.assertEqual(self.ledger.picked("wolf")["task_id"], "wolf0")

    def test_plan_counts_what_each_stage_still_costs(self):
        wolf = unit("wolf")  # picked, its model in flight: nothing to pay
        self.picked_concept(wolf)
        job = tripo.model_job(wolf, tripo.effective_pick(wolf, self.ledger), STYLE, self.ledger)
        self.ledger.add(asset="wolf", stage="model", task_id="m", hash=job.req.hash, status="running", credits=40, created=10**12)
        hall = tripo.Asset("hall", "buildings", "", "a hall")  # nothing yet: 2 concepts, pick, model
        stump = tripo.Asset("pine_stump", "nodes", "", "a stump", route="text")  # text model
        house = tripo.Asset("house", "buildings", "", "a house")  # made
        house.target().parent.mkdir(parents=True)
        house.target().write_bytes(b"glb")
        total = self.quietly(tripo.cmd_plan, [wolf, hall, stump, house], STYLE, self.ledger, None, bodies=True)
        self.assertEqual(total, (40 + 40) + 20)
        self.assertIn("per stage: model 40, concept 40, text_model 20", self.out)
        self.assertIn("model paid (running)", self.out)
        self.assertRegex(self.out, r"house +done \(art/static/source/buildings/house.glb\)")
        self.assertIn('"aspect_ratio": "1:1"', self.out, "--bodies prints the requests")

    def test_the_real_request_and_style_files_parse_and_cost_what_the_readme_says(self):
        self.patch("ROOT", REAL_ROOT)
        self.patch("ART", REAL_ROOT / "art" / "tripo")
        self.patch("MESHY", REAL_ROOT / "art" / "meshy")
        style = tripo.load_style()
        assets = tripo.load_assets(style=style)
        batches = {}
        for a in assets:
            batches.setdefault(a.batch, []).append(a.key)
        self.assertEqual({b: len(k) for b, k in batches.items()}, {"P0": 25, "P1": 27, "P2": 14, "meshy-finish": 11})
        for key in ("mountain_peak_d", "mountain_ridge_c", "mountain_foothill_c", "crater_rim_c", "star_core_b", "aether_monolith",
                    "impact_scar", "meteor_shard_b", "reeds_c", "river_rocks_c", "pine_stump", "oak_stump", "palm_stump", "sea_cliff_c",
                    "sea_stack_b", "beach_rocks", "bramble_c", "oak_d", "oak_e", "pine_d", "birch_b", "willow_b", "mossy_boulder_c",
                    "shrub_b", "fern_b", "lily_pads", "standing_stones_b", "ruined_watchtower", "wayside_shrine", "fallen_log_b",
                    "driftwood", "shipwreck", "stone_bridge", "wooden_footbridge", "backdrop_mountains_b", "backdrop_forest_b"):
            self.assertTrue(any(a.key == key for a in assets), key)
        for a in assets:
            if a.route == "text":
                self.assertEqual(a.target().parent.name, "nodes")
                self.assertTrue(a.key.endswith("_stump"))
                tripo.text_request(a, style)
            else:
                tripo.concept_request(a, style, 0)  # fits, and its references exist
            tripo.model_request(a, style, {"source": "tripo", "task_id": "t", "candidate": 0, "created": 0}, now=0)
            tripo.text_prompt(a, style, trim=True)
            if a.kind == "decor":
                self.assertEqual(a.target().parent, tripo.SOURCE / "decor")
        ledger = tripo.Ledger(self.dir / "empty.json")
        costs = {}
        for batch in ("meshy-finish", "P0", "P1", "P2"):
            costs[batch] = self.quietly(tripo.cmd_plan, tripo.load_assets(batch=batch, style=style), style, ledger, None)
        self.assertEqual(costs, {"meshy-finish": 440, "P0": 2000, "P1": 1980, "P2": 1120})


class StudioTests(TripoTest):
    def setUp(self):
        super().setUp()
        tasks = [{"asset": k, "stage": "concept", "candidate": 0, "task_id": f"m-{k}", "status": "SUCCEEDED", "picked": True} for k in ("skiff", "hawk")]
        self.meshy({"skiff": {"polycount": 20000}, "hawk": {}}, tasks,
                   [("skiff", "concept_0_m-skiff.png", b"skiff png"), ("hawk", "concept_0_m-hawk.png", b"hawk png")])
        self.requests([{"id": "peak", "kind": "decor", "prompt": "a peak", "batch": "P9", "face_limit": 20000}])
        self.batch = tripo.load_assets(batch="meshy-finish", style=STYLE)
        self.kit = tripo.STUDIO / "meshy-finish"

    def drop(self, folder, name, data=b"glTF\x02\x00\x00\x00 a model"):
        (self.kit / folder / name).write_bytes(data)

    def importing(self, assets=None):
        return self.quietly(tripo.cmd_import, "meshy-finish", assets or self.batch, self.ledger)

    def test_kit_writes_a_folder_per_model_with_its_image_prompt_and_settings(self):
        self.quietly(tripo.cmd_kit, "meshy-finish", self.batch, STYLE, self.ledger)
        self.assertEqual(sorted(p.name for p in self.kit.iterdir()), ["01_skiff", "02_hawk", "CHECKLIST.md"])
        folder = self.kit / "01_skiff"
        self.assertEqual(sorted(p.name for p in folder.iterdir()), ["concept.png", "settings.txt"], "a picked concept: nothing to paste")
        self.assertEqual((folder / "concept.png").read_bytes(), b"skiff png")
        settings = (folder / "settings.txt").read_text(encoding="utf-8")
        for says in ("Image to 3D: upload concept.png", "v3.1-20260211", "HD (detailed), not 8K", "PBR: off", "Face limit / polycount: 20000",
                     "55 credits", "Export (free): format GLB, texture resolution 2K", "art/units/source/skiff.glb",
                     "import-studio --batch meshy-finish"):
            self.assertIn(says, settings)
        checklist = (self.kit / "CHECKLIST.md").read_text(encoding="utf-8")
        self.assertIn("- [ ] `01_skiff`: image to 3D from concept.png, face limit 20000", checklist)
        self.assertIn("- [ ] `02_hawk`: image to 3D from concept.png, face limit 30000", checklist)
        self.assertIn("costs 55 Studio credits (65 with 8K textures, not needed): about 110 credits here", checklist)
        self.assertIn("drop the file into that folder (any file name, exactly one .glb per folder)", checklist)
        # No concept: Studio's free image generator first (with the style
        # references beside the kit), or text to 3D; a text filler: text to 3D.
        (self.dir / "refs").mkdir()
        (self.dir / "refs" / "rock.png").write_bytes(b"rock render")
        style = {**STYLE, "references": {"decor": ["refs/rock.png"]}}
        self.requests([{"id": "peak", "kind": "decor", "prompt": "a peak", "batch": "P9", "face_limit": 20000},
                       {"id": "pine_stump", "kind": "nodes", "prompt": "a stump", "batch": "P9", "route": "text"}], style)
        peak, stump = tripo.load_assets(batch="P9", style=style)
        self.quietly(tripo.cmd_kit, "P9", [peak, stump], style, self.ledger)
        kit = tripo.STUDIO / "P9"
        self.assertEqual((kit / "_references" / "decor" / "rock.png").read_bytes(), b"rock render")
        folder = kit / "01_peak"
        self.assertEqual((folder / "prompt.txt").read_text(encoding="utf-8").strip(), tripo.concept_prompt(peak, style, False))
        self.assertEqual((folder / "text_prompt.txt").read_text(encoding="utf-8").strip(), tripo.text_prompt(peak, style, trim=True))
        settings = (folder / "settings.txt").read_text(encoding="utf-8")
        for says in ("Studio's image generator (Nano Banana Pro; free", "../_references/decor/", tripo.STYLE_ONLY, "art/static/source/decor/peak.glb"):
            self.assertIn(says, settings)
        folder = kit / "02_pine_stump"
        self.assertEqual((folder / "prompt.txt").read_text(encoding="utf-8").strip(), tripo.text_prompt(stump, style, trim=True))
        self.assertEqual((folder / "image_prompt.txt").read_text(encoding="utf-8").strip(), tripo.concept_prompt(stump, style, False))
        self.assertIn("Text to 3D: paste prompt.txt", (folder / "settings.txt").read_text(encoding="utf-8"))
        checklist = (kit / "CHECKLIST.md").read_text(encoding="utf-8")
        self.assertIn("- [ ] `01_peak`: image generator from prompt.txt (free), pick one, image to 3D, face limit 20000", checklist)
        self.assertIn("about 55 credits here, plus 1 text-to-3D models at Studio's price", checklist)
        # The references folder is not an asset folder.
        (folder / "stump.glb").write_bytes(b"glTF stump")
        self.quietly(tripo.cmd_import, "P9", [peak, stump], self.ledger)
        self.assertTrue(stump.target().exists())

    def test_import_installs_dropped_models_refuses_ambiguous_folders_and_lists_what_is_missing(self):
        self.quietly(tripo.cmd_kit, "meshy-finish", self.batch, STYLE, self.ledger)
        skiff, hawk = self.batch
        self.drop("01_skiff", "tripo_export (1).GLB")
        self.drop("02_hawk", "a.glb")
        self.drop("02_hawk", "b.glb", b"glTF other")
        with self.assertRaises(tripo.TripoError) as e:
            self.importing()
        self.assertIn("02_hawk: 2 GLB files (a.glb, b.glb); leave only the one to install", str(e.exception))
        self.assertIn("1 imported, 0 already imported; 1 still to make: hawk", self.out)
        self.assertEqual(skiff.target().read_bytes(), b"glTF\x02\x00\x00\x00 a model")
        self.assertEqual(self.rel(skiff.target()), "art/units/source/skiff.glb")
        entry = self.stage("studio", "skiff")[0]
        self.assertEqual((entry["source"], entry["credits"], entry["status"], entry["installed"], entry["file"]),
                         ("studio", 0, "success", "art/units/source/skiff.glb", "tripo_export (1).GLB"))
        self.assertFalse(hawk.target().exists())
        # Not a GLB: refused.
        (self.kit / "02_hawk" / "b.glb").unlink()
        (self.kit / "02_hawk" / "a.glb").write_bytes(b"<html>not a model</html>")
        with self.assertRaises(tripo.TripoError) as e:
            self.importing()
        self.assertIn("not a GLB file", str(e.exception))
        # Fixed: imported; the skiff is not imported twice.
        self.drop("02_hawk", "a.glb")
        self.importing()
        self.assertIn("1 imported, 1 already imported; 0 still to make", self.out)
        self.assertEqual(len(self.stage("studio")), 2)
        # The API never makes (or pays for) a Studio model, and the kit lists it as done.
        api = FakeTripo()
        self.models(api, 100, self.batch)
        self.assertEqual(api.calls, [])
        self.assertIn("skiff: made in Tripo Studio", self.out)
        self.quietly(tripo.cmd_kit, "meshy-finish", self.batch, STYLE, self.ledger)
        checklist = (self.kit / "CHECKLIST.md").read_text(encoding="utf-8")
        self.assertIn("- [x] `01_skiff`: done (art/units/source/skiff.glb)", checklist)
        self.assertTrue((self.kit / "01_skiff" / "tripo_export (1).GLB").exists(), "dropped files are never touched")
        # A deleted install comes back from the same file, recorded once.
        skiff.target().unlink()
        self.importing()
        self.assertTrue(skiff.target().exists())
        self.assertEqual(len(self.stage("studio")), 2)

    def test_import_refuses_strays_and_needs_a_kit(self):
        with self.assertRaises(tripo.TripoError) as e:
            self.importing()
        self.assertIn("run `tripo.py kit --batch meshy-finish` first", str(e.exception))
        self.quietly(tripo.cmd_kit, "meshy-finish", self.batch, STYLE, self.ledger)
        (self.kit / "03_dragon").mkdir()
        with self.assertRaises(tripo.TripoError) as e:
            self.importing()
        self.assertIn("03_dragon: names no asset of batch meshy-finish", str(e.exception))
        self.assertIn("0 imported", self.out)

    def test_kit_and_import_from_the_command_line(self):
        self.patch("api_key", lambda required=True: (_ for _ in ()).throw(AssertionError("the key was read")))
        with contextlib.redirect_stdout(io.StringIO()) as out:
            self.assertEqual(tripo.main(["kit", "--batch", "meshy-finish"]), 0)
            self.drop("02_hawk", "hawk.glb")
            self.assertEqual(tripo.main(["import-studio", "--batch", "meshy-finish"]), 0)
        self.assertIn("1 imported, 0 already imported; 1 still to make: skiff", out.getvalue())
        self.assertTrue(self.batch[1].target().exists())


if __name__ == "__main__":
    unittest.main()
