"""Small PNG files for the models' textures (numpy + zlib, no extra packages).

Blender's own PNGs are RGBA at a low compression level, and the glTF
exporter embeds a packed PNG as it is, so the build writes its own:

* opaque textures: a 256-colour palette (k-means in sRGB on a sample, fixed
  seed: the build is reproducible), unfiltered rows, zlib level 9. About
  half the size of RGB; the mean error is around 1/255 on painted
  textures, invisible at RTS distance. Bevy expands palettes on load.
* alpha-cutout textures: RGBA, the best row filter per row. The alpha is
  made binary at the cutoff (the engine tests it against the same cutoff),
  and the colour of cut texels is bled from their opaque neighbours so
  filtering does not draw a dark fringe round the leaves.

Same approach as art/peasant/lib/png.py, plus alpha.
"""

import struct
import zlib

import numpy as np

PALETTE_SAMPLES = 60000  # texels the palette is fitted to
PALETTE_ROUNDS = 12      # k-means iterations
SEED = 0


def encode_rgb(rgb, colours=256):
    """PNG bytes for an (h, w, 3) uint8 image (top row first), palette of
    `colours` (0 = lossless RGB). Returns (bytes, decoded (h, w, 3) pixels)."""
    rgb = np.ascontiguousarray(rgb, np.uint8)
    h, w, _ = rgb.shape
    flat = rgb.reshape(-1, 3).astype(np.uint32)
    keys, index = np.unique((flat[:, 0] << 16) | (flat[:, 1] << 8) | flat[:, 2], return_inverse=True)
    if len(keys) <= 256:  # few colours already (flat-shaded art): an exact palette
        palette = np.stack([(keys >> 16) & 255, (keys >> 8) & 255, keys & 255], 1).astype(np.uint8)
        return _palette_png(palette, index.reshape(-1), w, h), rgb
    if not colours:
        return _png(_filtered(rgb.reshape(h, w * 3), 3), w, h, 2), rgb
    palette, index = quantise(rgb, colours)
    return _palette_png(palette, index, w, h), palette[index].reshape(h, w, 3)


def encode_rgba_cutout(rgba, cutoff=0.5):
    """PNG bytes for an (h, w, 4) uint8 image with alpha made binary at
    `cutoff`. Returns (bytes, decoded (h, w, 4) pixels)."""
    rgba = np.array(rgba, np.uint8)
    h, w, _ = rgba.shape
    keep = rgba[..., 3] >= round(cutoff * 255)
    rgba[..., 3] = np.where(keep, 255, 0)
    rgba[..., :3] = _bleed(rgba[..., :3], keep)
    return _png(_filtered(rgba.reshape(h, w * 4), 4), w, h, 6), rgba


def _bleed(rgb, keep, rounds=8):
    """Colour of cut texels: the mean of opaque neighbours, grown outward."""
    out = rgb.astype(np.float32)
    have = keep.copy()
    if not have.any():
        return rgb
    for _ in range(rounds):
        acc = np.zeros_like(out)
        cnt = np.zeros(have.shape, np.float32)
        for dy, dx in ((-1, 0), (1, 0), (0, -1), (0, 1)):
            src_have = np.roll(have, (dy, dx), (0, 1))
            acc += np.roll(out, (dy, dx), (0, 1)) * src_have[..., None]
            cnt += src_have
        grow = ~have & (cnt > 0)
        out[grow] = acc[grow] / cnt[grow][:, None]
        have |= grow
    out[~have] = out[keep].mean(0)
    return np.round(out).astype(np.uint8)


def quantise(rgb, colours):
    """(palette (n, 3) uint8, index per texel) by k-means on a sample."""
    flat = rgb.reshape(-1, 3).astype(np.float32)
    rng = np.random.default_rng(SEED)
    sample = flat[rng.choice(len(flat), min(PALETTE_SAMPLES, len(flat)), replace=False)]
    centres = sample[rng.choice(len(sample), colours, replace=False)].copy()
    for _ in range(PALETTE_ROUNDS):
        label = _nearest(sample, centres)
        sums = np.zeros_like(centres)
        np.add.at(sums, label, sample)
        counts = np.bincount(label, minlength=colours)[:, None]
        centres = np.where(counts > 0, sums / np.maximum(counts, 1), centres)
    palette = np.round(centres).clip(0, 255).astype(np.uint8)
    return palette, _nearest(flat, palette.astype(np.float32)).astype(np.uint8)


def _nearest(points, centres, block=1 << 16):
    c2 = (centres ** 2).sum(1)
    out = np.empty(len(points), np.int64)
    for i in range(0, len(points), block):
        p = points[i:i + block]
        out[i:i + block] = (c2[None, :] - 2.0 * p @ centres.T).argmin(1)
    return out


def _palette_png(palette, index, w, h):
    rows = np.hstack([np.zeros((h, 1), np.uint8), np.asarray(index, np.uint8).reshape(h, w)])
    return _png(rows.tobytes(), w, h, 3, palette)


def _filtered(raw, bpp):
    """Filter-type byte + filtered bytes per row (the filter with the
    smallest sum of absolute signed residuals per row, as libpng does)."""
    h, n = raw.shape
    x = raw.astype(np.int16)
    up = np.vstack([np.zeros((1, n), np.int16), x[:-1]])
    left = np.hstack([np.zeros((h, bpp), np.int16), x[:, :-bpp]])
    upleft = np.hstack([np.zeros((h, bpp), np.int16), up[:, :-bpp]])
    p = left + up - upleft
    pa, pb, pc = np.abs(p - left), np.abs(p - up), np.abs(p - upleft)
    paeth = np.where((pa <= pb) & (pa <= pc), left, np.where(pb <= pc, up, upleft))
    candidates = np.stack([x, x - left, x - up, x - (left + up) // 2, x - paeth]) % 256
    cost = np.abs(candidates.astype(np.uint8).astype(np.int8).astype(np.int32)).sum(2)
    best = cost.argmin(0)
    rows = candidates[best, np.arange(h)].astype(np.uint8)
    return np.hstack([best.astype(np.uint8)[:, None], rows]).tobytes()


def _png(data, w, h, colour_type, palette=None):
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    out = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, colour_type, 0, 0, 0))
    if palette is not None:
        out += chunk(b"PLTE", np.asarray(palette, np.uint8).tobytes())
    return out + chunk(b"IDAT", zlib.compress(data, 9)) + chunk(b"IEND", b"")
