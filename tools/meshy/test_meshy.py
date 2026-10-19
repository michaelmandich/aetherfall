"""Tests for meshy.py against a fake API (no network, no credits).

    python tools/meshy/test_meshy.py
"""

import contextlib
import io
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import meshy  # noqa: E402

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


class FakeApi:
    """A Meshy with a task queue: a task finishes after `polls` status
    checks; creating one while `queue` are in flight is refused like the
    real API (429 NoMorePendingTasks). `busy` tasks from elsewhere fill the
    queue until they end."""

    def __init__(self, queue=3, polls=2, busy=0):
        self.queue, self.polls = queue, polls
        self.calls, self.tasks, self.n = [], {}, 0
        self.created = []  # modes of the tasks actually accepted
        self.max_in_flight = 0
        for i in range(busy):
            self.tasks[f"other{i}"] = {"mode": "preview", "left": polls}

    def in_flight(self):
        return sum(1 for t in self.tasks.values() if t["left"] > 0)

    def __call__(self, method, path, body):
        self.calls.append((method, path, body))
        # Tasks from elsewhere progress on their own as time passes.
        for other in [k for k in self.tasks if k.startswith("other")]:
            self.tasks[other]["left"] = max(0, self.tasks[other]["left"] - 1)
        if method == "POST":
            if self.in_flight() >= self.queue:
                raise meshy.QueueFull("queue full")
            self.n += 1
            self.tasks[f"task{self.n}"] = {"mode": body["mode"], "left": self.polls}
            self.created.append(body["mode"])
            self.max_in_flight = max(self.max_in_flight, self.in_flight())
            return {"result": f"task{self.n}"}
        task_id = path.rsplit("/", 1)[-1]
        t = self.tasks.setdefault(task_id, {"mode": "preview", "left": 0})
        t["left"] = max(0, t["left"] - 1)
        if t["left"] > 0:
            return {"status": "IN_PROGRESS", "progress": 50}
        return {"status": "SUCCEEDED", "progress": 100, "consumed_credits": 20 if t["mode"] == "preview" else 10, "model_urls": {"glb": "fake://model"}, "thumbnail_url": "fake://thumb?Expires=1&Signature=s&Key-Pair-Id=k"}

    def posts(self):
        """Tasks created, in order (refused POSTs are not tasks)."""
        return list(self.created)


class MeshyTest(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir, ignore_errors=True)
        self.patch("OUT", self.dir / "out")
        self.patch("SOURCE", self.dir / "source")
        self.patch("ROOT", self.dir)
        self.patch("download", lambda url, to: (to.parent.mkdir(parents=True, exist_ok=True), to.write_bytes(b"glb")))
        self.ledger = meshy.Ledger(self.dir / "ledger.json")
        self.assets = [meshy.Asset("pine", "nodes", v, f"a pine {v}") for v in "abcde"] + [meshy.Asset("house", "buildings", "", "a house")]
        self.sleeps = 0

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

    def use_requests(self, spec):
        """Point meshy at a requests.json (and style.json) of our own."""
        (self.dir / "requests.json").write_text(json.dumps(spec), encoding="utf-8")
        (self.dir / "style.json").write_text(json.dumps(STYLE), encoding="utf-8")
        self.patch("ART", self.dir)

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
        assets = meshy.load_assets()
        keys = [a.key for a in assets]
        self.assertIn("pine_a", keys)
        self.assertIn("town_center", keys)
        style = meshy.load_style()
        self.assertIn("queue_limit", style)
        for a in assets:
            meshy.full_prompt(a, style)  # every prompt fits in 800 characters


if __name__ == "__main__":
    unittest.main()
