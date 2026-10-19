"""Small PNG files for the model's texture (numpy + zlib, no extra packages).

Blender writes RGBA PNGs at a low compression level; the glTF exporter
embeds a packed PNG as-is, so the build writes its own: RGB (the base
colour has no alpha), the best PNG row filter per row and zlib level 9,
and optionally a 256-colour palette (k-means in sRGB, a fixed seed so the
build is reproducible), which roughly halves the file. Bevy's PNG loader
expands palette images to RGBA on load.
"""

import struct
import zlib

import numpy as np

PALETTE_SAMPLES = 60000  # texels the palette is fitted to
PALETTE_ROUNDS = 12      # k-means iterations
SEED = 0


def encode(rgb, colours=0):
    """PNG bytes for an (h, w, 3) uint8 image, top row first. With
    `colours` (2..256) it is quantised to a palette of that many colours.
    Returns (png bytes, the (h, w, 3) uint8 pixels the file decodes to)."""
    rgb = np.ascontiguousarray(rgb, np.uint8)
    h, w, _ = rgb.shape
    if not colours:
        return _png(_filtered(rgb.reshape(h, w * 3), 3), w, h, 2), rgb
    palette, index = quantise(rgb, colours)
    # Palette images compress best unfiltered.
    rows = np.hstack([np.zeros((h, 1), np.uint8), index.reshape(h, w)])
    return _png(rows.tobytes(), w, h, 3, palette), palette[index].reshape(h, w, 3)


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
    """Index of the nearest centre for every point (squared distance)."""
    c2 = (centres ** 2).sum(1)
    out = np.empty(len(points), np.int64)
    for i in range(0, len(points), block):
        p = points[i:i + block]
        out[i:i + block] = (c2[None, :] - 2.0 * p @ centres.T).argmin(1)
    return out


def _filtered(raw, bpp):
    """Filter-type byte + filtered bytes per row, choosing per row the filter
    with the smallest sum of absolute (signed) residuals, as libpng does."""
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
        out += chunk(b"PLTE", palette.tobytes())
    return out + chunk(b"IDAT", zlib.compress(data, 9)) + chunk(b"IEND", b"")
