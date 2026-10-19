"""Generate game models with the Meshy API, in one consistent style.

Assets to make are listed in art/meshy/requests.json; the shared style (model
version, settings, the style text appended to every prompt) is
art/meshy/style.json. Finished models land in art/static/source/<kind>/,
ready for the static pipeline (docs/art/static-models.md).

    python tools/meshy/meshy.py balance
    python tools/meshy/meshy.py plan [--only pine,house]
    python tools/meshy/meshy.py make --max-credits 400 [--only ...]
    python tools/meshy/meshy.py preview --candidates 3 --max-credits 200 --only house
    python tools/meshy/meshy.py refine house <preview-task-id> --max-credits 20
    python tools/meshy/meshy.py status

`make` makes (mesh + texture) and installs every model not made yet;
`preview` only makes candidate meshes (pick one from their thumbnails in
art/meshy/out/, then `refine` it: the preview must be one of that same
asset's). The account's task queue is respected (at most `queue_limit`
tasks in flight, from style.json), nothing is started beyond
--max-credits, and every task is recorded in art/meshy/ledger.json, so an
interrupted run resumes instead of paying twice, and a made model whose
installed file was deleted is installed again without paying (the latest
texture of the asset, whichever candidate `refine` picked).

An asset's optional "retired" list in requests.json names variants that
were made once and taken out of the game: they are never made, previewed
or refined again.

The API key comes from the MESHY_API_KEY environment variable or the
git-ignored .env file at the repo root. It is never printed.
"""

from __future__ import annotations

import argparse
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
API = "https://api.meshy.ai"
TEXT_TO_3D = "/openapi/v2/text-to-3d"

POLL_SECONDS = 8.0
TASK_TIMEOUT_SECONDS = 45 * 60
IN_FLIGHT = ("PENDING", "IN_PROGRESS")
FINAL_FAILURES = ("FAILED", "CANCELED", "EXPIRED")


# Credits per stage (docs.meshy.ai/en/api/pricing): Meshy-6 previews cost
# 20, other models 5; texturing 10 at 2k/4k and 15 at 8k.
def preview_cost(ai_model: str) -> int:
    return 20 if ai_model.startswith("meshy-6") or ai_model in ("latest", "meshy-7.1") else 5


def refine_cost(resolution: str) -> int:
    return 15 if resolution == "8k" else 10


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
    kind: str  # "nodes" | "buildings"
    variant: str  # "" for a single model
    prompt: str
    # Per-asset style overrides (e.g. palms are not a northern forest).
    prompt_suffix: str | None = None
    texture_suffix: str | None = None

    @property
    def key(self) -> str:
        return f"{self.id}_{self.variant}" if self.variant else self.id

    def target(self) -> Path:
        return SOURCE / self.kind / f"{self.key}.glb"


def load_style() -> dict:
    return json.loads((ART / "style.json").read_text(encoding="utf-8"))


def load_assets(only: set[str] | None = None) -> list[Asset]:
    spec = json.loads((ART / "requests.json").read_text(encoding="utf-8"))
    assets, names = [], set()
    for a in spec["assets"]:
        if a["kind"] not in ("nodes", "buildings"):
            raise MeshyError(f"{a['id']}: kind must be nodes or buildings")
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
            asset = Asset(a["id"], a["kind"], variant, prompt, a.get("prompt_suffix"), a.get("texture_suffix"))
            # A name is a ledger history and an installed file: two assets
            # sharing one would pay twice and overwrite each other.
            if asset.key in names:
                raise MeshyError(f"{asset.key}: two assets in requests.json have this name")
            names.add(asset.key)
            if variant in retired:  # made once, taken out of the game: never made again
                continue
            if only is None or a["id"] in only or asset.key in only:
                assets.append(asset)
    return assets


def refine_target(assets: list[Asset], name: str) -> Asset:
    """The one asset `refine <name>` means: the asset whose id_variant is
    `name`, or the only live variant of the id `name`. Retired variants are
    not in `assets`, so they are never refined."""
    matches = [a for a in assets if a.key == name] or [a for a in assets if a.id == name]
    if len(matches) > 1:
        raise MeshyError(f"{name} could be {', '.join(a.key for a in matches)}: name one")
    if not matches:
        raise MeshyError(f"{name}: no such asset in art/meshy/requests.json (retired variants are never refined)")
    return matches[0]


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
    return prompt_hash(full_prompt(asset, style), style["ai_model"], str(style["polycount"][asset.kind]))


def refine_hash(asset: Asset, style: dict, preview_id: str) -> str:
    return prompt_hash(preview_id, texture_suffix(asset, style), style["texture_resolution"], str(style["enable_pbr"]))


def preview_body(asset: Asset, style: dict) -> dict:
    return {
        "mode": "preview",
        "prompt": full_prompt(asset, style),
        "ai_model": style["ai_model"],
        "should_remesh": True,
        "topology": "triangle",
        "target_polycount": style["polycount"][asset.kind],
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
# The scheduler
# --------------------------------------------------------------------------

@dataclass
class Job:
    """One model being made: its preview (mesh) task, then its refine
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
    def finished(self) -> bool:
        return self.error is not None or self.done is not None or (not self.texture and self.preview is not None and self.preview["status"] == "SUCCEEDED")


def attach_existing(job: Job, ledger: Ledger, style: dict) -> None:
    """Pick up the tasks a previous run already started for this job. A
    model takes the latest texture started on any of its asset's current
    meshes (`refine` may have picked another candidate than this job's),
    so a model already paid for is never paid for again."""
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


def job_cost(job: Job, style: dict) -> int:
    """Credits this job still has to pay."""
    cost = 0 if job.preview else preview_cost(style["ai_model"])
    if job.texture and not job.refine:
        cost += refine_cost(style["texture_resolution"])
    return cost


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


def thumbnail(job: Job, result: dict, to: Path) -> None:
    """Download a finished task's thumbnail. It is only a picture, so one
    that fails is a warning, not a reason to stop a paid run."""
    if result.get("thumbnail_url"):
        try:
            download(result["thumbnail_url"], to)
        except MeshyError as e:
            print(f"  {job.label}: no thumbnail ({e})", flush=True)


def place(job: Job, raw: Path, ledger: Ledger) -> Path:
    """Copy the texture task's downloaded model to where the static
    pipeline reads it, and record that in the ledger."""
    target = job.asset.target()
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(raw, target)
    ledger.update(job.refine["task_id"], installed=target.relative_to(ROOT).as_posix())
    return target


def reinstall(job: Job, http: Http, ledger: Ledger) -> None:
    """Install a texture task that finished on an earlier run but whose
    installed file is gone (deleted or moved), without paying again: from
    its download in art/meshy/out/ if that is still there, else fetched
    again. A task that cannot be fetched any more is the job's error."""
    task_id = job.refine["task_id"]
    raw = OUT / job.asset.key / f"{task_id}.glb"
    try:
        if raw.exists():
            job.done = place(job, raw, ledger)
        else:
            result = http("GET", f"{TEXT_TO_3D}/{task_id}", None)
            if result.get("status") != "SUCCEEDED":
                raise MeshyError(f"it is now {result.get('status')}")
            job.done = install(job, result, ledger)
        print(f"{job.label}: reinstalled {job.done.relative_to(ROOT)} from texture task {task_id} (no new credits)", flush=True)
    except MeshyError as e:
        job.error = f"{job.label}: texture task {task_id} finished on an earlier run but cannot be installed again: {e}"


def describe(job: Job) -> str:
    """The job's tasks and their states, for error messages."""
    stages = (("mesh", job.preview), ("texture", job.refine)) if job.texture else (("mesh", job.preview),)
    return ", ".join(f"{stage} {t['status']} (task {t['task_id']})" if t else f"no {stage} task" for stage, t in stages)


def run_jobs(http: Http, ledger: Ledger, style: dict, jobs: list[Job], queue_limit: int, sleep: Callable[[float], None] = time.sleep) -> None:
    """Drive every job to the end, keeping at most `queue_limit` tasks in
    flight: textures for finished meshes go first, then new meshes. A
    round that can neither start, install nor wait for anything fails
    every unfinished job instead of polling forever. A finished job's
    tasks (one that timed out) are left alone: a rerun resumes them."""
    if queue_limit < 1:
        raise MeshyError(f"queue_limit is {queue_limit} in style.json: nothing could ever start")
    waiting = False
    while not all(j.finished for j in jobs):
        progressed = queue_full = False
        # 1. Textures an earlier run finished whose installed file is gone:
        # install them again from what was already paid for.
        for j in jobs:
            if not j.finished and j.texture and j.refine is not None and j.refine["status"] == "SUCCEEDED":
                reinstall(j, http, ledger)
                progressed = True
        in_flight = sum(1 for j in jobs if not j.finished for t in (j.preview, j.refine) if t is not None and t["status"] in IN_FLIGHT)
        # 2. Start what can start: textures for finished meshes first, then
        # new meshes. A full queue (tasks from elsewhere count too) ends
        # this round; the next round tries again.
        try:
            for j in jobs:
                if in_flight >= queue_limit:
                    break
                if not j.finished and j.texture and j.refine is None and j.preview is not None and j.preview["status"] == "SUCCEEDED":
                    r = http("POST", TEXT_TO_3D, refine_body(j.preview["task_id"], j.asset, style))
                    j.refine = ledger.add(asset=j.asset.key, stage="refine", task_id=r["result"], preview=j.preview["task_id"], hash=refine_hash(j.asset, style, j.preview["task_id"]), status="PENDING", credits=refine_cost(style["texture_resolution"]), created=int(time.time()))
                    in_flight += 1
                    progressed = True
                    print(f"  {j.label}: texture started", flush=True)
            for j in jobs:
                if in_flight >= queue_limit:
                    break
                if not j.finished and j.preview is None:
                    r = http("POST", TEXT_TO_3D, preview_body(j.asset, style))
                    j.preview = ledger.add(asset=j.asset.key, stage="preview", candidate=j.candidate, task_id=r["result"], hash=preview_hash(j.asset, style), status="PENDING", credits=preview_cost(style["ai_model"]), created=int(time.time()))
                    in_flight += 1
                    progressed = True
                    print(f"  {j.label}: mesh started", flush=True)
        except QueueFull:
            queue_full = True
            if not waiting:
                print("  the account's task queue is full: waiting for a task to finish", flush=True)
        waiting = queue_full
        # 3. Check everything in flight.
        for j in jobs:
            if j.finished:
                continue
            for stage, task in (("mesh", j.preview), ("texture", j.refine)):
                if task is None or task["status"] not in IN_FLIGHT:
                    continue
                progressed = True
                result = http("GET", f"{TEXT_TO_3D}/{task['task_id']}", None)
                status = result.get("status", task["status"])
                progress = result.get("progress", 0)
                if j.printed.get(stage) != (status, progress // 25):
                    j.printed[stage] = (status, progress // 25)
                    print(f"  {j.label} {stage}: {status} {progress}%", flush=True)
                if status in IN_FLIGHT:
                    if time.time() - task.get("created", time.time()) > TASK_TIMEOUT_SECONDS:
                        j.error = f"{j.label}: {stage} still {status} after {TASK_TIMEOUT_SECONDS // 60} minutes (task {task['task_id']}; rerun to resume)"
                    continue
                ledger.update(task["task_id"], status=status, credits=result.get("consumed_credits", task["credits"]), error=result.get("task_error"), thumbnail=unsigned(result.get("thumbnail_url")))
                if status != "SUCCEEDED":
                    j.error = f"{j.label}: {stage} {status} ({result.get('task_error')})"
                elif stage == "mesh":
                    thumbnail(j, result, OUT / j.asset.key / f"preview_{j.candidate}_{task['task_id']}.png")
                else:
                    try:
                        j.done = install(j, result, ledger)
                        print(f"{j.label}: installed {j.done.relative_to(ROOT)}", flush=True)
                    except MeshyError as e:  # paid for: a rerun installs it without paying again
                        j.error = f"{j.label}: texture task {task['task_id']} finished but was not installed ({e}); rerun to try again"
        # 4. Nothing started, installed or in flight, and no full queue to
        # wait out: no later round can differ, so fail loudly, not hang.
        if not progressed and not queue_full:
            for j in jobs:
                if not j.finished:
                    j.error = f"{j.label}: stuck with {describe(j)}; nothing to start or wait for (meshy.py does not handle this state)"
        if not all(j.finished for j in jobs):
            sleep(POLL_SECONDS)


def plan_jobs(assets: list[Asset], style: dict, ledger: Ledger, budget: Budget, texture: bool, candidates: int = 1) -> tuple[list[Job], list[str]]:
    """Jobs for the models not made yet, in order, as many as the budget can
    finish (each job's whole remaining cost is reserved up front)."""
    jobs, skipped = [], []
    for a in assets:
        if texture and a.target().exists():
            print(f"{a.key}: already made")
            continue
        for c in range(candidates):
            job = Job(a, candidate=c, texture=texture)
            attach_existing(job, ledger, style)
            cost = job_cost(job, style)
            if budget.can(cost):
                budget.take(cost, job.label)
                jobs.append(job)
            else:
                skipped.append(job.label)
    return jobs, skipped


# --------------------------------------------------------------------------
# Commands
# --------------------------------------------------------------------------

def cmd_balance(http: Http) -> None:
    b = http("GET", "/openapi/v1/balance", None)
    resets = b.get("monthly_resets_at")
    when = time.strftime("%Y-%m-%d", time.localtime(resets / 1000)) if resets else "never"
    print(f"balance {b['balance']} credits ({b.get('monthly_credits', 0)} monthly, reset {when}; {b.get('permanent_credits', 0)} permanent)")


def cmd_plan(assets: list[Asset], style: dict, ledger: Ledger, http: Http | None) -> int:
    total = 0
    print(f"model {style['ai_model']}, texture {style['texture_resolution']}, pbr {style['enable_pbr']}, queue {style.get('queue_limit', 10)}")
    for a in assets:
        if a.target().exists():
            print(f"  {a.key:<24} done ({a.target().relative_to(ROOT)})")
            continue
        job = Job(a)
        attach_existing(job, ledger, style)
        cost = job_cost(job, style)
        total += cost
        note = (" (already paid; make reinstalls it)" if job.refine and job.refine["status"] == "SUCCEEDED"
                else " (already paid; make resumes it)" if job.refine else " (mesh already paid)" if job.preview else "")
        print(f"  {a.key:<24} {cost:>3} credits  {a.kind:<9} {a.prompt[:60]}{note}")
    print(f"total {total} credits to finish {len(assets)} models (ledger so far: {ledger.spent()})")
    if http is not None:
        cmd_balance(http)
    return total


def report(jobs: list[Job], skipped: list[str], budget: Budget, ledger: Ledger) -> None:
    made = [j for j in jobs if j.done]
    meshes = [j for j in jobs if not j.texture and j.preview and j.preview["status"] == "SUCCEEDED"]
    print(f"{len(made)} models made, {len(meshes)} candidate meshes; {budget.used} credits reserved this run (ledger total {ledger.spent()})")
    problems = [j.error for j in jobs if j.error] + ([f"not started (over --max-credits {budget.cap}): {', '.join(skipped)}"] if skipped else [])
    if problems:
        raise MeshyError("; ".join(problems))


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


def cmd_status(ledger: Ledger) -> None:
    for t in ledger.tasks:
        print(f"{t['asset']:<24} {t['stage']:<8} {t['status']:<10} {t.get('credits', 0):>3}  {t['task_id']}  {t.get('installed', '')}")
    print(f"{len(ledger.tasks)} tasks, {ledger.spent()} credits")


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("balance")
    sub.add_parser("status")
    for name in ("plan", "make", "preview", "refine"):
        s = sub.add_parser(name)
        s.add_argument("--only", help="comma-separated asset ids or ids_variant")
        if name != "plan":
            s.add_argument("--max-credits", type=int, required=True)
        if name == "preview":
            s.add_argument("--candidates", type=int, default=2)
        if name == "refine":
            s.add_argument("asset", help="id or id_variant")
            s.add_argument("preview_task")
    args = p.parse_args(argv)
    try:
        ledger = Ledger(LEDGER)
        if args.cmd == "status":
            cmd_status(ledger)
            return 0
        if args.cmd != "balance":
            # Settle what to make before the key is read: a bad name costs nothing.
            style = load_style()
            if args.cmd == "refine":
                assets = [refine_target(load_assets(), args.asset)]
            else:
                assets = load_assets(set(args.only.split(",")) if args.only else None)
                if not assets:
                    raise MeshyError("no matching assets in art/meshy/requests.json")
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
        return 0
    except MeshyError as e:
        print(f"meshy: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
