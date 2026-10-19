# Static model fixtures

Tiny models for `af_render`'s tests, built by the real static pipeline
(`art/static/`, contract: `docs/art/static-models.md`). Each GLB is a few
KB but has every feature the engine reads: variants, a stump, a team
banner, a palette texture.

**This folder is a miniature asset folder.** Every path in the manifests and
in `static.ron` is relative to this folder (the build ran with
`--asset-root` set to it), so a test can point an asset server or a path
join at `tests/fixtures/static/` and resolve them exactly as the game
resolves `assets/`-relative paths.

| file | what |
|---|---|
| `static.ron` | the registry: `units: {}`, `buildings: {"house": ...}`, `nodes: {"pine": ..., "stone_outcrop": ...}` |
| `nodes/pine/pine.model.ron` | `kind: Node`, `fit: Height`, `random_yaw: true`, variants `a.glb` (30 triangles, 4.54 m tall) and `b.glb` (40, 5.42 m), `stump: Some(stump.glb)` (32, 0.35 m), `banner: false` |
| `nodes/stone_outcrop/stone_outcrop.model.ron` | `kind: Node`, `fit: Footprint`, `random_yaw: true`, one variant `a.glb` (40 triangles, 2.53 x 1.22 x 1.70 m), `stump: None` |
| `buildings/house/house.model.ron` | `kind: Building`, `fit: Box`, `random_yaw: false`, one variant `a.glb` (84 triangles with the banner, 4 x 3 x 4 m without it), `banner: true` |

In every GLB the model is the node named after the id (`pine`,
`stone_outcrop`, `house`), with no transform, base on y = 0, centred on
x/z, front +Z. The house also has the node `Team_banner`, whose flag and
finial use the material `Team` (plain white) and whose pole uses
`Banner_pole`; the manifest's `min`/`max` exclude the banner (its flag rises
above `max.y`). The ids are real game ids, so tests can use the real
content's boxes (pine 0.9 x 6.0 x 0.9, stone outcrop 2.4 x 1.6 x 2.4, house
4 x 3 x 4).

## Rebuilding

From the repository root (writes the sources to a temporary folder, builds
them here, and rewrites `static.ron`):

```bash
"C:/Program Files/Blender Foundation/Blender 4.4/blender.exe" -b -P art/static/placeholders.py -- --fixtures
```

The recipes are the `fx_*` functions in `art/static/placeholders.py`. Do not
edit these files by hand.
