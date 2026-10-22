# HUD skin: painted UI textures

`paint_ui.py` paints every texture of the HUD skin (worn, ornate medieval:
stained wood in riveted, scratched metal frames) procedurally and
deterministically, and writes the QA sheet, two HUD previews and a
readability report next to it. Nothing in `assets/ui/` is edited by hand:
change the script and regenerate.

## Regenerating

From the repository root (Python 3.10 with numpy 1.23, Pillow 10.2 and
scipy 1.12):

```bash
python art/ui/paint_ui.py assets/ui                          # everything, about a minute
python art/ui/paint_ui.py assets/ui --only buttons           # just the ten buttons (plus the QA files)
python art/ui/paint_ui.py assets/ui --only button_economy,fx  # one skin and the effect sprites
python art/ui/paint_ui.py assets/ui --no-preview             # textures only, no QA files
```

| option | effect |
|---|---|
| `--only a,b` | Paints only these: `panel_wood`, `panel_frame`, `bar_frame`, `buttons`, `button_<skin>`, `fx` (glow, burst ring, spark, shine). The QA files are still made from everything in the texture folder. |
| `--no-preview` | Skips `sheet.png`, the previews and `readability.txt`. |
| `--jobs N` | Paints the buttons on N processes (default 10, `1` for none). The output does not depend on it. |
| `--preview-dir DIR` | Where the QA files go (default `art/ui/out/`, which is git-ignored). |

Every seed is fixed: running it again gives the same bytes (checked with
`--jobs 10` against `--jobs 1`). The previews print text with the engine's
font from the cargo registry (`FONT_PATH` at the top of the script).

## The files

All textures are PNG, RGBA, sRGB, straight alpha, and painted at **2x**: a
texture pixel is half a logical UI pixel, and the slicer draws corners at
`max_corner_scale = 0.5`. Bevy's slicer (bevy_ui_render 0.19) uses
`coef = min(node_size / texture_size, 0.5)`, so every texture is as small as
its corners allow.

| file | size px | how the engine draws it | what it is |
|---|---|---|---|
| `panel_wood.png` | 512x512 | `NodeImageMode::Tiled { tile_x: true, tile_y: true, stretch_value: 0.5 }` | Background of every panel: dark walnut planks, 64 px tall, seamless on both axes. |
| `panel_frame.png` | 160x160 | sliced, border `BorderRect::all(40.0)`, sides `Tile { stretch_value: 0.5 }`, centre `Stretch` | Riveted iron frame over a panel's wood: 12 px band, an iron rivet centred on each 80 px side segment, a 17 px brass corner plate and a riveted brass diamond on each band beside it (after ref3). Everything, its shadow on the wood included, stays within 18 px (9 logical) of the outer edge, so it never meets the command card's slots (9 px padding), whichever of the two is drawn first. The centre is transparent. |
| `bar_frame.png` | 160x48 | sliced, border `BorderRect::axes(40.0, 20.0)`, sides `Tile { stretch_value: 0.5 }`, centre `Stretch` | The same frame for thin strips (top bar, control groups, one-line tooltips): a brass rivet per 80 px band segment, brass end straps with a small riveted diamond. Its 8 px left / right segments are one row repeated. |
| `button_<skin>.png` x10 | 168x124 | sliced, border `BorderRect::all(32.0)`, sides and centre `Stretch` | A whole button: metal frame, corner ornaments, two plaques in dark wells, stained wood. Fully opaque. An 84x62 command-card slot draws it at exactly 0.5; other buttons (24x22 to 240x44) stretch it. |
| `glow.png` | 168x124 | sliced like the buttons, drawn over a button at the same size | White, alpha only: a bright line on the dark seam between band and wood with a short tail into the wood (about 0.2 at 1.5 logical px, 0 by 4), a bloom over the band and a bright rim on the frame's outer edge. 0 at the texture border, in the stretched centre and over both plaque wells (so hover and armed never touch the hotkey and count). Tinted gold at runtime: hover at alpha 0.35, the armed button pulsing up to about 0.8, the press flash. |
| `burst_ring.png` | 256x256 | `NodeImageMode::Stretch` | White soft ring expanded from a pressed button. |
| `spark.png` | 32x32 | `NodeImageMode::Stretch` | White four-point sparkle flying out of a pressed button. |
| `shine.png` | 64x128 | `NodeImageMode::Stretch` | Soft diagonal white streak swept across a pressed button. |

In code (Bevy 0.19):

```rust
let panel_frame = NodeImageMode::Sliced(TextureSlicer {
    border: BorderRect::all(40.0),
    center_scale_mode: SliceScaleMode::Stretch,
    sides_scale_mode: SliceScaleMode::Tile { stretch_value: 0.5 },
    max_corner_scale: 0.5,
});
let bar_frame = NodeImageMode::Sliced(TextureSlicer {
    border: BorderRect::axes(40.0, 20.0),
    center_scale_mode: SliceScaleMode::Stretch,
    sides_scale_mode: SliceScaleMode::Tile { stretch_value: 0.5 },
    max_corner_scale: 0.5,
});
let button = NodeImageMode::Sliced(TextureSlicer {
    border: BorderRect::all(32.0),
    center_scale_mode: SliceScaleMode::Stretch,
    sides_scale_mode: SliceScaleMode::Stretch,
    max_corner_scale: 0.5,
}); // also glow.png
let wood = NodeImageMode::Tiled { tile_x: true, tile_y: true, stretch_value: 0.5 };
```

A disabled button is the same texture multiplied by grey 0.45; there are no
disabled variants.

### Button skins

| skin | used for | frame metal | wood | plaque bezel / well corner | bottom corner piece |
|---|---|---|---|---|---|
| `command` | Move, Stop, Hold, Patrol, Attack, Gather, Repair, Rally | blued steel | deep blue | round | riveted L-bracket with a domed rivet |
| `stance` | Aggressive / Defensive / Stand ground / Passive | violet gunmetal | even slate violet-grey | octagonal | square cap with a four-faced forged stud |
| `economy` | the Economy build page and its buildings | dark olive brass, green patina | golden oak (honey), deep stain | round | riveted quarter-round brass fan |
| `military` | the Military build page and its buildings | blackened iron, rust-red wear | crimson | octagonal | forged corner spike, studded straps |
| `defence` | the Defence build page, walls and towers | bright silver-steel | pale ash, cool grey-blue, deep stain | hexagonal (a bolt head) | big hex bolt on a square plate, small bolts on the straps |
| `train` | train a unit | dark, red bronze | green | round | bronze leaf pointing out of the corner, leaves along the bands |
| `research` | research a tech | pale yellow gold, engraved scroll along the bands | purple | round | six-lobed rosette and gold scroll curls |
| `upgrade` | upgrade a building, age up | rich red gold | red amber | round | fleur with an amber cabochon, scroll curls |
| `nav` | More, Back | plain warm grey iron | dark, cold walnut | octagonal | riveted plate and straps |
| `plain` | every other button (menu, top bar, overlays, Cancel) | dull brown bronze with bright worn edges | medium brown | round | round bronze boss and small round plates |

The top corners carry the plaques; their outer strips (above and beside the
wells) carry a small family ornament: straps and rivets, a stud, a spike, a
bolt, leaves, filigree or a fleur. The band metals are at least 12 dE76 apart
at 1x (the closest pair, command and defence, differ in lightness and hue:
blued against white). The ornaments are the family metal lifted (`orn` in
`SKINS`) so the dark metals' corner pieces still read in greyscale.

## Plaque and slicing rules

* Every skin, `plain` included, has two plaques: the **hotkey** plaque
  centred at (20, 19.5) texture px and the **queued-count** plaque at
  (148, 19.5). The engine centres the 14 px hotkey / count text on them, at
  **(10, 9.75) and (74, 9.75) logical px** of an 84x62 slot.
* A plaque is a dark glass gem in a blackened, ink-outlined bezel, at most
  12 px from its centre (20 + 12 = 32), so not one texel falls in the
  stretched sides of the 32 px slicer border. The bezel shape is the
  family's: hexagonal for defence, octagonal for stance, military and nav,
  round for the rest.
* The 14 px glyph (about 8.4 x 10 logical px) is bigger than the plaque: with
  the 1-2 px ring around it, it covers texture x 8..32, y 6..33. So each
  plaque sits in a **well** (`SOCK`: x 6..31.6, y 5..31.6, cutting 1-2 px into
  the band) of deep enamel in the gem's colour, with a lip of blackened
  family metal; its inner corner follows the bezel (rounded, chamfered or
  clipped). Everything in a well is soft-clipped to luminance `WELL_YMAX`
  (0.05), so the glyph and its ring land on dark metal on every skin and in
  every state. A soft shade on the wood just under each well (down to y 40,
  fading out before x 32) keeps the bottom of the glyph's ring dark too; it
  is a smooth gradient, so the slicer may stretch it on tall buttons.
* Ornaments, wells and plaques stay inside the 32x32 corners. The script
  shades each button twice (wood and band only, then with ornaments and
  plaques) and keeps the ornament render only inside the corners, fading
  their shadows out before the slice lines, so nothing from a corner reaches
  a stretched side: stretched buttons (Age Up, Menu, overlays) show no smear.
* Bottom corner pieces keep out of the label's way: a 16 px corner square
  plus straps along the bands at most 9.5 px (bottom) and 11 px (sides)
  thick (`ORN_SQ`, `ORN_ARM_B`, `ORN_ARM_S`). Nothing brighter than the wood
  falls in x 16..32 / 136..152, y 92..112, where three-line and 76 px wide
  labels run.
* Side bands are at most about 8 px (4 logical): 7.0-7.3 px of metal plus
  a dark seam. The label (white, 10-14 px, up to 3 lines) runs close to them.

## Readability rules

`art/ui/out/readability.txt` measures the written PNGs and ends with
`ALL PASS` or the list of failures. The limits:

* Buttons, label area x 16..152, y 26..112 texture px: mean WCAG relative
  luminance <= 0.16 and 95th percentile <= 0.30. Economy and defence (honey
  oak and pale ash) are the watch items and must stay at p95 <= 0.20; the
  script stains them deeper (target mean 0.105, p95 0.15, so even three-line
  labels keep 4.5:1) and darkens any skin's wood automatically if it exceeds
  its target.
* `panel_wood.png`: mean <= 0.10, p95 <= 0.18.
* Text contrast at 1x on an 84x62 slot, 10th percentile of the contrast
  against the 1-2 px ring around the glyphs, in the normal, hover (glow
  0.35) and armed (glow 0.8) states:
  * hotkey, 14 px, worst of the card's 15 letters: >= 4.5 in `TEXT` in every
    state, >= 3.0 in `TEXT_DIM` (the engine's colour today). It is measured
    with the line box centred, as Bevy centres a text node, and reported
    with the ink box centred too (stricter for Q, whose tail lifts it);
  * count, 14 px, `HIGHLIGHT`, "3" / "8": >= 4.5 in every state ("12" is
    reported, see the engine notes);
  * labels, worst of 11 card labels (one to three lines, 14 down to 11 px,
    up to 76 px wide): >= 4.0 in every state.
* Buttons have no texel below alpha 255; `glow.png` and the effect sprites
  are 0 at their texture border, and the glow is 0 over the plaque wells;
  frame centres are 0; `panel_frame` reaches at most 18 px from its edge.
* The band metals of any two skins differ by at least 10 dE76; the
  label-safe zone beside the bottom ornaments is no brighter than the plain
  wood; the wells stay under `WELL_YMAX`.
* Tiny buttons (24x22 and 40x30, the top-bar arrows) keep a frame ring at
  least 1.8x brighter than the wood inside and around them.

The report also lists hover / armed visibility (frame ring dL* and dE76),
wood colour distances, band metal hue and the seam checks (wood wrap against
interior plank grooves, frame side segments end to end, bar side rows).

## QA files (`art/ui/out/`, git-ignored)

* `sheet.png`: every texture at 2x and 1x on a checkerboard, then the QA
  panels: panel_wood 2x2 at 1x (seam check), a panel frame and a top bar over
  wood at 1x, all skins as 84x62 slots, a 900 px frame (side tiling),
  stretched and tiny buttons (24x22 to 240x44), every skin disabled, hovered
  and armed, and the plaque fit at 3x (the 14 px hotkey and count as the
  engine centres them, normal and armed).
* `preview_hud.png`: a 1280x720 mock of the HUD composited with Bevy's
  9-slice math: top bar, minimap, selection panel, and the 454x212 command
  card (5x3 slots of 84x62, 4 px gaps, 9 px padding, as `card_view.rs`).
* `preview_zoom.png`: that command card at 3x, nearest neighbour.
* `readability.txt`: the numbers above.

## Engine notes

What the art cannot do on its own, for whoever wires the textures in:

* Centre the hotkey and count text nodes at (10, 9.75) and (74, 9.75) of the
  slot (`card_view.rs` places them at left 4 / right 4, top 0 today).
  `TEXT_DIM` hotkeys now read at 3.0:1 or better on every skin, but `TEXT`
  (6.8:1 or better), ideally with `theme::text_shadow()`, is the better
  choice.
* A two-digit count ("12", 16.8 px at 14 px) is wider than the plaque and
  runs onto the side band whatever the art does: draw counts of 10 or more at
  11 px, or show "9+".
* Three 14 px label lines fill the slot's padded box, so the third line's
  descenders touch the bottom band. Prefer 12 px when a label needs three
  lines, or a line height of 0.95.
* `panel_frame` stays within 9 px of its edge, so it can be drawn under or
  over the command card's slots.
* Top bar: the resource names in `TEXT_DIM` at 14 px reach about 3.1-3.5:1
  on the wood (the values in `TEXT` reach 7-8:1). Draw the names in `TEXT`
  or give them the text shadow.
* The hover glow changes the bright gold and silver frames less than the
  dark ones (frame-ring dL* about 9-11 against 17 on military). It shows on
  every skin as a gold rim and seam line; a hover alpha of 0.45 instead of
  0.35 evens it out if that ever matters.

## Changing the art

The skins are the `SKINS` table (metal, wood, band width and profile,
corner ornament, plaque bezel, gem colour, ornament lift, seed). Metals are
`METALS`, wood stains `WOODS`; corner ornaments are `corner_top` /
`corner_bottom`, the wells and plaques `sd_socket` / `plaque`, frames
`build_panel_frame` / `build_bar_frame`. Look at
`sheet.png` and `preview_zoom.png` and check `readability.txt` after every
change.

**Per-age variants** (the world slowly turns medieval + tech) can be painted
later as a new set beside this one: give the script another palette set
(metals, stains, ornaments) and write it under a new prefix or folder, for
example `assets/ui/age3/` or `button_age3_<skin>.png`, with the same sizes,
borders and plaque positions so the engine only swaps image handles.
