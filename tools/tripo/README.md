# tripo.py: game models from Tripo3D

`tools/tripo/tripo.py` makes the game's models with the Tripo3D API (v3), in
the style the Meshy models set. It took over from `tools/meshy/meshy.py` when
the Meshy credits ran out and keeps its guarantees:

- every paid task is written to the committed `art/tripo/ledger.json` the
  moment Tripo accepts it, keyed by a hash of exactly what was asked, so an
  interrupted run resumes instead of paying twice;
- every paid command takes `--max-credits`; an asset's whole remaining cost
  is reserved before its task starts, and nothing starts beyond the cap (or
  beyond the account balance, which is checked first, for free);
- a made model whose installed file was deleted is installed again without
  paying (from `art/tripo/out/`, or from Tripo with fresh links);
- retired variants are never made, and a Tripo asset can never take a Meshy
  asset's name (retired ones included, such as `oak_c`);
- the API key is never printed.

It also has a manual route through the Tripo Studio web app (`kit` and
`import-studio`), for credits bought as a Studio membership rather than API
credits.

## Setup

The key is `TRIPO_API_KEY`, from the environment or the git-ignored `.env` at
the repository root (`TRIPO_API_KEY=tsk_...`). It needs API credits, bought at
developers.tripo3d.ai; Studio membership credits are a different balance.
The tool talks to `https://openapi.tripo3d.ai/v3`: the docs' base URL
(`openapi.tripo3d.com`) is the China site and rejects international keys.
Python 3.10+, standard library only.

```bash
python tools/tripo/tripo.py balance        # free
python tools/tripo/tripo.py check          # free: the balance, and one upload (POST /files) to prove the key works
python tools/tripo/tripo.py usage          # free: the account's recent tasks, flagging any not in the ledger
python tools/tripo/tripo.py status         # the ledger (no key)
python tools/tripo/test_tripo.py           # the tests (no network, no credits)
```

## What it makes

| batch | what | route | installed to |
|---|---|---|---|
| `meshy-finish` | the 11 Meshy units whose concept was picked but never made into a model: dune_sandskiff, dune_sand_engine, ironhold_forgefather, starborn_orbital_lance, starborn_archon, steppe_skyrider, steppe_jet_lancer, steppe_war_ordu, sylvan_starstag_rider, sylvan_wisp_swarm, tide_bucentaur (all rigid) | their picked Meshy concept PNG is uploaded, then image to model | `art/units/source/<key>.glb`, where meshy.py would have put them |
| `P0` | mountain kit (peaks, ridges, foothills), meteor-site kit (crater rims, star cores, aether monolith, impact scar, meteor shards), river and lake dressing (reeds, river rocks): 25 models | concepts, pick, models | `art/static/source/decor/<key>.glb` |
| `P1` | stumps for pine, oak and palm; ocean edge (sea cliffs, sea stacks, beach rocks); brambles; tree variants oak_d, oak_e, pine_d; new species birch a-b and willow a-b; mossy boulders, shrubs (gorse, hazel), ferns; lily pads: 27 models | stumps from text (`make`); the rest concepts, pick, models | stumps and trees `art/static/source/nodes/`, the rest `.../decor/` |
| `P2` | standing stones, ruined watchtower, wayside shrine, fallen logs, driftwood, shipwreck, stone bridge, wooden footbridge, off-map backdrops of mountains and forest: 14 models | concepts, pick, models | `art/static/source/decor/` |

`meshy-finish` is not a list: it is every Meshy asset with a picked concept
(in `art/meshy/ledger.json`) and no successful Meshy model of that pick, so it
also catches any later pick. Its concept images are only in the main
checkout's git-ignored `art/meshy/out/`: run there, or pass `--meshy-out DIR`.
A rigged (humanoid) Meshy pick is skipped: tripo.py makes no rigs.

New assets go in `art/tripo/requests.json`: the `art/meshy/requests.json`
schema (id, kind, prompt, batch, age, faction, magic, framing, variants,
retired, prompt_suffix) plus the kind `decor`, `face_limit` (Tripo's
triangle limit; `polycount`, Meshy's name, also works) and `route` (`text`
for fillers made straight from text). The shared style is
`art/tripo/style.json`: Meshy's style texts and references word for word,
plus a `decor` kind, a `scenery` framing and a decor look that drop "resource
object" and "muted" for a lush, vibrant temperate European look while keeping
the style guide's "never" rules (no base plate, text, glow, people...).

## The commands, step by step

Every paid command takes `--max-credits N` (required), `--batch`, `--only
a,b` (ids or keys), `--concurrency N` (tasks in flight, default 100) and
`--dry-run` (prints every request body exactly as it would be posted and its
cost, and calls nothing). `plan` shows what is still to pay, per asset;
`plan --bodies` prints the request bodies too.

### 1. The eleven missing units: 440 credits

```bash
python tools/tripo/tripo.py plan --batch meshy-finish
python tools/tripo/tripo.py models --batch meshy-finish --max-credits 440
```

Each picked Meshy concept is uploaded (free) and made into a model (image to
model, 40 credits), then installed to `art/units/source/<key>.glb`. Then
build them like any rigid unit (`art/units/README.md`, "A rigid unit"). A
unit whose `art/units/source/<key>.glb` is already there (made in Studio,
say) is left alone: delete the file to make it here.

### 2. P0: concepts 1,000, models 1,000

```bash
python tools/tripo/tripo.py concepts --batch P0 --max-credits 1000
#   look at art/tripo/out/<key>/concept_<n>_<task>.png, then for each asset:
python tools/tripo/tripo.py pick mountain_peak_a 1     # a candidate number or a concept task id
python tools/tripo/tripo.py models --batch P0 --max-credits 1000
```

`concepts` makes 2 candidates per asset (`--candidates N` for more), each its
own task: image to image with the kind's style references (uploaded once
per run; they set the art style only), or text to image for a kind with
none. An asset already picked or made gets no new candidates unless `--only`
names it. `pick` needs no key. `models` makes every picked concept: the
concept's task id is passed straight to image to model while it is fresh
(an older one is uploaded from its download). An asset with no pick is
listed and skipped.

Try a pilot first if you like, e.g. `--only mountain_peak_a,mountain_peak_c,mountain_ridge_b,crater_rim_a`
(4 x 80 = 320 credits), and judge it in game before the rest.

### 3. P1: concepts 960, models 960, stumps 60

```bash
python tools/tripo/tripo.py make --batch P1 --max-credits 60          # the three stumps, from text
python tools/tripo/tripo.py concepts --batch P1 --max-credits 960
python tools/tripo/tripo.py pick <key> <n>                             # for each asset
python tools/tripo/tripo.py models --batch P1 --max-credits 960
```

### 4. P2: concepts 560, models 560

```bash
python tools/tripo/tripo.py concepts --batch P2 --max-credits 560
python tools/tripo/tripo.py pick <key> <n>
python tools/tripo/tripo.py models --batch P2 --max-credits 560
```

## What it costs

At the settings in `style.json`: concepts with `banana_pro` (the model Meshy
calls nano-banana-pro, which drew every Meshy concept), 2 per asset; models
with H3.1 (`v3.1-20260211`), detailed texture, no PBR; stumps from text with a
standard texture.

| batch | models | concept images | concepts | models | total (API credits) |
|---|---|---|---|---|---|
| meshy-finish | 11 | 0 (picked in Meshy) | 0 | 11 x 40 = 440 | **440** |
| P0 | 25 | 50 | 50 x 20 = 1,000 | 25 x 40 = 1,000 | **2,000** |
| P1 | 27 | 48 | 48 x 20 = 960 | 24 x 40 + 3 stumps x 20 = 1,020 | **1,980** |
| P2 | 14 | 28 | 28 x 20 = 560 | 14 x 40 = 560 | **1,120** |
| all | 77 | 126 | 2,520 | 3,020 | **5,540** |

Re-rolls are extra: a third candidate is 20 credits, a model remade from
another pick 40.

Where the prices come from:

- **Documented** (Tripo's v3.1 table): text to model 10 / 20 / 30, image to
  model 20 / 30 / 40 credits, untextured / standard texture / detailed
  texture. Upload, balance, usage and task queries are free.
- **Reserved high, unverified**: the docs give no price for an image; 20 is
  the `credits_consumed` of their text-to-image and image-to-image examples.
  Add-ons the docs only say "cost extra" are reserved as PBR +10, Ultra
  geometry +20, 8K ("extreme") texture +20; the chosen settings use none of
  them. tripo.py records what Tripo really charged (`credits_consumed`) in
  the ledger and warns after a run if a task cost more than was reserved
  (then raise the price in `tripo.py` so `--max-credits` holds).

## The Tripo Studio route (by hand)

For Studio membership credits (not API credits), `kit` writes one folder per
model still to make, and `import-studio` installs what you made:

```bash
python tools/tripo/tripo.py kit --batch meshy-finish    # or P0, P1, P2; --only works too
#   art/tripo/studio/<batch>/CHECKLIST.md lists the folders in order;
#   make each model in Studio and drop its GLB into its folder (any file name)
python tools/tripo/tripo.py import-studio --batch meshy-finish
```

Each `art/tripo/studio/<batch>/<NN>_<key>/` folder (git-ignored) has
`settings.txt` (what to start from and the Studio choices: H3.1, HD texture
not 8K, PBR off, its face limit, export GLB with 2K textures) and either:

- `concept.png`: its picked concept (the Meshy picks, or a Tripo pick):
  image to 3D from it;
- `prompt.txt`: the concept prompt for Studio's image generator (Nano Banana
  Pro, the same model; free with the membership, 4 images a prompt), then
  image to 3D from the best image; `text_prompt.txt` for text to 3D instead.
  The kind's style references are copied to `_references/<kind>/`;
- for a text filler (the stumps): `prompt.txt` for text to 3D.

`import-studio` installs the one `.glb` in each folder to the asset's place
(units `art/units/source/`, nodes and buildings `art/static/source/<kind>/`,
decor `art/static/source/decor/`), records it in the ledger as a Studio import
(0 API credits, source studio, the file's SHA-256), refuses a folder with two
GLBs or a file that is not a GLB, and says what is still missing. It can be
run again at any time; the API commands never remake (or pay for) a model
imported from Studio, and numbers and dropped files survive a new `kit`.

Studio costs (as the owner's Studio shows them): image to 3D with H3.1 is 55
Studio credits (65 with 8K textures, not needed); image generation and
exports are free; one export runs at a time (15-20 s).

| batch | models | Studio credits |
|---|---|---|
| meshy-finish | 11 | 605 |
| P0 | 25 | 1,375 |
| P1 | 27 | 1,320 + 3 stumps (text to 3D at Studio's price, or 165 by the image route) |
| P2 | 14 | 770 |

By hand, a model with a ready concept takes about 3 minutes of attention
(upload, settings, generate while working on the next, export, drop the
file): the eleven units are 30-45 minutes. A model that needs its concept
drawn first (generate 4 images, pick one) takes about 5 minutes: P0 is about
2 hours, P1 about 2 hours, P2 about an hour.

## How a run works

- **Concurrency.** At most `--concurrency` tasks in flight (default 100, the
  owner's limit; style.json `concurrency`), submitted by a pool of 16 worker
  threads as fast as Tripo's rate limits allow. HTTP 429 with code 1007 (rate
  limit) is waited out from `Retry-After`, else `X-RateLimit-Reset`, else 1,
  2, 4... 32 s, then retried (nothing was created); an endpoint whose window
  is used up (`X-RateLimit-Remaining: 0`) is held until it resets. Tripo has
  a concurrency pool per category (image generation, 3D generation); a full
  one (code 2000) makes tripo.py wait until one of its tasks there finishes,
  and never start more there than Tripo took. Models start before concepts
  when both wait.
- **Polling** goes through `POST /tasks/list`, 100 tasks a request, every 3 s;
  never one `GET` per task.
- **Downloads.** A finished task's files are downloaded at once (signed links
  expire), with 3 tries per link; when they all fail the task is queried again
  for fresh links (up to 3 times), and a task still not fetched is left for a
  rerun, which installs it without paying. Downloads stay in
  `art/tripo/out/<key>/` (git-ignored): concepts, model GLBs and Tripo's
  preview renders.
- **A lost answer.** A task-creating request whose answer is lost (a 5xx, a
  timeout, a dropped connection) is never sent again automatically: Tripo may
  have made, and charged for, the task. It is recorded as `uncertain` and
  blocks that exact request until you settle it: `tripo.py usage` lists the
  account's tasks and flags any not in the ledger; `tripo.py resolve <key>
  <task_id>` records the task it made (the next run fetches it without
  paying), `tripo.py resolve <key> none` says it made none (the next run makes
  it).
- **Out of credits.** Tripo's code 2010 stops every task not yet started; the
  ones in flight finish.
- **Failed tasks** are refunded by Tripo; the ledger records 0 for them and a
  rerun makes them again.

## Settings (style.json) and why

| setting | value | why |
|---|---|---|
| concept model | `banana_pro`, 2K, 2 candidates | the model behind every Meshy concept, so new concepts match |
| aspect | character 3:4, object and prop 1:1, scenery 4:3 | Meshy's, plus a wider frame for landscape pieces |
| model | `v3.1-20260211` | the newest H model (P-series low-poly topology is not needed: the builds decimate) |
| texture | detailed, `texture_version` v3.5 with `delight` | sharper texture; delight strips lighting baked into the concept (style guide: no baked lighting) |
| pbr | false | the static and unit builds keep only the base colour; Meshy's models had no PBR either |
| geometry_quality | standard | Ultra costs extra and the builds reduce to a few thousand triangles anyway |
| orientation | default | Tripo stands it upright and front-facing; `align_image` would copy the concept's three-quarter view |
| texture_alignment | original_image | keep the concept's colours |
| auto_size | false | the pipelines scale every model to its game box |
| face_limit | units and buildings 30,000, nodes 8,000, props 3,000, decor 10,000 or per asset (mountains 20,000, rocks 5,000-6,000...) | detail for the builds to reduce from (nodes end at 2,000-3,000, buildings at 6,000-10,000) |

## After the models arrive

- Check which way the first models face: the builds expect the front toward
  +Z. If Tripo's face another way, set `yaw` in `art/static/assets.toml`
  (static models) or `YAW` in the unit's `unit.py`.
- The stumps (`pine_stump`, `oak_stump`, `palm_stump`) and the tree variants
  (`oak_d`, `oak_e`, `pine_d`) are picked up by the static build as they are.
- `birch` and `willow` are new node ids: they need game data
  (`assets/data/core/nodes.ron`: resource wood, amount, box) and the engine's
  tree list (`TREE_IDS`) before the game uses them.
- `decor` is a new kind: the static build, the visual registry and the map
  generator do not know it yet (the art review's "must change" list); until
  then the decor GLBs only sit in `art/static/source/decor/`.

## Tested, and what is not

`tools/tripo/test_tripo.py` (43 tests, a few seconds) runs everything against a fake Tripo
that checks every request body against the parameters the docs list for its
endpoint, and the HTTP layer against a fake `urlopen`: payloads per endpoint,
resume after a crash at every stage without paying twice, the credit and
balance caps, 429 backoff from the headers, category queues, concurrency,
polling 100 at a time, expired links re-queried, uncertain creates,
reinstalls, a model already in place never paid for again, install paths per
kind, the Studio kit and import, the schema checks, the real requests and
style files (and the costs above), and that the real Meshy ledger yields
exactly the eleven units.

Checked against the live API (free calls only): `GET /account/balance`,
`GET /account/usage` and `POST /files` (multipart upload: a file token comes
back). No task was ever created: the account has 0 API credits. Untested
until there are credits: the real answers of the task-creating endpoints and
`POST /tasks/list` (their shapes are from the docs), real prices
(`credits_consumed`), Tripo's real rate-limit and concurrency numbers, and
which way Tripo's models face.
