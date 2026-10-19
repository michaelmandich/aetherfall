"""Build the static models (buildings and resource nodes) for the game.

Run headless from the repository root:

    "C:/Program Files/Blender Foundation/Blender 4.4/blender.exe" -b \
        -P art/static/build_static.py -- [options]

Options (after the `--`):
    --only pine,house    build only these ids (the registry still lists every
                         asset already built)
    --source DIR         where the source models are (default art/static/source),
                         in DIR/buildings/ and DIR/nodes/
    --out-root DIR       where the models go (default assets/models/static)
    --registry PATH      the generated registry (default assets/visuals/static.ron)
    --asset-root DIR     the folder the engine's asset paths are relative to
                         (default assets/); --out-root must be inside it
    --settings PATH      per-asset settings (default art/static/assets.toml)
    --sheets DIR         contact sheets go here (default: a folder in the
                         system temp folder, printed at the end)
    --no-sheets          no contact sheets
    --prune              delete built assets whose source is gone (without it
                         the build only lists them)
    --data DIR           the game content, for ids and box sizes (default assets/data)
    --selftest           build synthetic sources with every awkward feature into a
                         temporary folder and check the results (lib/selftest.py)

The contract with the engine (names, formats, defaults) is
docs/art/static-models.md; how to use this is art/static/README.md.
"""

import argparse
import os
import shutil
import sys
import tempfile
import time

sys.dont_write_bytecode = True  # keep __pycache__ out of the repository
HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
if HERE not in sys.path:
    sys.path.insert(0, HERE)

from lib import content, discover, settings  # noqa: E402
from lib.build import BuildError, build_asset  # noqa: E402
from lib.manifest import find_manifests, write_registry  # noqa: E402

DEFAULT_SHEETS = os.path.join(tempfile.gettempdir(), "aetherfall-static-sheets")


def parse_args(argv=None):
    if argv is None:
        argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    ap = argparse.ArgumentParser(prog="build_static.py", description=__doc__.split("\n\n")[0])
    ap.add_argument("--only", default=None)
    ap.add_argument("--source", default=os.path.join(HERE, "source"))
    ap.add_argument("--out-root", default=os.path.join(REPO, "assets", "models", "static"))
    ap.add_argument("--registry", default=os.path.join(REPO, "assets", "visuals", "static.ron"))
    ap.add_argument("--asset-root", default=os.path.join(REPO, "assets"))
    ap.add_argument("--settings", default=os.path.join(HERE, "assets.toml"))
    ap.add_argument("--sheets", default=DEFAULT_SHEETS)
    ap.add_argument("--no-sheets", action="store_true")
    ap.add_argument("--prune", action="store_true")
    ap.add_argument("--data", default=os.path.join(REPO, "assets", "data"))
    ap.add_argument("--selftest", action="store_true")
    return ap.parse_args(argv)


def _abs(path):
    return os.path.abspath(path if os.path.isabs(path) else os.path.join(os.getcwd(), path))


def run(args):
    """Build what `args` asks for. Returns the number of failed assets."""
    t0 = time.time()
    source, out_root, asset_root = _abs(args.source), _abs(args.out_root), _abs(args.asset_root)
    registry = _abs(args.registry)
    rel = os.path.relpath(out_root, asset_root)
    if rel.startswith("..") or os.path.isabs(rel):
        raise SystemExit(f"--out-root {out_root} is not inside --asset-root {asset_root}")
    game = content.load(_abs(args.data))
    overrides = settings.load(_abs(args.settings))
    assets = discover.discover(source, game)
    print(f"source {source}: {len(assets)} asset(s)")

    wanted = sorted(assets)
    if args.only:
        names = [n.strip() for n in args.only.split(",") if n.strip()]
        unknown = [n for n in names if not any(a.id == n for a in assets.values())]
        if unknown:
            raise SystemExit(f"--only: no source for {unknown}; have {sorted(a.id for a in assets.values())}")
        wanted = [k for k in wanted if assets[k].id in names]
    stale = [i for i in overrides if not any(a.id == i for a in assets.values())]
    if stale and not args.only:
        print(f"  (assets.toml has settings for ids without a source: {', '.join(sorted(stale))})")

    sheets = None if args.no_sheets else _abs(args.sheets)
    failed = []
    for key in wanted:
        asset = assets[key]
        entry = game.get(asset.id) if asset.known else None
        print(f"\n== {asset.key} ({', '.join(asset.variants)}{' + stump' if asset.stump else ''})")
        for note in asset.notes:
            print(f"  ! {note}")
        try:
            s = settings.resolve(asset.kind, asset.id, entry, overrides)
            build_asset(asset, s, entry, out_root, asset_root, sheets)
        except (BuildError, settings.SettingsError, RuntimeError, ValueError) as e:
            print(f"  FAILED: {e}")
            failed.append(asset.key)

    orphans = [(kind, ident) for kind, found in find_manifests(out_root).items() for ident in found
               if (kind, ident) not in assets]
    for kind, ident in orphans:
        folder = os.path.join(out_root, kind, ident)
        if args.prune:
            shutil.rmtree(folder)
            print(f"\nremoved {folder}: no source")
            # Leave no empty <kind>/ or output folder behind.
            for empty in (os.path.join(out_root, kind), out_root):
                if os.path.isdir(empty) and not os.listdir(empty):
                    os.rmdir(empty)
        else:
            print(f"\n! {folder} has no source in {source}; it stays in the registry "
                  f"(delete the folder, or build with --prune)")
    manifests = find_manifests(out_root)
    write_registry(registry, manifests, asset_root, out_root)
    n = sum(len(v) for v in manifests.values())
    print(f"\nregistry {registry}: {n} asset(s)")
    if sheets and wanted:
        print(f"contact sheets in {sheets}")
    if failed:
        print(f"FAILED: {', '.join(failed)} (their previous output, if any, is untouched)")
    print(f"done in {time.time() - t0:.1f}s")
    return len(failed)


def main():
    try:
        args = parse_args()
        if args.selftest:
            from lib import selftest
            failed = selftest.run(run, parse_args)
        else:
            failed = run(args)
    except (discover.SourceError, settings.SettingsError, content.RonError) as e:
        print(f"error: {e}")
        sys.exit(2)
    except Exception:
        import traceback
        traceback.print_exc()
        sys.exit(1)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
