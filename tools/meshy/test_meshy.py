"""Tests for meshy.py against a fake API (no network, no credits).

    python tools/meshy/test_meshy.py
"""

import base64
import contextlib
import dataclasses
import io
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import meshy  # noqa: E402

REAL_ROOT = meshy.ROOT

STYLE = {
    "ai_model": "meshy-6",
    "texture_resolution": "2k",
    "enable_pbr": False,
    "remove_lighting": True,
    "queue_limit": 3,
    "polycount": {"nodes": 8000, "buildings": 30000},
    "prompt_suffix": {"nodes": "nature style", "buildings": "village style"},
    "texture_suffix": {"nodes": "natural", "buildings": "hand-made"},
}

# The concept-first style.json: today's keys plus the new sections.
NEW_STYLE = {
    **STYLE,
    "polycount": {"nodes": 8000, "buildings": 30000, "units": 12000, "props": 3000},
    "concept": {"ai_model": "nano-banana-pro", "candidates": 2, "aspect": {"character": "3:4", "object": "1:1", "prop": "1:1"}},
    "model": {"ai_model": "meshy-6", "texture_resolution": "2k", "remove_lighting": True, "enable_pbr": False},
    "framing": {"character": "One full-body character standing in an A-pose", "object": "One object on flat ground", "prop": "One hand-held prop"},
    "look": "Painterly game art",
    "ages": {str(i): f"Age {i} technology" for i in range(1, 6)},
    "factions": {"celts": "Celtic tribe colours: green and ochre", "norse": "Norse raiders"},
    "magic": {"celts": "Glowing green spirit runes"},
    "kinds": {"units": "A soldier of the ancient world", "props": "A hand-held item", "buildings": "A village building", "nodes": "A natural resource"},
}

MODES = {meshy.TEXT_TO_IMAGE: "text-to-image", meshy.IMAGE_TO_IMAGE: "image-to-image", meshy.IMAGE_TO_3D: "image-to-3d", meshy.RIGGING: "rigging"}
CREDITS = {"preview": 20, "refine": 10, "text-to-image": 9, "image-to-image": 9, "image-to-3d": 30, "rigging": 5}
SIGNED = "?Expires=1&Signature=s&Key-Pair-Id=k"


class Crash(Exception):
    """The process dies mid-run (Ctrl+C, a power cut)."""


class FakeApi:
    """A Meshy with a task queue: a task finishes after `polls` status
    checks; creating one while `queue` are in flight is refused like the
    real API (429 NoMorePendingTasks). `busy` tasks from elsewhere fill the
    queue until they end. Tasks of the `fail` modes fail (refunded).
    `animations`: True, "skin-only" (no armature-only GLBs) or False (no
    basic_animations). `crash` = (method, path prefix): the next such call
    dies instead, once. Task ids are unique within a test, like Meshy's:
    the first fake's are task1, task2..., a second fake's api2-task1..."""

    made = 0  # fakes made in this test

    def __init__(self, queue=3, polls=2, busy=0, fail=(), animations=True):
        FakeApi.made += 1
        self.prefix = "task" if FakeApi.made == 1 else f"api{FakeApi.made}-task"
        self.queue, self.polls = queue, polls
        self.calls, self.tasks, self.n = [], {}, 0
        self.created = []  # modes of the tasks actually accepted
        self.bodies = []  # (mode, body) of the tasks actually accepted
        self.max_in_flight = 0
        self.fail, self.animations, self.crash = set(fail), animations, None
        for i in range(busy):
            self.tasks[f"other{i}"] = {"mode": "preview", "left": polls}

    def in_flight(self):
        return sum(1 for t in self.tasks.values() if t["left"] > 0)

    @staticmethod
    def mode(path, body=None):
        base = path if body is not None else path.rsplit("/", 1)[0]
        if base == meshy.TEXT_TO_3D:
            return body["mode"] if body else "preview"
        return MODES[base]

    def check_input(self, body, modes):
        """Like Meshy: the input task must be a finished task of the right kind."""
        t = self.tasks.get(body["input_task_id"])
        if t is not None and (t["mode"] not in modes or t["left"] > 0):
            raise meshy.MeshyError(f"HTTP 400: {body['input_task_id']} is not a finished {' or '.join(modes)} task")

    def __call__(self, method, path, body):
        if self.crash and method == self.crash[0] and path.startswith(self.crash[1]):
            self.crash = None
            raise Crash(f"{method} {path}")
        self.calls.append((method, path, body))
        # Tasks from elsewhere progress on their own as time passes.
        for other in [k for k in self.tasks if k.startswith("other")]:
            self.tasks[other]["left"] = max(0, self.tasks[other]["left"] - 1)
        if method == "POST":
            if self.in_flight() >= self.queue:
                raise meshy.QueueFull("queue full")
            mode = self.mode(path, body)
            if mode == "image-to-3d":
                self.check_input(body, ("text-to-image", "image-to-image"))
            if mode == "rigging":
                self.check_input(body, ("image-to-3d",))
            self.n += 1
            task_id = f"{self.prefix}{self.n}"
            self.tasks[task_id] = {"mode": mode, "left": self.polls}
            self.created.append(mode)
            self.bodies.append((mode, body))
            self.max_in_flight = max(self.max_in_flight, self.in_flight())
            return {"result": task_id}
        task_id = path.rsplit("/", 1)[-1]
        t = self.tasks.setdefault(task_id, {"mode": self.mode(path), "left": 0})
        t["left"] = max(0, t["left"] - 1)
        if t["left"] > 0:
            return {"status": "IN_PROGRESS", "progress": 50}
        return self.result(task_id, t["mode"])

    def result(self, task_id, mode):
        if mode in self.fail:
            return {"status": "FAILED", "progress": 0, "consumed_credits": 0, "task_error": {"message": "no luck"}}
        done = {"status": "SUCCEEDED", "progress": 100, "consumed_credits": CREDITS[mode]}
        if mode in ("preview", "refine"):
            return {**done, "model_urls": {"glb": "fake://model"}, "thumbnail_url": "fake://thumb" + SIGNED}
        done["task_error"] = {"message": ""}
        if mode in ("text-to-image", "image-to-image"):
            return {**done, "image_urls": [f"fake://{task_id}/image.png{SIGNED}"]}
        if mode == "image-to-3d":
            return {**done, "model_urls": {"glb": f"fake://{task_id}/model.glb{SIGNED}"}, "thumbnail_url": f"fake://{task_id}/preview.png{SIGNED}"}
        rig = {"rigged_character_glb_url": f"fake://{task_id}/rigged.glb{SIGNED}", "rigged_character_fbx_url": f"fake://{task_id}/rigged.fbx{SIGNED}"}
        if self.animations:
            basic = {}
            for name in ("walking", "running"):
                basic[f"{name}_glb_url"] = f"fake://{task_id}/{name}.glb{SIGNED}"
                basic[f"{name}_fbx_url"] = f"fake://{task_id}/{name}.fbx{SIGNED}"
                if self.animations != "skin-only":
                    basic[f"{name}_armature_glb_url"] = f"fake://{task_id}/{name}_armature.glb{SIGNED}"
            rig["basic_animations"] = basic
        return {**done, "result": rig}

    def posts(self):
        """Tasks created, in order (refused POSTs are not tasks)."""
        return list(self.created)

    def body(self, mode, n=0):
        return [b for m, b in self.bodies if m == mode][n]


def fake_download(url, to):
    """A download whose file says which link it came from."""
    to.parent.mkdir(parents=True, exist_ok=True)
    to.write_bytes(url.split("?", 1)[0].encode())


class MeshyTest(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)
        self.patch("OUT", self.dir / "out")
        self.patch("SOURCE", self.dir / "source")
        self.patch("UNITS", self.dir / "units")
        self.patch("ROOT", self.dir)
        self.patch("download", fake_download)
        self.ledger = meshy.Ledger(self.dir / "ledger.json")
        self.assets = [meshy.Asset("pine", "nodes", v, f"a pine {v}") for v in "abcde"] + [meshy.Asset("house", "buildings", "", "a house")]
        self.sleeps = 0
        self.out = ""
        FakeApi.made = 0

    def nosleep(self, seconds):
        """No waiting, but a run that keeps polling fails instead of hanging."""
        self.sleeps += 1
        if self.sleeps > 200:
            raise AssertionError("run_jobs is still polling after 200 rounds: it would hang")

    def make(self, api, cap, assets=None):
        meshy.cmd_make(api, assets or self.assets, STYLE, self.ledger, meshy.Budget(cap), self.nosleep)

    def made_earlier(self, asset):
        """Ledger entries of a model an earlier run paid for and installed."""
        self.ledger.add(asset=asset.key, stage="preview", candidate=0, task_id="mesh1", hash=meshy.preview_hash(asset, STYLE), status="SUCCEEDED", credits=20, created=10**9)
        self.ledger.add(asset=asset.key, stage="refine", task_id="tex1", preview="mesh1", hash=meshy.refine_hash(asset, STYLE, "mesh1"), status="SUCCEEDED", credits=10, created=10**9)

    def use_requests(self, spec, style=STYLE):
        """Point meshy at a requests.json (and style.json) of our own."""
        (self.dir / "requests.json").write_text(json.dumps(spec), encoding="utf-8")
        (self.dir / "style.json").write_text(json.dumps(style), encoding="utf-8")
        self.patch("ART", self.dir)

    # -- the concept-first pipeline --------------------------------------

    def unit(self, name="spearman", **fields):
        return meshy.Asset(name, "units", "", f"a {name}", **fields)

    def quietly(self, run, *args):
        """Run a command, keeping what it prints in self.out."""
        out = io.StringIO()
        try:
            with contextlib.redirect_stdout(out):
                return run(*args)
        finally:
            self.out += out.getvalue()

    def concepts(self, api, cap, assets, candidates=None, named=None, style=NEW_STYLE):
        self.quietly(meshy.cmd_concepts, api, assets, style, self.ledger, meshy.Budget(cap), candidates, named, self.nosleep)

    def models(self, api, cap, assets, style=NEW_STYLE):
        self.quietly(meshy.cmd_models, api, assets, style, self.ledger, meshy.Budget(cap), self.nosleep)

    def pick(self, asset, choice):
        return self.quietly(meshy.cmd_pick, self.ledger, asset, choice)

    def picked_concept(self, asset):
        """A finished concept of the asset, picked, as an earlier run left it."""
        task_id = f"{asset.key}_concept"
        self.ledger.add(asset=asset.key, stage="concept", candidate=0, task_id=task_id, api="text-to-image", hash="h", status="SUCCEEDED", credits=9, created=10**9)
        self.ledger.pick(task_id)
        return task_id

    def restart(self):
        """A new process: it only knows the ledger saved on disk."""
        self.ledger = meshy.Ledger(self.ledger.path)

    def stage(self, name, asset=None):
        return [t for t in self.ledger.tasks if t["stage"] == name and (asset is None or t["asset"] == asset.key)]

    def patch(self, name, value):
        self.addCleanup(setattr, meshy, name, getattr(meshy, name))
        setattr(meshy, name, value)

    def test_make_installs_everything_within_the_queue(self):
        api = FakeApi(queue=3)
        self.make(api, 6 * 30)
        for a in self.assets:
            self.assertTrue(a.target().exists(), a.key)
        self.assertLessEqual(api.max_in_flight, 3, "never more tasks in flight than the queue allows")
        self.assertEqual(api.posts().count("preview"), 6)
        self.assertEqual(api.posts().count("refine"), 6)
        self.assertEqual(self.ledger.spent(), 180)
        thumbs = [t["thumbnail"] for t in self.ledger.tasks if t.get("thumbnail")]
        self.assertTrue(thumbs and all("?" not in u for u in thumbs), "the committed ledger keeps no signed links")
        # A second run makes nothing new.
        again = FakeApi()
        self.make(again, 180)
        self.assertEqual(again.posts(), [])

    def test_textures_go_before_new_meshes(self):
        api = FakeApi(queue=1, polls=1)
        self.make(api, 60, self.assets[:2])
        self.assertEqual(api.posts(), ["preview", "refine", "preview", "refine"])

    def test_the_credit_cap_never_starts_what_it_cannot_finish(self):
        api = FakeApi()
        with self.assertRaises(meshy.MeshyError) as e:
            self.make(api, 100)
        # 100 credits finish three models (30 each); the rest never start.
        self.assertEqual(api.posts().count("preview"), 3)
        self.assertEqual(api.posts().count("refine"), 3)
        self.assertIn("not started", str(e.exception))
        self.assertEqual(sum(a.target().exists() for a in self.assets), 3)

    def test_an_interrupted_run_is_resumed_not_paid_again(self):
        asset = self.assets[0]
        self.ledger.add(asset=asset.key, stage="preview", candidate=0, task_id="old", hash=meshy.preview_hash(asset, STYLE), status="IN_PROGRESS", credits=20, created=10**12)
        api = FakeApi()
        self.make(api, 10, [asset])
        self.assertEqual(api.posts(), ["refine"], "the recorded mesh is polled, only the texture is paid")
        self.assertIn(("GET", f"{meshy.TEXT_TO_3D}/old", None), api.calls)
        self.assertTrue(asset.target().exists())

    def test_a_queue_full_of_other_tasks_is_waited_out(self):
        api = FakeApi(queue=3, busy=3)
        self.make(api, 60, self.assets[:2])
        self.assertTrue(all(a.target().exists() for a in self.assets[:2]))

    def test_a_made_model_whose_file_is_gone_is_reinstalled_from_its_download(self):
        asset = self.assets[0]
        self.made_earlier(asset)
        raw = meshy.OUT / asset.key / "tex1.glb"
        raw.parent.mkdir(parents=True)
        raw.write_bytes(b"made earlier")
        api = FakeApi()
        self.make(api, 0, [asset])
        self.assertEqual(api.calls, [], "no API call at all, nothing paid")
        self.assertEqual(asset.target().read_bytes(), b"made earlier")
        self.assertEqual(self.ledger.get("tex1")["installed"], asset.target().relative_to(meshy.ROOT).as_posix())

    def test_a_made_model_with_no_download_left_is_fetched_again(self):
        asset = self.assets[0]
        self.made_earlier(asset)
        api = FakeApi()
        self.make(api, 0, [asset])
        self.assertEqual(api.posts(), [])
        self.assertEqual(api.calls, [("GET", f"{meshy.TEXT_TO_3D}/tex1", None)])
        self.assertTrue(asset.target().exists())
        self.assertTrue((meshy.OUT / asset.key / "tex1.glb").exists())
        self.assertEqual(self.ledger.get("tex1")["installed"], asset.target().relative_to(meshy.ROOT).as_posix())

    def test_a_made_model_that_cannot_be_fetched_again_is_an_error_naming_its_task(self):
        asset = self.assets[0]
        self.made_earlier(asset)

        def expired_link(url, to):
            raise meshy.MeshyError(f"downloading {to.name} failed (HTTP Error 403: Forbidden)")

        def no_glb(method, path, body):
            return {"status": "SUCCEEDED", "model_urls": {}}

        for http, fetch in ((no_glb, meshy.download), (FakeApi(), expired_link)):
            meshy.download = fetch
            with self.assertRaises(meshy.MeshyError) as e:
                self.make(http, 0, [asset])
            self.assertIn("texture task tex1", str(e.exception))
            self.assertFalse(asset.target().exists())
        self.assertEqual(self.ledger.spent(), 30, "nothing new was paid")

    def test_a_state_nothing_handles_fails_instead_of_hanging(self):
        # A mesh in a status meshy.py does not know: nothing can start and
        # nothing is in flight to wait for.
        asset = self.assets[0]
        self.ledger.add(asset=asset.key, stage="preview", candidate=0, task_id="odd", hash=meshy.preview_hash(asset, STYLE), status="ON_HOLD", credits=20, created=10**9)
        api = FakeApi()
        with self.assertRaises(meshy.MeshyError) as e:
            self.make(api, 10, [asset])
        self.assertIn("mesh ON_HOLD (task odd)", str(e.exception))
        self.assertEqual(api.calls, [])
        self.assertEqual(self.sleeps, 0, "it fails in the first round")
        # A queue that can never hold a task is a config error, said as one.
        with self.assertRaises(meshy.MeshyError) as e:
            meshy.run_jobs(api, self.ledger, STYLE, [meshy.Job(self.assets[1])], 0, self.nosleep)
        self.assertIn("queue_limit is 0", str(e.exception))

    def test_a_job_that_timed_out_does_not_keep_the_run_alive(self):
        # Meshy never finishes "slow"; once it times out it is not polled
        # forever, so the stuck job beside it still fails loudly.
        a, b = self.assets[:2]
        self.ledger.add(asset=a.key, stage="preview", candidate=0, task_id="slow", hash=meshy.preview_hash(a, STYLE), status="IN_PROGRESS", credits=20, created=1)
        self.ledger.add(asset=b.key, stage="preview", candidate=0, task_id="odd", hash=meshy.preview_hash(b, STYLE), status="ON_HOLD", credits=20, created=1)

        def never_done(method, path, body):
            self.assertEqual(method, "GET", "nothing is started")
            return {"status": "IN_PROGRESS", "progress": 10}

        with self.assertRaises(meshy.MeshyError) as e:
            self.make(never_done, 20, [a, b])
        self.assertIn("mesh still IN_PROGRESS after 45 minutes (task slow", str(e.exception))
        self.assertIn("mesh ON_HOLD (task odd)", str(e.exception))

    def test_a_texture_made_from_another_candidate_is_the_model(self):
        # `refine` textured pine_a's candidate 2 (finished) and pine_b's
        # candidate 1 (still running); both installed files are gone.
        a, b = self.assets[:2]
        for asset, refined, status in ((a, 2, "SUCCEEDED"), (b, 1, "IN_PROGRESS")):
            for c in range(3):
                self.ledger.add(asset=asset.key, stage="preview", candidate=c, task_id=f"{asset.key}_mesh{c}", hash=meshy.preview_hash(asset, STYLE), status="SUCCEEDED", credits=20, created=10**9)
            mesh = f"{asset.key}_mesh{refined}"
            self.ledger.add(asset=asset.key, stage="refine", task_id=f"{asset.key}_tex", preview=mesh, hash=meshy.refine_hash(asset, STYLE, mesh), status=status, credits=10, created=10**12)
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(meshy.cmd_plan([a, b], STYLE, self.ledger, None), 0, "both are paid for")
        api = FakeApi()
        self.make(api, 0, [a, b])
        self.assertEqual(api.posts(), [], "candidate 0 is never textured on top")
        self.assertEqual({path for _, path, _ in api.calls}, {f"{meshy.TEXT_TO_3D}/pine_a_tex", f"{meshy.TEXT_TO_3D}/pine_b_tex"})
        for asset in (a, b):
            self.assertEqual(self.ledger.get(f"{asset.key}_tex")["installed"], asset.target().relative_to(meshy.ROOT).as_posix())
        self.assertEqual(len(self.ledger.tasks), 8, "no task was added")

    def test_a_texture_that_failed_to_download_is_installed_by_the_rerun(self):
        asset = self.assets[0]
        working = meshy.download

        def no_glb(url, to):
            if to.suffix == ".glb":
                raise meshy.MeshyError(f"downloading {to.name} failed (connection reset)")
            working(url, to)

        meshy.download = no_glb
        api = FakeApi()
        with self.assertRaises(meshy.MeshyError) as e:
            self.make(api, 30, [asset])
        self.assertIn("pine_a: texture task task2 finished but was not installed", str(e.exception))
        self.assertFalse(asset.target().exists())
        meshy.download = working
        self.make(api, 0, [asset])
        self.assertEqual(api.posts(), ["preview", "refine"], "the rerun pays nothing")
        self.assertTrue(asset.target().exists())

    def test_a_thumbnail_that_fails_is_only_a_warning(self):
        working = meshy.download

        def no_pictures(url, to):
            if to.suffix == ".png":
                raise meshy.MeshyError(f"downloading {to.name} failed (HTTP Error 404)")
            working(url, to)

        meshy.download = no_pictures
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.make(FakeApi(), 60, self.assets[:2])
        self.assertTrue(all(a.target().exists() for a in self.assets[:2]))
        self.assertIn("pine_a: no thumbnail", out.getvalue())

    def test_refine_only_takes_a_preview_of_the_same_asset(self):
        oak_a = meshy.Asset("oak", "nodes", "a", "an oak, tall")
        self.ledger.add(asset="oak_c", stage="preview", candidate=0, task_id="oak_c_mesh", hash="h", status="SUCCEEDED", credits=20, created=1)
        self.ledger.add(asset="oak_a", stage="refine", task_id="oak_a_tex", preview="x", hash="h", status="SUCCEEDED", credits=10, created=1)
        api = FakeApi()
        budget = meshy.Budget(100)
        for wrong, owner in (("oak_c_mesh", "a preview of oak_c"), ("oak_a_tex", "a refine of oak_a")):
            with self.assertRaises(meshy.MeshyError) as e:
                meshy.cmd_refine(api, oak_a, STYLE, self.ledger, budget, wrong, self.nosleep)
            self.assertIn(owner, str(e.exception))
        self.assertEqual(api.calls, [], "nothing posted, nothing polled")
        self.assertEqual(budget.used, 0)
        self.assertFalse(oak_a.target().exists())
        # Its own preview is textured and installed.
        self.ledger.add(asset="oak_a", stage="preview", candidate=0, task_id="oak_a_mesh", hash="h", status="SUCCEEDED", credits=20, created=1)
        meshy.cmd_refine(api, oak_a, STYLE, self.ledger, budget, "oak_a_mesh", self.nosleep)
        self.assertEqual(api.posts(), ["refine"])
        self.assertTrue(oak_a.target().exists())

    def test_refine_names_exactly_one_live_asset(self):
        self.use_requests({"assets": [
            {"id": "oak", "kind": "nodes", "prompt": "an oak", "variants": {"a": "tall", "b": "young", "c": "wide"}, "retired": ["c"]},
            {"id": "elm", "kind": "nodes", "prompt": "an elm", "variants": {"a": "tall", "c": "wide"}, "retired": ["c"]},
            {"id": "house", "kind": "buildings", "prompt": "a house"},
        ]})
        assets = meshy.load_assets()
        for name, key in (("oak_b", "oak_b"), ("house", "house"), ("elm", "elm_a")):
            self.assertEqual(meshy.refine_target(assets, name).key, key)
        for name in ("oak", "oak_c", "elm_c", "pine"):
            with self.assertRaises(meshy.MeshyError, msg=name):
                meshy.refine_target(assets, name)
        # The command refuses before the key is read or anything is called.
        def no_key(*args):
            raise AssertionError("the API key was read")
        self.patch("api_key", no_key)
        self.patch("make_http", no_key)
        self.patch("LEDGER", self.dir / "ledger.json")
        for name, says in (("oak", "oak_a, oak_b"), ("oak_c", "retired")):
            err = io.StringIO()
            with contextlib.redirect_stderr(err):
                self.assertEqual(meshy.main(["refine", name, "some-preview", "--max-credits", "10"]), 1)
            self.assertIn(says, err.getvalue())

    def test_prompts_get_the_style_and_respect_the_limit(self):
        body = meshy.preview_body(self.assets[-1], STYLE)
        self.assertEqual(body["prompt"], "a house. village style")
        self.assertEqual(body["target_polycount"], 30000)
        palm = meshy.Asset("palm", "nodes", "", "a palm", prompt_suffix="desert style")
        self.assertEqual(meshy.full_prompt(palm, STYLE), "a palm. desert style")
        with self.assertRaises(meshy.MeshyError):
            meshy.full_prompt(meshy.Asset("x", "nodes", "", "y" * 800), STYLE)

    def test_retired_variants_are_never_made_again(self):
        spec = {"assets": [{"id": "oak", "kind": "nodes", "prompt": "an oak",
                            "variants": {"a": "tall", "c": "wide"}, "retired": ["c"]}]}
        self.use_requests(spec)
        self.assertEqual([a.key for a in meshy.load_assets()], ["oak_a"])
        # Only a list of existing variant names will do (a bare "c" is not one).
        for bad in (["x"], "c", [""], [3], None, {"c": True}):
            spec["assets"][0]["retired"] = bad
            self.use_requests(spec)
            with self.assertRaises(meshy.MeshyError, msg=repr(bad)):
                meshy.load_assets()

    def test_two_assets_never_share_a_name(self):
        # One name is one ledger history and one installed file; a retired
        # variant's name stays taken.
        for other in ("oak_a", "oak_c"):
            self.use_requests({"assets": [
                {"id": "oak", "kind": "nodes", "prompt": "an oak", "variants": {"a": "tall", "c": "wide"}, "retired": ["c"]},
                {"id": other, "kind": "nodes", "prompt": "another oak"},
            ]})
            with self.assertRaises(meshy.MeshyError, msg=other) as e:
                meshy.load_assets()
            self.assertIn(f"{other}: two assets", str(e.exception))

    def test_the_real_request_file_parses(self):
        self.patch("ROOT", REAL_ROOT)  # the real style references
        style = meshy.load_style()
        assets = meshy.load_assets(style=style)
        keys = [a.key for a in assets]
        self.assertIn("pine_a", keys)
        self.assertIn("town_center", keys)
        self.assertIn("queue_limit", style)
        for a in assets:
            if a.kind in meshy.TEXT_TO_3D_KINDS and a.batch is None:
                meshy.full_prompt(a, style)  # batch 1's text to 3D prompts fit in 800 characters
            if "concept" in style:
                meshy.concept_request(a, style, 0)  # fits, and its references exist
                meshy.model_body(a, style, "concept")

    # -- concepts --------------------------------------------------------

    def test_concepts_make_candidates_and_download_them(self):
        spearman = self.unit(rig=True, faction="celts", age=1)
        hall = meshy.Asset("hall", "buildings", "", "a great hall")
        spear = meshy.Asset("spear", "props", "", "a long spear")
        api = FakeApi(queue=3)
        self.concepts(api, 54, [spearman, hall, spear])
        self.assertEqual(api.posts(), ["text-to-image"] * 6, "two candidates each, one image per task")
        self.assertLessEqual(api.max_in_flight, 3)
        concepts = self.stage("concept")
        self.assertEqual([(t["asset"], t["candidate"]) for t in concepts], [(a.key, n) for a in (spearman, hall, spear) for n in (0, 1)])
        for t in concepts:
            self.assertEqual((t["status"], t["api"], t["credits"]), ("SUCCEEDED", "text-to-image", 9))
            image = meshy.OUT / t["asset"] / f"concept_{t['candidate']}_{t['task_id']}.png"
            self.assertEqual(image.read_bytes(), f"fake://{t['task_id']}/image.png".encode())
            self.assertEqual(t["image"], f"fake://{t['task_id']}/image.png", "no signed link in the ledger")
        # A character is drawn in an A-pose at 3:4; objects and props square.
        character, building, prop = (api.body("text-to-image", n) for n in (0, 2, 4))
        self.assertEqual(character, {"ai_model": "nano-banana-pro", "prompt": meshy.concept_prompt(spearman, NEW_STYLE, False), "aspect_ratio": "3:4", "remove_background": True, "pose_mode": "a-pose"})
        self.assertEqual((building["aspect_ratio"], prop["aspect_ratio"]), ("1:1", "1:1"))
        self.assertNotIn("pose_mode", building)
        self.assertNotIn("pose_mode", prop)
        self.assertEqual(self.ledger.spent(), 54)
        # Again: nothing new. A third candidate is only the third.
        self.concepts(api, 0, [spearman, hall, spear])
        self.assertEqual(len(api.posts()), 6)
        self.concepts(api, 27, [spearman, hall, spear], candidates=3)
        self.assertEqual(len(api.posts()), 9)
        self.assertEqual(sorted(t["candidate"] for t in self.stage("concept", hall)), [0, 1, 2])

    def test_concepts_leave_picked_and_made_assets_alone_unless_named(self):
        picked = self.unit("wolf")
        self.picked_concept(picked)
        made = meshy.Asset("house", "buildings", "", "a house")
        made.target().parent.mkdir(parents=True)
        made.target().write_bytes(b"made by make")
        api = FakeApi()
        self.concepts(api, 100, [picked, made])
        self.assertEqual(api.posts(), [])
        self.concepts(api, 100, [picked, made], named={"wolf"})
        self.assertEqual(api.posts(), ["text-to-image"] * 2)
        self.assertEqual(self.ledger.picked("wolf")["task_id"], "wolf_concept", "new candidates do not change the pick")

    def test_concept_prompts_layer_the_style(self):
        knight = self.unit("knight", faction="celts", magic=True, age=2, rig=True)
        self.assertEqual(meshy.concept_prompt(knight, NEW_STYLE, False),
                         "One full-body character standing in an A-pose. Subject: a knight. Celtic tribe colours: green and ochre. "
                         "Glowing green spirit runes. Age 2 technology. A soldier of the ancient world. Painterly game art.")
        self.assertTrue(meshy.concept_prompt(knight, NEW_STYLE, True).startswith(meshy.STYLE_ONLY + " One full-body"))
        # An asset's prompt_suffix replaces its kind's text, as for text to 3D.
        palm = meshy.Asset("palm", "nodes", "a", "a date palm, straight trunk", prompt_suffix="Desert oasis")
        self.assertEqual(meshy.concept_prompt(palm, NEW_STYLE, False), "One object on flat ground. Subject: a date palm, straight trunk. Desert oasis. Painterly game art.")
        # Each layer is part of the concept's hash.
        plain = meshy.concept_request(knight, NEW_STYLE, 0).hash
        for changed in (dataclasses.replace(knight, magic=False), dataclasses.replace(knight, age=3), dataclasses.replace(knight, faction="norse", magic=False)):
            self.assertNotEqual(meshy.concept_request(changed, NEW_STYLE, 0).hash, plain)
        self.assertNotEqual(meshy.concept_request(knight, NEW_STYLE, 1).hash, plain, "and the candidate number")
        self.assertNotEqual(meshy.concept_request(knight, {**NEW_STYLE, "look": "Pixel art"}, 0).hash, plain)
        with self.assertRaises(meshy.MeshyError) as e:
            meshy.concept_prompt(meshy.Asset("giant", "units", "", "x" * 1800), NEW_STYLE, False)
        self.assertIn("max 1800", str(e.exception))

    def test_references_go_as_data_uris_and_at_most_five(self):
        (self.dir / "refs").mkdir()
        files = []
        for i, ext in enumerate(("png", "jpg")):
            (self.dir / "refs" / f"r{i}.{ext}").write_bytes(f"image {i}".encode())
            files.append(f"refs/r{i}.{ext}")
        style = {**NEW_STYLE, "references": {"units": files}}
        spearman = self.unit(rig=True)
        api = FakeApi()
        self.concepts(api, 9, [spearman], candidates=1, style=style)
        self.assertEqual(api.posts(), ["image-to-image"])
        body = api.body("image-to-image")
        self.assertEqual(body["reference_image_urls"], ["data:image/png;base64," + base64.b64encode(b"image 0").decode(),
                                                        "data:image/jpeg;base64," + base64.b64encode(b"image 1").decode()])
        self.assertNotIn("pose_mode", body, "not documented for image to image")
        self.assertTrue(body["prompt"].startswith(meshy.STYLE_ONLY))
        entry = self.stage("concept")[0]
        self.assertEqual((entry["api"], entry["references"]), ("image-to-image", files))
        self.assertNotIn("base64", self.ledger.path.read_text(encoding="utf-8"), "the ledger keeps paths, not images")
        # A reference whose bytes change is a new concept.
        (self.dir / "refs" / "r0.png").write_bytes(b"another image 0")
        self.assertNotEqual(meshy.concept_request(spearman, style, 0).hash, entry["hash"])
        # Six references, or a missing one, are refused before anything is sent.
        for refs, says in ((files * 3, "at most 5"), (["refs/gone.png"], "does not exist"), (["refs/r0.gif"], "is not a")):
            api = FakeApi()
            with self.assertRaises(meshy.MeshyError, msg=says) as e:
                self.concepts(api, 100, [spearman], style={**NEW_STYLE, "references": {"units": refs}})
            self.assertIn(says, str(e.exception))
            self.assertEqual(api.calls, [])

    # -- pick and models -------------------------------------------------

    def test_pick_then_models_only_for_picked_assets(self):
        wolf = self.unit("wolf")
        hall = meshy.Asset("hall", "buildings", "", "a great hall")
        api = FakeApi()
        self.concepts(api, 36, [wolf, hall])
        first, second = self.stage("concept", wolf)
        self.pick(wolf, "1")
        self.assertTrue(self.ledger.get(second["task_id"])["picked"])
        self.models(api, 30, [wolf, hall])
        self.assertEqual(api.posts().count("image-to-3d"), 1)
        self.assertIn("no concept picked (skipped; meshy.py pick <key> <n>): hall", self.out)
        self.assertEqual(api.body("image-to-3d"), {
            "input_task_id": second["task_id"], "ai_model": "meshy-6", "should_texture": True, "enable_pbr": False,
            "texture_resolution": "2k", "remove_lighting": True, "should_remesh": True, "topology": "triangle",
            "target_polycount": 12000, "target_formats": ["glb"], "image_enhancement": True,
        })
        self.assertTrue(wolf.target().exists())
        self.assertFalse(hall.target().exists())
        model = self.stage("model", wolf)[0]
        self.assertEqual((model["concept"], model["installed"], model["thumbnail"]), (second["task_id"], "units/wolf.glb", f"fake://{model['task_id']}/preview.png"))
        # Again: nothing. A new pick (by task id) is made, and is the only pick.
        self.models(api, 0, [wolf, hall])
        self.assertEqual(api.posts().count("image-to-3d"), 1)
        self.pick(wolf, first["task_id"])
        self.assertEqual([t["task_id"] for t in self.stage("concept") if t.get("picked")], [first["task_id"]])
        self.models(api, 30, [wolf])
        self.assertEqual(api.body("image-to-3d", 1)["input_task_id"], first["task_id"])
        self.assertEqual(wolf.target().read_bytes(), f"fake://{self.stage('model', wolf)[1]['task_id']}/model.glb".encode())
        # A polycount of its own wins over the kind's.
        self.assertEqual(meshy.model_body(dataclasses.replace(wolf, polycount=20000), NEW_STYLE, "c")["target_polycount"], 20000)

    def test_a_picked_asset_made_outside_meshy_is_left_alone(self):
        wolf, bear = self.unit("wolf"), self.unit("bear")
        self.picked_concept(wolf)
        self.picked_concept(bear)
        wolf.target().parent.mkdir(parents=True, exist_ok=True)
        wolf.target().write_bytes(b"made in another tool")
        api = FakeApi()
        self.models(api, 30, [wolf, bear])
        self.assertEqual(api.posts().count("image-to-3d"), 1)
        self.assertEqual(api.body("image-to-3d")["input_task_id"], "bear_concept")
        self.assertIn("wolf: already made outside Meshy (units/wolf.glb); left alone", self.out)
        self.assertEqual(wolf.target().read_bytes(), b"made in another tool")
        self.out = ""
        self.quietly(meshy.cmd_plan, [wolf], NEW_STYLE, self.ledger, None)
        self.assertRegex(self.out, r"wolf +done \(units/wolf.glb, made outside Meshy\)")

    def test_pick_takes_only_a_finished_concept_of_the_asset(self):
        wolf = self.unit("wolf")
        add = self.ledger.add
        add(asset="wolf", stage="concept", candidate=0, task_id="wolf0", hash="h", status="SUCCEEDED", credits=9, created=1)
        add(asset="wolf", stage="concept", candidate=1, task_id="wolf1", hash="h", status="IN_PROGRESS", credits=9, created=1)
        add(asset="bear", stage="concept", candidate=0, task_id="bear0", hash="h", status="SUCCEEDED", credits=9, created=1)
        add(asset="wolf", stage="model", task_id="wolfmodel", concept="wolf0", hash="h", status="SUCCEEDED", credits=30, created=1)
        for choice, says in (("1", "no finished concept candidate 1"), ("7", "no finished concept candidate 7"), ("nope", "no such task"),
                             ("bear0", "a concept of bear, not a concept of wolf"), ("wolfmodel", "a model of wolf"), ("wolf1", "IN_PROGRESS")):
            with self.assertRaises(meshy.MeshyError, msg=choice) as e:
                self.pick(wolf, choice)
            self.assertIn(says, str(e.exception))
        self.assertIsNone(self.ledger.picked("wolf"))
        # From the command line: no key is read, nothing is called.
        def no_key(*args):
            raise AssertionError("the API key was read")
        self.patch("api_key", no_key)
        self.patch("make_http", no_key)
        self.patch("LEDGER", self.ledger.path)
        self.use_requests({"assets": [{"id": "wolf", "kind": "units", "prompt": "a wolf"}]}, NEW_STYLE)
        err = io.StringIO()
        with contextlib.redirect_stderr(err), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(meshy.main(["pick", "wolf", "1"]), 1)
            self.assertEqual(meshy.main(["pick", "wolf", "0"]), 0)
        self.assertIn("no finished concept candidate 1", err.getvalue())
        self.restart()
        self.assertEqual(self.ledger.picked("wolf")["task_id"], "wolf0")

    def test_rig_only_rigged_units_and_install_each_kind_in_its_place(self):
        spearman = self.unit(rig=True, height_m=1.8)
        wolf = self.unit("wolf")
        spear = meshy.Asset("spear", "props", "", "a long spear")
        hall = meshy.Asset("hall", "buildings", "", "a great hall")
        pine = meshy.Asset("pine", "nodes", "a", "a pine, tall")
        assets = [spearman, wolf, spear, hall, pine]
        for a in assets:
            self.picked_concept(a)
        api = FakeApi(queue=3)
        self.models(api, 5 * 30 + 5, assets)
        self.assertEqual(sorted(api.posts()), ["image-to-3d"] * 5 + ["rigging"])
        self.assertLessEqual(api.max_in_flight, 3)
        spear_model = self.stage("model", spearman)[0]
        self.assertEqual(api.body("rigging"), {"input_task_id": spear_model["task_id"], "height_meters": 1.8})
        self.assertEqual(api.body("image-to-3d", 0)["pose_mode"], "a-pose", "a character is modelled in its A-pose")
        self.assertNotIn("pose_mode", api.body("image-to-3d", 1))
        for _, body in api.bodies:
            self.assertNotIn("texture_prompt", body)
        for a, path in ((spearman, "units/spearman.glb"), (wolf, "units/wolf.glb"), (spear, "units/props/spear.glb"),
                        (hall, "source/buildings/hall.glb"), (pine, "source/nodes/pine_a.glb")):
            self.assertEqual(a.target(), self.dir / path)
            self.assertTrue(a.target().exists(), path)
        # The rigged unit is its rigged GLB, with Meshy's armature-only
        # animations beside it; the others are their image-to-3D GLBs.
        rig = self.stage("rig")[0]
        self.assertEqual((rig["asset"], rig["model"], rig["height_m"], rig["installed"]), ("spearman", spear_model["task_id"], 1.8, "units/spearman.glb"))
        self.assertEqual(spearman.target().read_bytes(), f"fake://{rig['task_id']}/rigged.glb".encode())
        self.assertEqual(rig["animations"], {name: {"from": f"{name}_armature", "installed": f"units/spearman_{name}.glb"} for name in ("walking", "running")})
        for name in ("walking", "running"):
            self.assertEqual((self.dir / "units" / f"spearman_{name}.glb").read_bytes(), f"fake://{rig['task_id']}/{name}_armature.glb".encode())
        self.assertNotIn("installed", spear_model, "the bare model of a rigged unit is not installed")
        self.assertTrue((meshy.OUT / "spearman" / f"model_{spear_model['task_id']}.glb").exists(), "but kept to look at")
        wolf_model = self.stage("model", wolf)[0]
        self.assertEqual(wolf.target().read_bytes(), f"fake://{wolf_model['task_id']}/model.glb".encode())
        self.assertEqual([t["installed"] for t in self.stage("model") if "installed" in t], ["units/wolf.glb", "units/props/spear.glb", "source/buildings/hall.glb", "source/nodes/pine_a.glb"])
        self.assertNotIn("Signature", self.ledger.path.read_text(encoding="utf-8"))
        self.assertEqual(self.ledger.spent(), 5 * 9 + 5 * 30 + 5)

    def test_rig_animations_fall_back_to_the_skinned_ones_or_are_only_a_warning(self):
        a, b = self.unit("a", rig=True), self.unit("b", rig=True)
        for asset in (a, b):
            self.picked_concept(asset)
        self.models(FakeApi(animations="skin-only"), 35, [a])
        rig = self.stage("rig", a)[0]
        self.assertIn("no armature-only walking GLB; installing the with-skin one", self.out)
        self.assertEqual(rig["animations"]["walking"], {"from": "walking", "installed": "units/a_walking.glb"})
        self.assertEqual((self.dir / "units" / "a_walking.glb").read_bytes(), f"fake://{rig['task_id']}/walking.glb".encode())
        self.models(FakeApi(animations=False), 35, [b])
        self.assertIn("b: the rig came with no basic animations", self.out)
        self.assertTrue(b.target().exists())
        self.assertFalse((self.dir / "units" / "b_walking.glb").exists())
        self.assertEqual(self.stage("rig", b)[0]["animations"], {})

    def test_rig_tasks_start_before_models_and_concepts(self):
        a, b = self.unit("a", rig=True), self.unit("b", rig=True)
        for asset in (a, b):
            self.picked_concept(asset)
        api = FakeApi(queue=1, polls=1)
        self.models(api, 70, [a, b])
        self.assertEqual(api.posts(), ["image-to-3d", "rigging", "image-to-3d", "rigging"])

    # -- resume, reinstall, budget, queue --------------------------------

    def test_concepts_resume_after_a_crash_without_paying_twice(self):
        assets = [self.unit(rig=True), meshy.Asset("hall", "buildings", "", "a great hall")]
        api = FakeApi(queue=3)
        api.crash = ("GET", meshy.TEXT_TO_IMAGE)
        with self.assertRaises(Crash):
            self.concepts(api, 36, assets)
        self.assertEqual(api.posts(), ["text-to-image"] * 3)
        self.restart()
        self.assertEqual([t["status"] for t in self.stage("concept")], ["PENDING"] * 3, "each task was saved as it started")
        self.concepts(api, 9, assets)  # only the candidate that never started is paid
        self.assertEqual(api.posts(), ["text-to-image"] * 4)
        self.assertEqual(len(self.stage("concept")), 4)
        for t in self.stage("concept"):
            self.assertTrue((meshy.OUT / t["asset"] / f"concept_{t['candidate']}_{t['task_id']}.png").exists())
        # An image whose download is gone is fetched again, not paid again.
        image = meshy.OUT / "hall" / f"concept_1_{self.stage('concept', assets[1])[1]['task_id']}.png"
        image.unlink()
        self.concepts(FakeApi(), 0, assets)
        self.assertTrue(image.exists())

    def test_models_resume_after_a_crash_without_paying_twice(self):
        spearman, wolf = self.unit(rig=True), self.unit("wolf")
        for a in (spearman, wolf):
            self.picked_concept(a)
        api = FakeApi(queue=3)
        api.crash = ("GET", meshy.IMAGE_TO_3D)
        with self.assertRaises(Crash):
            self.models(api, 65, [spearman, wolf])
        self.assertEqual(api.posts(), ["image-to-3d"] * 2)
        self.restart()
        self.models(api, 5, [spearman, wolf])  # only the rig is left to pay
        self.assertEqual(api.posts(), ["image-to-3d", "image-to-3d", "rigging"])
        self.assertTrue(spearman.target().exists() and wolf.target().exists())
        self.assertEqual(len(self.ledger.tasks), 2 + 3)

    def test_rigs_resume_after_a_crash_without_paying_twice(self):
        spearman = self.unit(rig=True)
        self.picked_concept(spearman)
        api = FakeApi()
        api.crash = ("GET", meshy.RIGGING)
        with self.assertRaises(Crash):
            self.models(api, 35, [spearman])
        self.assertEqual(api.posts(), ["image-to-3d", "rigging"])
        self.restart()
        self.models(api, 0, [spearman])
        self.assertEqual(api.posts(), ["image-to-3d", "rigging"])
        self.assertTrue((self.dir / "units" / "spearman_running.glb").exists())
        # Dying while the finished rig's files download: the rerun fetches
        # the rig again and installs it, paying nothing.
        other = self.unit("other", rig=True)
        self.picked_concept(other)

        def dies_on_animations(url, to):
            if "running_armature" in url:
                raise Crash(url)
            fake_download(url, to)

        meshy.download = dies_on_animations
        with self.assertRaises(Crash):
            self.models(api, 35, [other])
        meshy.download = fake_download
        self.restart()
        self.assertEqual(self.stage("rig", other)[0]["status"], "SUCCEEDED")
        self.assertFalse(other.target().exists())
        self.models(api, 0, [other])
        self.assertEqual(api.posts().count("rigging"), 2)
        self.assertTrue((self.dir / "units" / "other_running.glb").exists())

    def test_made_models_are_reinstalled_without_paying(self):
        spearman, wolf = self.unit(rig=True), self.unit("wolf")
        for a in (spearman, wolf):
            self.picked_concept(a)
        self.models(FakeApi(), 65, [spearman, wolf])
        files = [spearman.target(), wolf.target(), self.dir / "units" / "spearman_walking.glb", self.dir / "units" / "spearman_running.glb"]
        made = {f: f.read_bytes() for f in files}
        # From art/meshy/out/: no API call at all.
        for f in files:
            f.unlink()
        api = FakeApi()
        self.models(api, 0, [spearman, wolf])
        self.assertEqual(api.calls, [])
        self.assertEqual({f: f.read_bytes() for f in files}, made)
        # Only an animation gone: that is not made either.
        files[2].unlink()
        self.models(api, 0, [spearman, wolf])
        self.assertEqual(api.calls, [])
        self.assertTrue(files[2].exists())
        # Downloads gone too: fetched again, still nothing paid.
        for f in files:
            f.unlink()
        shutil.rmtree(meshy.OUT)
        self.models(api, 0, [spearman, wolf])
        self.assertEqual(api.posts(), [])
        self.assertEqual({path.rsplit("/", 1)[0] for _, path, _ in api.calls}, {meshy.RIGGING, meshy.IMAGE_TO_3D})
        self.assertEqual({f: f.read_bytes() for f in files}, made)
        self.assertIn("reinstalled units/spearman.glb from rig task", self.out)

    def test_the_credit_cap_never_starts_what_it_cannot_finish_in_new_stages(self):
        assets = [self.unit(rig=True), self.unit("wolf"), meshy.Asset("hall", "buildings", "", "a great hall")]
        api = FakeApi()
        with self.assertRaises(meshy.MeshyError) as e:
            self.concepts(api, 30, assets)
        self.assertEqual(api.posts(), ["text-to-image"] * 3, "30 credits are three 9-credit concepts")
        self.assertIn("not started (over --max-credits 30): wolf#1, hall#0, hall#1", str(e.exception))
        # A rigged unit is model and rig: 35, reserved before its model starts.
        spearman, wolf = assets[:2]
        for a in (spearman, wolf):
            self.picked_concept(a)
        api = FakeApi()
        with self.assertRaises(meshy.MeshyError) as e:
            self.models(api, 34, [spearman])
        self.assertEqual(api.calls, [])
        self.assertIn("not started (over --max-credits 34): spearman", str(e.exception))
        with self.assertRaises(meshy.MeshyError):
            self.models(api, 60, [spearman, wolf])
        self.assertEqual(api.posts(), ["image-to-3d", "rigging"], "the first fits; the second would overspend")

    def test_a_full_queue_is_waited_out_on_the_new_endpoints(self):
        assets = [self.unit(rig=True), self.unit("wolf"), meshy.Asset("spear", "props", "", "a spear")]
        api = FakeApi(queue=2, busy=2)
        self.concepts(api, 54, assets)
        self.assertEqual(len(self.stage("concept")), 6)
        self.assertLessEqual(api.max_in_flight, 2)
        for a in assets:
            self.pick(a, "0")
        api = FakeApi(queue=2, busy=2)
        self.models(api, 95, assets)
        self.assertTrue(all(a.target().exists() for a in assets))
        self.assertLessEqual(api.max_in_flight, 2)
        self.assertIn("the account's task queue is full", self.out)

    def test_a_failed_task_is_refunded_and_made_again(self):
        wolf = self.unit("wolf")
        self.picked_concept(wolf)
        with self.assertRaises(meshy.MeshyError) as e:
            self.models(FakeApi(fail={"image-to-3d"}), 30, [wolf])
        self.assertIn("wolf: model FAILED (no luck)", str(e.exception))
        failed = self.stage("model")[0]
        self.assertEqual((failed["status"], failed["credits"], failed["error"]), ("FAILED", 0, "no luck"))
        api = FakeApi()
        self.models(api, 30, [wolf])
        self.assertEqual(api.posts(), ["image-to-3d"])
        self.assertTrue(wolf.target().exists())

    # -- schema, plan, text to 3D -----------------------------------------

    def test_requests_are_checked_against_the_schema(self):
        good = {"id": "spearman", "kind": "units", "prompt": "a spearman"}
        for change, says in (
            ({"kind": "heroes"}, "kind must be one of"), ({"framing": "portrait"}, "framing must be one of"),
            ({"faction": "romans"}, "faction 'romans' is not in style.json factions"), ({"faction": 3}, "is not in style.json factions"),
            ({"age": 0}, "age must be"), ({"age": 6}, "age must be"), ({"age": "2"}, "age must be"), ({"age": True}, "age must be"),
            ({"magic": True}, "magic needs a faction"), ({"faction": "norse", "magic": True}, "faction norse has no text in style.json magic"),
            ({"magic": "yes"}, "magic must be true or false"), ({"rig": "yes"}, "rig must be true or false"),
            ({"kind": "buildings", "rig": True}, "only units can be rigged"), ({"rig": True, "framing": "object"}, "needs framing character"),
            ({"height_m": 0}, "height_m"), ({"height_m": "tall"}, "height_m"), ({"polycount": 50}, "polycount"), ({"polycount": 1.5}, "polycount"),
            ({"batch": 2.5}, "batch"), ({"rigg": True}, "unknown field rigg"), ({"prompt": ""}, "prompt"), ({"variants": ["a"]}, "variants"),
        ):
            self.use_requests({"assets": [{**good, **change}]}, NEW_STYLE)
            with self.assertRaises(meshy.MeshyError, msg=repr(change)) as e:
                meshy.load_assets()
            self.assertIn(says, str(e.exception))
            self.assertIn("spearman", str(e.exception))
        self.use_requests({"assets": [
            good,
            {**good, "id": "knight", "faction": "celts", "magic": True, "age": 3, "rig": True, "height_m": 1.9, "batch": "2", "polycount": 20000},
            {"id": "spear", "kind": "props", "prompt": "a spear", "batch": 2, "_note": "comments are fine"},
        ]}, NEW_STYLE)
        spearman, knight, spear = meshy.load_assets()
        self.assertEqual((spearman.framing, knight.framing, spear.framing), ("object", "character", "prop"))
        self.assertEqual((spearman.height_m, knight.height_m, knight.polycount, knight.magic), (1.7, 1.9, 20000, True))
        self.assertEqual([a.key for a in meshy.load_assets(batch="2")], ["knight", "spear"])
        self.assertEqual([a.key for a in meshy.load_assets({"knight", "spearman"}, batch="2")], ["knight"])
        # The style's new sections are checked when they are needed.
        for broken, says in (
            ({"concept": None}, "no concept section"), ({"concept": {**NEW_STYLE["concept"], "ai_model": "dall-e"}}, "does not know its price"),
            ({"concept": {**NEW_STYLE["concept"], "aspect": {"character": "5:4", "object": "1:1", "prop": "1:1"}}}, "concept.aspect"),
            ({"framing": {"character": "x"}}, "style.json framing"), ({"model": {**NEW_STYLE["model"], "ai_model": "meshy-4"}}, "model.ai_model"),
            ({"polycount": {"nodes": 1}}, "polycount has no units"),
        ):
            with self.assertRaises(meshy.MeshyError, msg=says) as e:
                meshy.preflight("concepts", [spearman], {**NEW_STYLE, **broken})
                meshy.preflight("models", [spearman], {**NEW_STYLE, **broken})
            self.assertIn(says, str(e.exception))

    def test_plan_counts_what_each_stage_still_costs(self):
        spearman = self.unit(rig=True)  # nothing yet: concepts, pick, model, rig
        wolf = self.unit("wolf")  # picked, its model in flight: nothing to pay
        self.picked_concept(wolf)
        self.ledger.add(asset="wolf", stage="model", task_id="m", concept="wolf_concept", hash=meshy.model_hash(wolf, NEW_STYLE, "wolf_concept"), status="IN_PROGRESS", credits=30, created=10**12)
        hall = meshy.Asset("hall", "buildings", "", "a great hall")  # one concept made
        self.concepts(FakeApi(), 9, [hall], candidates=1)
        house = meshy.Asset("house", "buildings", "", "a house")  # made
        house.target().parent.mkdir(parents=True)
        house.target().write_bytes(b"glb")
        self.out = ""
        total = self.quietly(meshy.cmd_plan, [spearman, wolf, hall, house], NEW_STYLE, self.ledger, None)
        self.assertEqual(total, (18 + 30 + 5) + 0 + (9 + 30))
        self.assertIn("per stage: concept 27, model 60, rig 5", self.out)
        self.assertIn("2 assets have no pick yet (concepts, then meshy.py pick <key> <n>): spearman, hall", self.out)
        self.assertRegex(self.out, r"house +done \(source/buildings/house.glb\)")
        self.assertIn("model paid", self.out)

    def test_text_to_3d_leaves_units_and_picked_assets_alone(self):
        spear = meshy.Asset("spear", "props", "", "a spear")
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(meshy.text_to_3d_assets([spear, self.assets[0]]), [self.assets[0]])
        hall = meshy.Asset("hall", "buildings", "", "a great hall")
        self.picked_concept(hall)
        api = FakeApi()
        self.quietly(self.make, api, 30, [hall])
        self.assertEqual(api.calls, [])
        self.assertIn("hall: has a picked concept: `models` makes it", self.out)


if __name__ == "__main__":
    unittest.main()
