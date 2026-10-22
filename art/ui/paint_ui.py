#!/usr/bin/env python3
"""
paint_ui.py -- procedural "worn ornate medieval" HUD skin for Aetherfall.

    python art/ui/paint_ui.py assets/ui [--only a,b] [--no-preview] [--jobs N] [--preview-dir DIR]

Writes the textures panel_wood.png, panel_frame.png, bar_frame.png,
button_<skin>.png (x10), glow.png, burst_ring.png, spark.png and shine.png
into the output folder, and the QA files sheet.png, preview_hud.png,
preview_zoom.png and readability.txt into art/ui/out/ (or --preview-dir).
See art/ui/README.md for what each file is and how the engine slices it.

Technique: every texture is built as a stack of layers (height field +
material channels) at 4x supersampling, shaded once with normals from the
composite height field (Lambert + Blinn-Phong for wood, environment
reflection + specular for metal, dark dielectric for gems), cavity AO,
height-field ray-marched shadows and curvature-driven wear (rust / patina in
crevices, polished convex edges), then box-downsampled in linear light. All
noise is FFT-filtered white noise, so it is periodic on the array; tileable
textures exploit that. Fixed seeds: re-running gives the same bytes.

Buttons are shaded twice, once with only the wood and the frame band and
once with the corner ornaments and hotkey plaques; outside the 32x32 slicer
corners the plain render is kept, so nothing an ornament adds (its shadow
included) can leak into the stretched sides and centre. Each plaque sits in a
dark well that fills its corner inside the band, so the engine's 14 px hotkey
and count glyphs (bigger than the plaque) always land on dark metal; glow.png
is cut out over the wells.
"""
import argparse
import math
import os
import time

import numpy as np
from PIL import Image, ImageDraw, ImageFont
from scipy import ndimage

F32 = np.float32
SS = 4
FONT_PATH = ("C:/Users/meman/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/"
             "bevy_text-0.19.1/src/FiraMono-subset.ttf")
TEXT_SRGB = (0.93, 0.92, 0.88)       # theme::TEXT (labels)
TEXT_DIM_SRGB = (0.62, 0.62, 0.60)   # theme::TEXT_DIM (hotkeys are drawn in it today)
HIGHLIGHT_SRGB = (1.0, 0.84, 0.35)   # theme::HIGHLIGHT (queued count)
GLOW_GOLD = (1.0, 0.80, 0.35)        # tint of glow.png for hover / armed
HERE = os.path.dirname(os.path.abspath(__file__))

LIGHT = np.array([-0.48, -0.60, 0.64])
LIGHT = LIGHT / np.linalg.norm(LIGHT)
LXY = LIGHT[:2] / np.linalg.norm(LIGHT[:2])
HALF = LIGHT + np.array([0.0, 0.0, 1.0])
HALF = HALF / np.linalg.norm(HALF)
TAN_E = LIGHT[2] / np.linalg.norm(LIGHT[:2])

BTN_W, BTN_H = 168, 124
CORNER = 32.0                  # button slicer border (texture px)
LABEL_BOX = (16, 152, 26, 112)  # x0, x1, y0, y1 (texture px)
# Hotkey / count plaques: centres in texture px and the outer radius of the
# whole plaque (ink outline included): 20 + 12 = 32 and 19.5 + 12 = 31.5, so
# not one texel of it falls in the stretched sides of the 32 px slicer. The
# engine centres the 14 px hotkey / count text at half these (logical px).
PLAQ_C = ((20.0, 19.5), (148.0, 19.5))
PLAQ_R = 12.0
TEXT_POS = ((PLAQ_C[0][0] / 2, PLAQ_C[0][1] / 2), (PLAQ_C[1][0] / 2, PLAQ_C[1][1] / 2))
# The 14 px glyph is about 8.4 x 10 logical px, bigger than the plaque: with the 1-2 px ring
# around it, it covers texture x 8..32, y 6..33. Each plaque therefore sits in a dark well
# (the "socket") that fills its corner inside the band, so the glyph and its ring land on
# dark metal everywhere. Corner coordinates (U, V = distance from the left / top edge):
SOCK = (6.0, 5.0, 31.6, 31.6)   # U0, V0, U1, V1: the well cuts 1-2 px into the band
# Bottom corners: the label (up to 3 lines of 14 px, or 76 px wide lines of 11 px) runs
# close to them, so a bottom ornament stays in a 16 px corner square plus arms along the
# bands at most 9.5 px (bottom) and 11 px (sides) thick: nothing bright in x 16..32,
# y 92..112 (the label-safe zone, measured in readability.txt).
ORN_SQ, ORN_ARM_B, ORN_ARM_S = 16.0, 9.5, 11.0
WELL_YMAX = 0.05   # luminance ceiling inside a well (soft clip): white text >= 9:1 on it

# readability limits (contract) and the extra plaque / label limits
LIM_MEAN, LIM_P95 = 0.16, 0.30
LIM_WOOD_MEAN, LIM_WOOD_P95 = 0.10, 0.18
LIM_GLYPH_LABEL = 4.0   # p10 contrast of the ring around a card label, in every state
LIM_HK_TEXT = 4.5       # ... around a 14 px hotkey in TEXT, every state
LIM_HK_DIM = 3.0        # ... around a 14 px hotkey in TEXT_DIM, normal state
LIM_COUNT = 4.5         # ... around a 14 px one-digit count in HIGHLIGHT, every state
CARD_HOTKEYS = 'QWERTASDFGZXCVB'
QA_LABELS = ['Barracks', 'Economy', 'Aggressive', 'Blacksmith', 'Train Archer', 'Research Iron Casting',
             'Research Heavy Plough', 'Upgrade Stone Wall', 'Fortified Watch Tower', 'Train Crossbowman',
             'Train Man-at-arms']
WATCH_P95 = 0.20        # economy / defence must stay under this p95
LIM_BAND_DE = 10.0      # the closest two skins' band metals differ by at least this (dE76)


# ----------------------------------------------------------------------------- colour
def s2l(c):
    c = np.asarray(c, dtype=F32)
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4).astype(F32)


def l2s(c):
    c = np.clip(np.asarray(c, dtype=F32), 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(c, 1 / 2.4) - 0.055).astype(F32)


def rgb8(r, g, b):
    return s2l(np.array([r, g, b], F32) / 255.0)


def lum(lin):
    return lin[..., 0] * 0.2126 + lin[..., 1] * 0.7152 + lin[..., 2] * 0.0722


def sstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def lerp(a, b, t):
    return a + (b - a) * t


def soft_clip(c, knee=0.80):
    over = np.maximum(c - knee, 0.0)
    return np.where(c > knee, knee + (1.0 - knee) * (1.0 - np.exp(-over / (1.0 - knee))), c).astype(F32)


# ----------------------------------------------------------------------------- noise
def pnoise(h, w, rng, sx, sy=None, beta=2.0):
    """Periodic (h x w) Gaussian noise with unit std. sx/sy: feature size (array px)."""
    if sy is None:
        sy = sx
    fy = np.fft.fftfreq(h)[:, None]
    fx = np.fft.rfftfreq(w)[None, :]
    k2 = (2 * np.pi) ** 2 * ((fx * sx) ** 2 + (fy * sy) ** 2)
    amp = (1.0 + k2) ** (-beta / 2.0)
    wn = rng.standard_normal((h, w))
    out = np.fft.irfft2(np.fft.rfft2(wn) * amp, s=(h, w))
    out -= out.mean()
    out /= out.std() + 1e-9
    return out.astype(F32)


# ----------------------------------------------------------------------------- geometry
def grid(w, h, ss):
    xs = (np.arange(w * ss, dtype=F32) + 0.5) / ss
    ys = (np.arange(h * ss, dtype=F32) + 0.5) / ss
    return np.meshgrid(xs, ys)


def cov_sd(sd, ss):
    return np.clip(0.5 - sd * ss, 0.0, 1.0).astype(F32)


def sd_box(X, Y, cx, cy, hx, hy, r=0.0):
    qx = np.abs(X - cx) - hx + r
    qy = np.abs(Y - cy) - hy + r
    return (np.hypot(np.maximum(qx, 0), np.maximum(qy, 0))
            + np.minimum(np.maximum(qx, qy), 0) - r).astype(F32)


def sd_circle(X, Y, cx, cy, r):
    return (np.hypot(X - cx, Y - cy) - r).astype(F32)


def sd_polygon(X, Y, verts):
    """Exact signed distance to a closed polygon (negative inside)."""
    v = np.asarray(verts, np.float64)
    n = len(v)
    d = np.full(X.shape, 1e18)
    s = np.ones(X.shape)
    for i in range(n):
        j = (i - 1) % n
        ex, ey = v[j, 0] - v[i, 0], v[j, 1] - v[i, 1]
        wx, wy = X - v[i, 0], Y - v[i, 1]
        t = np.clip((wx * ex + wy * ey) / (ex * ex + ey * ey), 0, 1)
        bx, by = wx - ex * t, wy - ey * t
        d = np.minimum(d, bx * bx + by * by)
        c1 = Y >= v[i, 1]
        c2 = Y < v[j, 1]
        c3 = ex * wy > ey * wx
        flip = (c1 & c2 & c3) | (~c1 & ~c2 & ~c3)
        s = np.where(flip, -s, s)
    return (s * np.sqrt(d)).astype(F32)


def ngon(cx, cy, R, n, rot):
    return [(cx + R * math.cos(rot + k * 2 * math.pi / n), cy + R * math.sin(rot + k * 2 * math.pi / n))
            for k in range(n)]


def sd_hex(X, Y, cx, cy, inr):
    """flat-top hexagon, inradius inr"""
    kx, ky, kz = -0.866025404, 0.5, 0.577350269
    px = np.abs(Y - cy)  # rotate 90 deg so a flat side faces up
    py = np.abs(X - cx)
    d = np.minimum(kx * px + ky * py, 0.0)
    px = px - 2.0 * d * kx
    py = py - 2.0 * d * ky
    px = px - np.clip(px, -kz * inr, kz * inr)
    py = py - inr
    return (np.hypot(px, py) * np.sign(py)).astype(F32)


def sd_socket(U, V, shape):
    """The dark well around a plaque (corner coordinates). Its inner corner (toward the
    wood) follows the family's bezel: rounded, chamfered (octagonal) or clipped (hex)."""
    u0, v0, u1, v1 = SOCK
    sd = sd_box(U, V, (u0 + u1) / 2, (v0 + v1) / 2, (u1 - u0) / 2, (v1 - v0) / 2, 2.2)
    if shape == 'round':
        R = 5.5
        q = (U > u1 - R) & (V > v1 - R)
        arc = np.hypot(np.maximum(U - (u1 - R), 0), np.maximum(V - (v1 - R), 0)) - R
        sd = np.where(q, np.maximum(sd, arc), sd)
    else:
        c = 5.5 if shape == 'oct' else 3.5
        sd = np.maximum(sd, (U + V - (u1 + v1 - c)) / math.sqrt(2))
    return sd.astype(F32)


def polyline_dist(X, Y, pts):
    pts = np.asarray(pts, np.float64)
    d = np.full(X.shape, 1e9, F32)
    seg = np.hypot(np.diff(pts[:, 0]), np.diff(pts[:, 1]))
    cum = np.concatenate([[0.0], np.cumsum(seg)])
    tot = cum[-1]
    tpar = np.zeros(X.shape, F32)
    for i in range(len(pts) - 1):
        ax, ay = pts[i]
        bx, by = pts[i + 1]
        pax = X - ax
        pay = Y - ay
        bax = bx - ax
        bay = by - ay
        hh = np.clip((pax * bax + pay * bay) / (bax * bax + bay * bay + 1e-12), 0, 1)
        di = np.hypot(pax - bax * hh, pay - bay * hh)
        m = di < d
        d = np.where(m, di, d)
        tpar = np.where(m, (cum[i] + hh * seg[i]) / tot, tpar)
    return d.astype(F32), tpar.astype(F32)


def spiral_pts(cx, cy, r0, r1, a0, turns, n=90, cw=1.0):
    t = np.linspace(0.0, 1.0, n)
    ang = a0 + cw * t * turns * 2 * np.pi
    rad = r0 + (r1 - r0) * t
    return np.stack([cx + rad * np.cos(ang), cy + rad * np.sin(ang)], -1)


# ----------------------------------------------------------------------------- materials
METALS = {
    # base = linear F0 tint, polish = bright worn-edge colour, wear = rust/patina/grime colour
    # command: blued steel, cooler and darker than defence's bright silver
    'steel': dict(base=(0.25, 0.31, 0.44), polish=0.8, polish_col=(0.70, 0.80, 0.98), wear=0.30,
                  wear_col=rgb8(70, 50, 40), gloss=0.65, mottle=0.12, pit=0.6, ham=0.8),
    # defence: bright, nearly white silver-steel
    'silver': dict(base=(0.76, 0.78, 0.80), polish=0.8, polish_col=(1.0, 1.0, 1.0), wear=0.30,
                   wear_col=rgb8(56, 52, 48), gloss=0.72, mottle=0.08, pit=0.35, ham=0.5),
    'iron': dict(base=(0.20, 0.20, 0.21), polish=1.0, polish_col=(0.60, 0.60, 0.62), wear=0.85,
                 wear_col=rgb8(104, 48, 24), gloss=0.45, mottle=0.20, pit=1.0, ham=1.0),
    # nav: plain grey iron, lighter and a little warm, rough
    'darkiron': dict(base=(0.215, 0.195, 0.170), polish=1.0, polish_col=(0.72, 0.68, 0.62), wear=0.5,
                     wear_col=rgb8(70, 50, 36), gloss=0.42, mottle=0.18, pit=0.9, ham=1.0, rough=0.35),
    # military: blackened iron with rust-red wear
    'blackiron': dict(base=(0.075, 0.068, 0.068), polish=1.0, polish_col=(0.56, 0.54, 0.54), wear=0.55,
                      wear_col=rgb8(104, 36, 16), gloss=0.55, mottle=0.25, pit=0.9, ham=1.0),
    # stance: blued-violet gunmetal
    'gunmetal': dict(base=(0.15, 0.125, 0.22), polish=1.0, polish_col=(0.66, 0.60, 0.84), wear=0.35,
                     wear_col=rgb8(54, 40, 64), gloss=0.6, mottle=0.18, pit=0.7, ham=0.9),
    # economy: darker antique brass, olive, green patina in the crevices
    'brass': dict(base=(0.36, 0.34, 0.10), polish=0.9, polish_col=(0.80, 0.80, 0.44), wear=0.70,
                  wear_col=rgb8(56, 100, 70), gloss=0.5, mottle=0.16, pit=0.5, ham=0.6),
    # panel / bar frame plates: the warmer aged brass of ref3
    'agedbrass': dict(base=(0.74, 0.49, 0.16), polish=0.9, polish_col=(1.0, 0.84, 0.46), wear=0.55,
                      wear_col=rgb8(52, 94, 74), gloss=0.6, mottle=0.15, pit=0.4, ham=0.6),
    # train: darker, redder bronze
    'bronze': dict(base=(0.44, 0.165, 0.050), polish=0.9, polish_col=(0.92, 0.54, 0.30), wear=0.55,
                   wear_col=rgb8(46, 88, 72), gloss=0.55, mottle=0.18, pit=0.5, ham=0.7),
    # plain: dull brown bronze with bright worn edges (stays readable when tiny)
    'dullbronze': dict(base=(0.44, 0.27, 0.13), polish=1.0, polish_col=(1.0, 0.78, 0.52), wear=0.40, rough=0.55,
                       wear_col=rgb8(52, 38, 24), gloss=0.48, mottle=0.18, pit=0.5, ham=0.8),
    # research: bright, pale yellow gold
    'gold': dict(base=(1.0, 0.80, 0.22), polish=0.8, polish_col=(1.0, 0.96, 0.66), wear=0.25,
                 wear_col=rgb8(48, 34, 14), gloss=0.8, mottle=0.08, pit=0.2, ham=0.4),
    # upgrade: the richest, redder ornate gold
    'redgold': dict(base=(0.92, 0.26, 0.02), polish=0.85, polish_col=(1.0, 0.54, 0.16), wear=0.35,
                    wear_col=rgb8(60, 22, 6), gloss=0.8, mottle=0.08, pit=0.2, ham=0.4),
}
# the well around the plaques: near-black enamel
WELL = dict(base=(0.011, 0.010, 0.011), metal=0.0, gloss=0.35, ks=0.02, wear=0.0, wear_col=(0, 0, 0),
            polish=0.0, polish_col=(0, 0, 0), gem=0.0, alpha=1.0)

WOODS = {  # (early wood, late wood, light streak) in sRGB 0-255
    'panel': ((110, 73, 46), (54, 35, 22), (140, 98, 61)),
    'blue': ((38, 66, 128), (16, 30, 72), (60, 98, 162)),
    'slate': ((92, 78, 108), (64, 54, 80), (110, 96, 128)),
    'honey': ((170, 120, 36), (102, 66, 16), (200, 148, 54)),
    'crimson': ((130, 34, 28), (64, 15, 13), (162, 54, 42)),
    'ash': ((96, 110, 128), (56, 64, 80), (126, 140, 158)),
    'green': ((48, 96, 56), (21, 47, 26), (72, 124, 76)),
    'purple': ((92, 48, 120), (44, 20, 66), (120, 72, 150)),
    'amber': ((176, 68, 18), (104, 30, 7), (212, 96, 36)),
    'walnut': ((70, 54, 44), (35, 26, 21), (90, 72, 60)),
    'brown': ((118, 78, 44), (64, 40, 22), (148, 102, 62)),
}


def metal_mat(name, n_mot, n_wear, wear_scale=1.0):
    M = METALS[name]
    base = np.asarray(M['base'], F32)[None, None, :] * (1.0 + M['mottle'] * n_mot)[..., None]
    wear = np.clip(M['wear'] * wear_scale * (0.6 + 0.55 * n_wear), 0.0, 1.0)
    return dict(base=base, metal=1.0, gloss=M['gloss'], ks=0.0, wear=wear,
                wear_col=np.asarray(M['wear_col'], F32), polish=M['polish'],
                polish_col=np.asarray(M['polish_col'], F32), gem=0.0, rough=M.get('rough', 0.0), alpha=1.0)


def darker(M, k, polish=None, gloss=None):
    D = dict(M)
    D['base'] = np.asarray(M['base'], F32) * k
    D['polish_col'] = np.asarray(M['polish_col'], F32) * (0.35 + 0.65 * k)
    if polish is not None:
        D['polish'] = polish
    if gloss is not None:
        D['gloss'] = gloss
    return D


def wood_mat(alb, gloss=0.32, ks=0.06):
    return dict(base=alb, metal=0.0, gloss=gloss, ks=ks, wear=0.0, wear_col=(0, 0, 0),
                polish=0.0, polish_col=(0, 0, 0), gem=0.0, alpha=1.0)


def gem_mat(base, spec=1.0, refl=1.0):
    return dict(base=base, metal=0.0, gloss=0.95, ks=0.0, wear=0.0, wear_col=(0, 0, 0),
                polish=0.0, polish_col=(0, 0, 0), gem=1.0, alpha=1.0, gspec=spec, grefl=refl)


# near-black forged edge / ink line around ornaments, so they read at 1x
INK = dict(base=(0.010, 0.008, 0.007), metal=1.0, gloss=0.3, ks=0.0, wear=0.0, wear_col=(0, 0, 0),
           polish=0.0, polish_col=(0, 0, 0), gem=0.0, alpha=1.0)


class Surf:
    S1 = ('h', 'metal', 'gloss', 'ks', 'wear', 'patch', 'polish', 'gem', 'gspec', 'grefl', 'rough', 'alpha')
    S3 = ('base', 'wear_col', 'polish_col')

    def __init__(self, shape):
        self.shape = shape
        for k in self.S1:
            setattr(self, k, np.zeros(shape, F32))
        for k in self.S3:
            setattr(self, k, np.zeros(shape + (3,), F32))
        self.cov = np.zeros(shape, F32)  # union of everything put since the last reset

    def copy(self):
        T = Surf.__new__(Surf)
        T.shape = self.shape
        for k in self.S1 + self.S3 + ('cov',):
            setattr(T, k, getattr(self, k).copy())
        return T

    def put(self, cov, h, mat):
        c = np.broadcast_to(np.asarray(cov, F32), self.shape)
        self.h += (np.broadcast_to(np.asarray(h, F32), self.shape) - self.h) * c
        for k in self.S1[1:]:
            v = np.broadcast_to(np.asarray(mat.get(k, 1.0 if k == 'alpha' else 0.0), F32), self.shape)
            arr = getattr(self, k)
            arr += (v - arr) * c
        c3 = c[..., None]
        for k in self.S3:
            v = np.broadcast_to(np.asarray(mat.get(k, (0.0, 0.0, 0.0)), F32), self.shape + (3,))
            arr = getattr(self, k)
            arr += (v - arr) * c3
        np.maximum(self.cov, c, out=self.cov)


# ----------------------------------------------------------------------------- shading
def env_light(rx, ry, rz):
    """Studio environment seen in reflections: big soft key box up-left, dark floor band,
    thin cool rim from the lower right."""
    t = rx * LXY[0] + ry * LXY[1]
    q = -t
    e = 0.10 + 0.17 * np.clip(rz, 0, 1)
    e = e + 2.4 * sstep(0.26, 0.78, t)
    e = e + 0.65 * sstep(0.55, 0.95, q)
    e = e * (1.0 - 0.72 * sstep(0.06, 0.38, q) * (1.0 - sstep(0.55, 0.9, q)))
    return e.astype(F32)


def shadow_march(H, ss, max_px, tan_e, wrap):
    mode = 'wrap' if wrap else 'nearest'
    occ = np.zeros_like(H)
    n = int(max_px * ss)
    step = max(1, ss // 2)
    for k in range(step, n + 1, step):
        sh = ndimage.shift(H, (-LXY[1] * k, -LXY[0] * k), order=1, mode=mode)
        occ = np.maximum(occ, sh - H - (k / ss) * tan_e)
    return occ


def shade(S, ss, wrap=False, sky=None, shadow_px=8.0, tan_e=None, amb=0.30, kd=0.92,
          ao_k=(0.30, 0.10)):
    if tan_e is None:
        tan_e = TAN_E
    mode = 'wrap' if wrap else 'nearest'
    H = S.h
    if wrap:
        gx = (np.roll(H, -1, 1) - np.roll(H, 1, 1)) * 0.5 * ss
        gy = (np.roll(H, -1, 0) - np.roll(H, 1, 0)) * 0.5 * ss
    else:
        gy, gx = np.gradient(H)
        gx = gx * ss
        gy = gy * ss
    inv = 1.0 / np.sqrt(gx * gx + gy * gy + 1.0)
    nx, ny, nz = -gx * inv, -gy * inv, inv
    ndl = np.clip(nx * LIGHT[0] + ny * LIGHT[1] + nz * LIGHT[2], 0, 1)
    ndh = np.clip(nx * HALF[0] + ny * HALF[1] + nz * HALF[2], 0, 1)
    rx, ry, rz = 2 * nz * nx, 2 * nz * ny, 2 * nz * nz - 1
    E = env_light(rx, ry, rz)
    if sky is not None:
        E = E + sky
    Hb = ndimage.gaussian_filter(H, 0.7 * ss, mode=mode)
    lap = ndimage.laplace(Hb, mode=mode) * ss * ss
    convex = sstep(0.25, 1.6, -lap)
    concave = sstep(0.15, 1.2, lap)
    c1 = ndimage.gaussian_filter(H, 1.2 * ss, mode=mode) - H
    c2 = ndimage.gaussian_filter(H, 4.0 * ss, mode=mode) - H
    ao = np.clip(1 - ao_k[0] * np.clip(c1, 0, None) - ao_k[1] * np.clip(c2, 0, None), 0.15, 1)
    occ = shadow_march(H, ss, shadow_px, tan_e, wrap)
    shad = 1 - 0.88 * sstep(0.0, 0.9, occ)

    wearm = np.clip(S.wear * np.clip(concave * 0.9 + (1 - ao) * 2.4, 0, 1) + S.patch, 0, 1)
    pol = np.clip(S.polish * convex, 0, 1)
    base = lerp(S.base, S.polish_col, pol[..., None])
    gloss = np.clip(S.gloss + 0.25 * pol - 0.5 * wearm, 0, 1)
    p = 6 + 420 * gloss ** 2
    spec = ndh ** p
    mcol = (base * (E * (0.35 + 0.65 * ao))[..., None]
            + base * (spec * shad * (1.5 + 3.0 * gloss))[..., None])
    diff = amb * ao + kd * ndl * shad
    # rough (oxidised, unpolished) metal: part of its light is diffuse
    mcol = lerp(mcol, base * diff[..., None], S.rough[..., None])
    fres = 0.04 + 0.96 * (1 - nz) ** 5
    dcol = S.base * diff[..., None] + (S.ks * (spec * shad * (0.6 + 1.5 * gloss) + 0.5 * fres * E * ao))[..., None]
    col = lerp(dcol, mcol, S.metal[..., None])
    wcol = S.wear_col * (diff * (0.9 + 0.2 * ndl))[..., None]
    col = lerp(col, wcol, (wearm * S.metal)[..., None])
    fg = 0.06 + 0.94 * (1 - nz) ** 3
    gcol = (S.base * (0.55 * ao + 0.6 * ndl * shad)[..., None]
            + (fg * E * 0.8 * S.grefl + ndh ** 600 * 3.0 * shad * S.gspec)[..., None])
    col = lerp(col, gcol, S.gem[..., None])
    return col.astype(F32), dict(ao=ao, shad=shad, convex=convex, concave=concave)


def box_down(a, ss):
    if ss == 1:
        return a
    h, w = a.shape[:2]
    if a.ndim == 2:
        return a.reshape(h // ss, ss, w // ss, ss).mean(axis=(1, 3)).astype(F32)
    return a.reshape(h // ss, ss, w // ss, ss, a.shape[2]).mean(axis=(1, 3)).astype(F32)


def save_rgba(path, lin_rgb, alpha):
    srgb = l2s(np.clip(lin_rgb, 0, 1))
    arr = np.concatenate([srgb, np.clip(alpha, 0, 1)[..., None]], -1)
    Image.fromarray(np.round(arr * 255.0).astype(np.uint8), 'RGBA').save(path)


def scratch_map(X, Y, rng, n, lr, ang_mu, ang_sd, wr, xr, yr, period=None, mask=None):
    out = np.zeros(X.shape, F32)
    for _ in range(n):
        x0 = rng.uniform(*xr)
        y0 = rng.uniform(*yr)
        L = rng.uniform(*lr)
        a = rng.normal(ang_mu, ang_sd)
        w = rng.uniform(*wr)
        dep = rng.uniform(0.45, 1.0)
        bend = rng.normal(0, 0.004)
        dx = X - x0
        dy = Y - y0
        if period is not None:
            dx = (dx + period[0] / 2) % period[0] - period[0] / 2
            if period[1]:
                dy = (dy + period[1] / 2) % period[1] - period[1] / 2
        ca, sa = math.cos(a), math.sin(a)
        al = dx * ca + dy * sa
        ac = -dx * sa + dy * ca - bend * al * al
        tap = np.clip(1 - np.abs(al) / (L / 2), 0, 1) ** 0.6
        prof = np.clip(1 - np.abs(ac) / (w * tap + 1e-3), 0, 1) * (tap > 0)
        out = np.maximum(out, prof * dep)
    if mask is not None:
        out *= mask
    return out


# ----------------------------------------------------------------------------- wood
def wood_surface(Xa, Ya, nz, prm, pal):
    """Flat-sawn board figure. Xa along grain, Ya across grain (px, board centre = 0).
    Returns linear albedo (...,3) and relief height."""
    early, late, hi = [rgb8(*c) for c in pal]
    z = prm['z0'] + prm['zamp'] * np.sin(2 * np.pi * Xa / prm['wl'] + prm['ph']) + prm['zn'] * nz['warp']
    z = np.maximum(np.abs(z), 2.5)
    yy = Ya - prm['pith'] + prm['warp'] * nz['warp2']
    r = np.sqrt(yy * yy + z * z) + prm['fine'] * nz['fine']
    knot_m = np.zeros_like(Xa)
    knot_ring = np.zeros_like(Xa)
    for (kx, ky, ax, ay) in prm.get('knots', ()):
        dx = Xa - kx
        if prm.get('period'):
            dx = np.mod(dx + prm['period'] / 2, prm['period']) - prm['period'] / 2
        dy = Ya - ky
        de = np.sqrt((dx / ax) ** 2 + (dy / ay) ** 2)
        r = r + prm['sp'] * 2.6 * np.exp(-de * de / 2.2)
        m = sstep(1.12, 0.82, de)
        knot_ring = np.where(m > knot_m, de * ay, knot_ring)
        knot_m = np.maximum(knot_m, m)
    rr_ = r / prm['sp']
    rr_ = rr_ + 0.22 * np.sin(2 * np.pi * rr_ / 3.7 + prm['ph'])
    rid = np.floor(rr_).astype(np.float64)
    g = rr_ - rid
    hsh = np.mod(np.sin(rid * 12.9898 + prm['ph'] * 78.233) * 43758.5453, 1.0)
    inten = (prm.get('inten', 0.55) + (1 - prm.get('inten', 0.55)) * hsh).astype(F32)
    fade = 0.70 + 0.30 * sstep(-1.2, 1.2, nz['fade'])
    lw = sstep(0.45, 0.93, g) ** 1.6 * (1 - sstep(0.93, 0.995, g)) * inten * fade
    g2 = np.mod(rr_ * 2.0 + 0.3, 1.0)
    lw = np.maximum(lw, 0.22 * sstep(0.72, 0.95, g2) * (1 - sstep(0.95, 1.0, g2)) * fade)
    fib = nz['fib']
    pore = sstep(1.7, 2.6, nz['pore']) * (0.35 + 0.65 * lw)
    streak = sstep(0.5, 1.9, nz['streak'])
    col = lerp(early, late, (0.88 * lw)[..., None])
    col = lerp(col, hi, (prm.get('streak', 0.55) * streak * (1 - lw))[..., None])
    col = col * ((1 + 0.04 * fib) * (1 - 0.40 * pore) * (1 + prm.get('low', 0.10) * nz['low']))[..., None]
    # stain sits deeper in the latewood: earlywood keeps a little of the natural wood tone
    nat = rgb8(150, 112, 78)
    natm = nat[None, :] * (lum(col) / float(lum(nat)))[..., None]
    col = lerp(col, natm, (prm.get('stain_mix', 0.0) * (1 - lw))[..., None])
    h = 0.24 * lw + 0.03 * fib - 0.16 * pore
    if prm.get('knots'):
        kk = prm.get('knot_k', 1.0)  # strength: < 1 for a quiet knot that does not catch the eye
        kg = np.mod(knot_ring / 1.15, 1.0)
        kl = 0.55 + 0.45 * sstep(0.3, 0.8, kg)
        kcol = late * (0.55 * kl)[..., None]
        core = sstep(0.45, 0.15, knot_ring / 3.0)
        kcol = kcol * (1 - 0.5 * core)[..., None]
        col = lerp(col, kcol, (knot_m * kk)[..., None])
        h = h + knot_m * (0.25 - 0.6 * core) * kk
        # dark halo around the knot
        col = col * (1 - 0.25 * kk * sstep(2.2, 1.0, knot_ring / 3.5 + 0.0) * (1 - knot_m))[..., None]
    return col.astype(F32), h.astype(F32), lw


def wood_noises(h, w, rng, ss):
    return dict(warp=pnoise(h, w, rng, 90 * ss, 30 * ss, 2.0),
                warp2=pnoise(h, w, rng, 40 * ss, 9 * ss, 2.0),
                fine=pnoise(h, w, rng, 14 * ss, 2.0 * ss, 1.6),
                fib=pnoise(h, w, rng, 50 * ss, 0.8 * ss, 1.4),
                pore=pnoise(h, w, rng, 2.5 * ss, 0.5 * ss, 1.0),
                streak=pnoise(h, w, rng, 120 * ss, 5.0 * ss, 1.8),
                low=pnoise(h, w, rng, 90 * ss, 50 * ss, 2.0),
                fade=pnoise(h, w, rng, 40 * ss, 5 * ss, 1.8))


# ============================================================================= panel_wood
def build_panel_wood(out):
    N, ss = 512, 2
    R = N * ss
    rng = np.random.default_rng(4242)
    X, Y = grid(N, N, ss)
    nz = wood_noises(R, R, rng, ss)
    n_grime = pnoise(R, R, rng, 45 * ss, 30 * ss, 1.6)
    n_edge = pnoise(R, R, rng, 5 * ss, 5 * ss, 1.5)
    n_dent = pnoise(R, R, rng, 3 * ss, 3 * ss, 1.6)
    PH = 64
    pidx = np.floor(Y / PH).astype(np.int32)
    yl = Y - pidx * PH
    alb = np.zeros((R, R, 3), F32)
    hgt = np.zeros((R, R), F32)
    jd = np.full((R, R), 1e3, F32)
    joints = []
    for p in range(N // PH):
        rowm = pidx == p
        nj = int(rng.choice([0, 0, 1, 1, 0]))
        js = np.sort(rng.uniform(0, N, size=nj))
        for b in range(max(nj, 1)):
            if nj == 0:
                x0, L = 0.0, float(N)
            else:
                x0 = js[b]
                L = (js[b + 1] - x0) if b + 1 < nj else (js[0] + N - x0)
            xa = np.mod(X - x0, N)
            bm = rowm & (xa < L)
            knots = []
            if rng.uniform() < 0.30 and L > 120:
                knots.append((rng.uniform(40, L - 40), rng.uniform(-14, 14), rng.uniform(6, 9), rng.uniform(3.4, 4.6)))
            wl = rng.uniform(180, 420) if nj else float(N)
            zamp = rng.uniform(4, 14)
            prm = dict(z0=zamp + rng.uniform(8, 30), zamp=zamp, wl=wl,
                       ph=rng.uniform(0, 2 * np.pi), zn=rng.uniform(1.5, 4.0), pith=rng.uniform(-40, 40),
                       warp=rng.uniform(1.5, 3.5), fine=rng.uniform(0.2, 0.45), sp=rng.uniform(3.8, 5.8),
                       knots=knots, period=(N if nj == 0 else None), knot_k=0.3)
            tint = rng.uniform(0.80, 1.16)
            warm = rng.uniform(-0.06, 0.06)
            sub = {k: v[bm] for k, v in nz.items()}
            a, h, _ = wood_surface(xa[bm], (yl - PH / 2)[bm], sub, prm, WOODS['panel'])
            a = a * tint * np.array([1 + warm, 1.0, 1 - warm], F32)
            alb[bm] = a
            cup = 0.35 * ((yl[bm] - PH / 2) / (PH / 2)) ** 2
            hgt[bm] = h + cup + rng.uniform(-0.3, 0.3)
        for j in js:
            joints.append((p, j))
            dj = np.abs(np.mod(X - j + N / 2, N) - N / 2)
            jd = np.where(rowm, np.minimum(jd, dj), jd)
    # plank edges and butt joints: worn rounded edges, dark gaps, chips
    de = np.minimum(yl, PH - yl)
    bw = 2.0 + 1.1 * np.clip(n_edge * 0.6 + 0.5, 0, 1.6)
    edge = -1.9 * (1 - sstep(0.0, bw, de - 0.5)) ** 2
    jedge = -1.6 * (1 - sstep(0.0, bw * 0.8, jd - 0.4)) ** 2
    hgt = hgt + np.minimum(edge, jedge)
    gap = np.maximum(sstep(0.75, 0.35, de), sstep(0.6, 0.25, jd))
    hgt = lerp(hgt, -4.0, gap)
    # chips along plank edges
    chips = np.zeros_like(hgt)
    for _ in range(46):
        cx = rng.uniform(0, N)
        cy = PH * rng.integers(0, N // PH) + rng.choice([-1, 1]) * rng.uniform(0.0, 1.2)
        rr = rng.uniform(1.2, 3.2)
        dx = np.mod(X - cx + N / 2, N) - N / 2
        dy = np.mod(Y - cy + N / 2, N) - N / 2
        d = np.sqrt(dx * dx / 1.8 + dy * dy)
        chips = np.maximum(chips, np.clip(1 - d / rr, 0, 1) ** 0.7)
    hgt -= 1.3 * chips
    # scratches and dents
    sc = scratch_map(X, Y, rng, 60, (8, 70), 0.0, 0.45, (0.35, 0.8), (0, N), (0, N), period=(N, N))
    sc2 = scratch_map(X, Y, rng, 14, (6, 26), 1.2, 0.5, (0.3, 0.6), (0, N), (0, N), period=(N, N))
    sc = np.maximum(sc, sc2)
    hgt -= 0.45 * sc
    dents = sstep(1.9, 2.8, n_dent)
    hgt -= 0.35 * dents
    # colour: grime, seam dirt, scratches expose paler fibres. The large blotches are kept
    # soft and the knots quiet (knot_k above), so the 256 px (logical) repeat of the tile
    # does not show on wide panels as a row of dark spots.
    n_dark = pnoise(R, R, rng, 150 * ss, 9 * ss, 1.8)
    grime = (1 - 0.26 * sstep(-0.4, 1.8, n_grime)) * (1 - 0.20 * sstep(0.2, 1.8, n_dark))
    seam_dirt = 1 - 0.40 * np.exp(-np.maximum(de - 0.4, 0) / 2.6) - 0.30 * np.exp(-np.maximum(jd - 0.3, 0) / 2.2)
    alb = alb * (0.93 * grime * seam_dirt * (1 - 0.25 * dents))[..., None]
    alb = lerp(alb, alb * 1.55 + rgb8(30, 22, 14) * 0.5, (sc * 0.55)[..., None])
    alb = lerp(alb, alb * 0.75, chips[..., None] * 0.6)
    S = Surf((R, R))
    S.put(np.ones((R, R), F32), hgt, wood_mat(alb, gloss=0.30, ks=0.05))
    # nails beside the butt joints
    n_mot = pnoise(R, R, rng, 2 * ss, 2 * ss, 1.5)
    M = metal_mat('iron', n_mot, n_mot * 0.5 + 0.6)
    for (p, j) in joints:
        for sx, yy in ((-4.2, rng.uniform(12, 20)), (4.2, rng.uniform(44, 52))):
            cx = j + sx
            cy = p * PH + yy
            dx = np.mod(X - cx + N / 2, N) - N / 2
            dy = np.mod(Y - cy + N / 2, N) - N / 2
            d = np.hypot(dx, dy)
            rr = 1.9
            stain = np.exp(-(d / 4.5) ** 2) * 0.45
            S.base *= (1 - stain)[..., None]
            t = np.clip(d / rr, 0, 1)
            S.put(cov_sd(d - rr, ss), 0.4 + 1.1 * np.sqrt(np.clip(1 - t * t, 0, 1)), M)
    col, _ = shade(S, ss, wrap=True, shadow_px=6, tan_e=0.7, amb=0.34, kd=0.85, ao_k=(0.22, 0.08))
    col = box_down(soft_clip(col), ss)
    save_rgba(os.path.join(out, 'panel_wood.png'), col, np.ones(col.shape[:2], F32))
    return col


# ============================================================================= shared metalwork
def band_profile(d, T, kind, hb=4.0):
    """Cross-section of a frame band, d = distance from the outer edge (0..T)."""
    x = np.clip(d / T, 0, 1) * 2 - 1
    if kind == 'chamfer':      # forged strap: wide outer bevel, flat top, mid ridge, inner bevel
        h = hb * np.clip(np.minimum(d / 2.3, (T - d) / 1.9), 0, 1) ** 0.85
        h = h + 0.55 * np.exp(-((d - T * 0.47) / 0.55) ** 2) + 0.2 * (1 - x * x)
    elif kind == 'round':      # half-round rail, slightly flattened
        h = hb * np.clip(1 - np.abs(x) ** 2.2, 0, 1) ** 0.6
    elif kind == 'flat':       # heavy flat bar with broad bevels and an incised centre line
        h = hb * np.clip(np.minimum(d / 2.1, (T - d) / 1.8), 0, 1) ** 0.85 + 0.30 * (1 - x * x)
        h = h - 0.55 * np.exp(-((d - T * 0.5) / 0.45) ** 2)
    elif kind == 'bead':       # bevelled band carrying a raised bead
        h = hb * 0.80 * np.clip(np.minimum(d / 2.0, (T - d) / 1.6), 0, 1) ** 0.85
        h = h + 1.45 * np.exp(-((d - T * 0.47) / 0.95) ** 2)
    elif kind == 'iron_bar':   # panel frame: flat forged bar
        h = hb * np.clip(np.minimum(d / 1.6, (T - d) / 1.2), 0, 1) ** 0.8 + 0.45 * (1 - x * x)
    else:
        raise ValueError(kind)
    return h.astype(F32)


def put_ink(S, sd, ink, ss, mask=1.0):
    """A dark rim of width `ink` around the shape sd (keeps the height under it)."""
    if ink > 0:
        S.put(cov_sd(sd - ink, ss) * mask, S.h, INK)


def rivet(S, U, V, cu, cv, r, base_h, dome, mat, ss, mask=1.0, flat=1.12, ink=0.0):
    d = np.hypot(U - cu, V - cv)
    put_ink(S, d - r, ink, ss, mask)
    t = np.clip(d / r, 0, 1)
    h = base_h + dome * np.minimum(1.0, flat * np.sqrt(np.clip(1 - t * t, 0, 1)) ** 0.85)
    S.put(cov_sd(d - r, ss) * mask, h, mat)


def plate(S, sd, h0, h1, bevel, mat, ss, mask=1.0, hn=0.0, kind='round', ink=0.0):
    put_ink(S, sd, ink, ss, mask)
    t = np.clip(-sd / bevel, 0, 1)
    p = np.sqrt(1 - (1 - t) ** 2) if kind == 'round' else t
    S.put(cov_sd(sd, ss) * mask, h0 + (h1 - h0) * p + hn, mat)


def frame_shadow(alpha_frame, height, ss, off=(2.4, 2.0), sigma=2.8, contact=1.1,
                 k_drop=0.80, k_contact=0.55, cap=0.86):
    occ = alpha_frame * np.clip(height / 4.0, 0.5, 1.6)
    sh = ndimage.shift(occ, (off[0] * ss, off[1] * ss), order=1, mode='nearest')
    drop = ndimage.gaussian_filter(sh, sigma * ss, mode='nearest')
    con = ndimage.gaussian_filter(alpha_frame, contact * ss, mode='nearest')
    a = np.clip(k_drop * drop + k_contact * con, 0, cap)
    return a * (1 - alpha_frame)


def finish_frame(S, col, ss, shadow_a, shadow_rgb=(0.010, 0.006, 0.003)):
    af = S.alpha
    srgb = np.asarray(shadow_rgb, F32)
    a = af + shadow_a * (1 - af)
    prem = col * af[..., None] + srgb * (shadow_a * (1 - af))[..., None]
    a_d = box_down(a, ss)
    prem_d = box_down(prem, ss)
    rgb = np.where(a_d[..., None] > 1e-5, prem_d / np.maximum(a_d, 1e-5)[..., None], srgb)
    # bleed: fully transparent texels carry the shadow colour (no dark/bright halos)
    return rgb.astype(F32), a_d


# ============================================================================= frames
def build_panel_frame(out):
    N, ss, B = 160, SS, 12.0
    rng = np.random.default_rng(777)
    X, Y = grid(N, N, ss)
    U = np.minimum(X, N - X)
    V = np.minimum(Y, N - Y)
    D = np.minimum(U, V)
    horiz = V <= U
    along = np.where(horiz, X, Y)
    side = np.where(horiz, np.where(Y < N / 2, 0, 1), np.where(X < N / 2, 2, 3))
    # periodic band tables: 80 px along the side (320 ss px), rows = across + side offset
    TA, TR = 80 * ss, 512
    tabs = dict(mot=pnoise(TR, TA, rng, 8 * ss, 4 * ss, 1.8), wear=pnoise(TR, TA, rng, 5 * ss, 4 * ss, 1.6),
                ham=pnoise(TR, TA, rng, 5 * ss, 4 * ss, 2.2), pit=pnoise(TR, TA, rng, 0.8 * ss, 0.8 * ss, 1.6),
                brush=pnoise(TR, TA, rng, 18 * ss, 0.5 * ss, 1.4))
    ri = (np.floor(D * ss).astype(np.int32) + side * 110) % TR
    ci = np.floor(along * ss).astype(np.int32) % TA
    bt = {k: v[ri, ci] for k, v in tabs.items()}
    # plain 2D noise for the (never tiled) corner pieces
    hs, ws = X.shape
    p_mot = pnoise(hs, ws, rng, 6 * ss, 6 * ss, 1.8)
    p_wear = pnoise(hs, ws, rng, 4 * ss, 4 * ss, 1.6)
    p_ham = pnoise(hs, ws, rng, 4.0 * ss, 4.0 * ss, 2.2)
    p_pit = pnoise(hs, ws, rng, 0.8 * ss, 0.8 * ss, 1.6)

    S = Surf(X.shape)
    Mi = metal_mat('iron', bt['mot'] + 0.25 * bt['brush'], bt['wear'])
    pits = sstep(2.3, 3.0, bt['pit'])
    hn = 0.14 * bt['ham'] - 0.6 * pits + 0.03 * bt['brush']
    Mi['wear'] = np.clip(Mi['wear'] + 0.6 * pits, 0, 1)
    Mi['patch'] = 0.55 * sstep(0.9, 2.2, bt['wear'] + 0.4 * bt['mot'])
    # periodic scratches on the band (per side, wrap 80 along)
    scr = np.zeros(X.shape, F32)
    for s in range(4):
        m = (side == s).astype(F32)
        scr = np.maximum(scr, scratch_map(along, D, rng, 4, (6, 22), 0.0, 0.35, (0.3, 0.55),
                                          (0, 80), (1.5, B - 1.5), period=(80, 0)) * m)
    hb = band_profile(D, B, 'iron_bar', 3.6) + hn - 0.3 * scr
    Mi['base'] = lerp(Mi['base'], np.asarray(METALS['iron']['polish_col'], F32)[None, None, :] * 0.8,
                      (scr * 0.7)[..., None])
    S.put(cov_sd(D - B, ss), hb, Mi)
    # one rivet per 80 px side segment, centred
    for (cu, cv) in ((80.0, B / 2), (B / 2, 80.0)):
        rivet(S, U, V, cu, cv, 4.7, 3.0, 0.6, Mi, ss, flat=1.0)  # burr ring
        rivet(S, U, V, cu, cv, 3.8, 3.6, 2.8, Mi, ss)
    band_alpha, band_h = S.alpha.copy(), S.h.copy()
    # corner: a square brass plate in the corner plus a riveted diamond plate on each band
    # beside it (ref3). Everything, shadows included, stays within 18 px (9 logical) of an
    # outer edge: the command card's slots start 9 px in, and they must not cover or be
    # covered by any of it, whichever is drawn first.
    Mb = metal_mat('agedbrass', p_mot, p_wear)
    phn = 0.10 * p_ham - 0.4 * sstep(2.3, 3.0, p_pit)
    for (du, dv) in ((27.0, 6.0), (6.0, 27.0)):
        sd_dia = np.abs(U - du) + np.abs(V - dv) - 5.8
        sd_dia = (sd_dia / math.sqrt(2)).astype(F32)
        plate(S, sd_dia, 3.6, 5.6, 1.4, Mb, ss, hn=phn, kind='chamfer', ink=0.8)
        S.h -= (0.5 * np.exp(-((sd_dia + 2.0) / 0.4) ** 2) * (sd_dia < -1.0)).astype(F32)
        rivet(S, U, V, du, dv, 1.9, 5.6, 1.5, Mb, ss, ink=0.45)
    sd_sq = sd_box(U, V, 8.9, 8.9, 8.4, 8.4, 2.4)
    plate(S, sd_sq, 4.0, 6.2, 1.7, Mb, ss, hn=phn, ink=0.9)
    eng = np.abs(sd_box(U, V, 8.9, 8.9, 5.6, 5.6, 1.4))
    S.h -= (0.6 * np.exp(-(eng / 0.45) ** 2) * (sd_sq < -1.0)).astype(F32)
    rivet(S, U, V, 8.9, 8.9, 3.6, 6.2, 2.6, Mb, ss, ink=0.6)
    for (cu, cv) in ((15.0, 2.9), (2.9, 15.0)):
        rivet(S, U, V, cu, cv, 1.4, 6.2, 1.1, Mb, ss, ink=0.35)
    S.alpha = np.clip(S.alpha, 0, 1)
    col, _ = shade(S, ss, wrap=False, shadow_px=6, tan_e=0.8)
    col = col * (0.40 + 0.60 * sstep(0.0, 1.0, D))[..., None]
    col = soft_clip(col)
    # the band's own shadow everywhere (it is uniform along the sides); the corner plates'
    # extra shadow only inside the 40x40 corners, faded out before the slice lines so the
    # tiled side segments stay identical from end to end
    sh_band = frame_shadow(band_alpha, band_h, ss)
    sh_full = frame_shadow(S.alpha, S.h, ss)
    wc = (1 - sstep(33.0, 39.0, U)) * (1 - sstep(33.0, 39.0, V))
    sh = lerp(sh_band, np.maximum(sh_band, sh_full), wc) * (1 - S.alpha)
    sh = sh * (1 - sstep(14.5, 18.0, D))  # nothing beyond 18 px (9 logical) of the edge
    rgb, a = finish_frame(S, col, ss, sh)
    save_rgba(os.path.join(out, 'panel_frame.png'), rgb, a)
    return rgb, a


def build_bar_frame(out):
    W, H, ss, B, ST = 160, 48, SS, 10.0, 14.0
    rng = np.random.default_rng(778)
    X, Y = grid(W, H, ss)
    U = np.minimum(X, W - X)
    V = np.minimum(Y, H - Y)
    side = np.where(Y < H / 2, 0, 1)
    TA, TR = 80 * ss, 256
    tabs = dict(mot=pnoise(TR, TA, rng, 8 * ss, 4 * ss, 1.8), wear=pnoise(TR, TA, rng, 5 * ss, 4 * ss, 1.6),
                ham=pnoise(TR, TA, rng, 5 * ss, 4 * ss, 2.2), pit=pnoise(TR, TA, rng, 0.8 * ss, 0.8 * ss, 1.6),
                brush=pnoise(TR, TA, rng, 18 * ss, 0.5 * ss, 1.4))
    ri = (np.floor(V * ss).astype(np.int32) + side * 110) % TR
    ci = np.floor(X * ss).astype(np.int32) % TA
    bt = {k: v[ri, ci] for k, v in tabs.items()}
    # strap noise: 2D, but frozen (uniform in y) over the stretched rows 20..28
    hs, ws = X.shape
    yq = np.where(Y < 20, Y, np.where(Y > 28, Y - 8, 20.0))
    yi = np.clip(np.floor(yq * ss).astype(np.int32), 0, hs - 1)
    xi = np.clip(np.floor(X * ss).astype(np.int32), 0, ws - 1)

    def frozen(n):
        return n[yi, xi]
    s_mot = frozen(pnoise(hs, ws, rng, 6 * ss, 6 * ss, 1.8))
    s_wear = frozen(pnoise(hs, ws, rng, 4 * ss, 4 * ss, 1.6))
    s_ham = frozen(pnoise(hs, ws, rng, 4.0 * ss, 4.0 * ss, 2.2))
    s_pit = frozen(pnoise(hs, ws, rng, 0.8 * ss, 0.8 * ss, 1.6))

    S = Surf(X.shape)
    Mi = metal_mat('iron', bt['mot'] + 0.25 * bt['brush'], bt['wear'])
    pits = sstep(2.3, 3.0, bt['pit'])
    Mi['wear'] = np.clip(Mi['wear'] + 0.6 * pits, 0, 1)
    Mi['patch'] = 0.55 * sstep(0.9, 2.2, bt['wear'] + 0.4 * bt['mot'])
    scr = np.zeros(X.shape, F32)
    for s in range(2):
        m = (side == s).astype(F32)
        scr = np.maximum(scr, scratch_map(X, V, rng, 4, (6, 22), 0.0, 0.35, (0.3, 0.55),
                                          (0, 80), (1.5, B - 1.5), period=(80, 0)) * m)
    hb = band_profile(V, B, 'iron_bar', 3.4) + 0.14 * bt['ham'] - 0.6 * pits - 0.3 * scr
    Mi['base'] = lerp(Mi['base'], np.asarray(METALS['iron']['polish_col'], F32)[None, None, :] * 0.8,
                      (scr * 0.7)[..., None])
    S.put(cov_sd(V - B, ss), hb, Mi)
    # brass rivets on the bands, one centred in each 80 px side segment (every 40 logical px)
    Mr = metal_mat('agedbrass', bt['mot'], bt['wear'])
    rivet(S, U, V, 80.0, B / 2, 2.7, 3.4, 2.0, Mr, ss, ink=0.5)
    # brass end strap, uniform through the stretched middle rows
    Mb = metal_mat('agedbrass', s_mot, s_wear)
    shn = 0.10 * s_ham - 0.4 * sstep(2.3, 3.0, s_pit)
    xs = np.clip(U / ST, 0, 1) * 2 - 1
    hstrap = 3.6 + 1.8 * np.clip(np.minimum(U / 1.5, (ST - U) / 1.2), 0, 1) + 0.4 * (1 - xs * xs)
    hstrap = hstrap - 0.5 * np.exp(-((U - 3.0) / 0.45) ** 2) - 0.5 * np.exp(-((U - (ST - 3.0)) / 0.45) ** 2)
    put_ink(S, U - ST, 0.8, ss)
    S.put(cov_sd(U - ST, ss), hstrap + shn, Mb)
    # small riveted diamond on the band beside each end strap (matches the panel corners)
    sdd = sd_polygon(U, V, [(19.0, 5.0), (24.5, -0.5), (30.0, 5.0), (24.5, 10.5)])
    plate(S, sdd, 3.6, 5.2, 1.3, Mb, ss, hn=shn, kind='chamfer', ink=0.7)
    rivet(S, U, V, 24.5, 5.0, 1.7, 5.2, 1.3, Mb, ss, ink=0.4)
    rivet(S, U, V, ST / 2, 9.5, 3.6, 5.0, 2.4, Mb, ss, ink=0.5)
    col, _ = shade(S, ss, wrap=False, shadow_px=6, tan_e=0.8)
    col = col * (0.40 + 0.60 * sstep(0.0, 1.0, np.minimum(U, V)))[..., None]
    col = soft_clip(col)
    sh = frame_shadow(S.alpha, S.h, ss, off=(1.8, 1.8), sigma=2.0, contact=0.9)
    rgb, a = finish_frame(S, col, ss, sh)
    # the left / right side segments (rows 20..27, x < 40 and x >= 120) are tiled every
    # 4 logical px by the engine: build them from one repeated row
    for xs_ in (slice(0, 40), slice(120, 160)):
        rgb[20:28, xs_] = rgb[24, xs_][None]
        a[20:28, xs_] = a[24, xs_][None]
    save_rgba(os.path.join(out, 'bar_frame.png'), rgb, a)
    return rgb, a


# ============================================================================= buttons
# bezel: hotkey / count plaque silhouette (round / oct / hex); corner: ornament family
SKINS = {
    'command': dict(metal='steel', wood='blue', band=7.2, prof='chamfer', corner='bracket', bezel='round',
                    gem=(0.008, 0.026, 0.115), seed=11, orn=1.15),
    'stance': dict(metal='gunmetal', wood='slate', band=7.2, prof='chamfer', corner='cap', bezel='oct',
                   gem=(0.040, 0.014, 0.075), seed=12, orn=2.0,
                   wood_prm=dict(z0=(70, 110), inten=0.55, streak=0.25, low=0.04, stain_mix=0.06, knots=False),
                   grime=0.06),
    'economy': dict(metal='brass', wood='honey', band=7.2, prof='round', corner='fan', bezel='round',
                    gem=(0.005, 0.055, 0.022), seed=13),
    'military': dict(metal='blackiron', wood='crimson', band=7.2, prof='chamfer', corner='spike', bezel='oct',
                     gem=(0.095, 0.006, 0.004), seed=14),
    'defence': dict(metal='silver', wood='ash', band=7.3, prof='flat', corner='bolt', bezel='hex',
                    gem=(0.006, 0.042, 0.065), seed=15),
    'train': dict(metal='bronze', wood='green', band=7.2, prof='round', corner='leaf', bezel='round',
                  gem=(0.075, 0.026, 0.003), seed=16, orn=1.15),
    'research': dict(metal='gold', wood='purple', band=7.2, prof='bead', corner='filigree', bezel='round',
                     gem=(0.048, 0.010, 0.095), seed=17),
    'upgrade': dict(metal='redgold', wood='amber', band=7.3, prof='bead', corner='flourish', bezel='round',
                    gem=(0.105, 0.008, 0.014), seed=18),
    'nav': dict(metal='darkiron', wood='walnut', band=7.0, prof='flat', corner='rivet', bezel='oct',
                gem=(0.016, 0.016, 0.020), seed=19, orn=1.6),
    'plain': dict(metal='dullbronze', wood='brown', band=7.3, prof='round', corner='smallplate', bezel='round',
                  gem=(0.050, 0.024, 0.008), seed=20, band_polish=1.0, orn=1.3),
}
SKIN_ORDER = ['command', 'stance', 'economy', 'military', 'defence', 'train', 'research', 'upgrade', 'nav', 'plain']


def tube(S, U, V, pts, r, base_h, mat, ss, mask, end_ball=0.0, taper=0.0, ink=0.0):
    d, t = polyline_dist(U, V, pts)
    rr = r * (1 - taper * t)
    put_ink(S, d - rr, ink, ss, mask)
    if end_ball > 0:
        ex, ey = pts[-1]
        put_ink(S, np.hypot(U - ex, V - ey) - end_ball, ink, ss, mask)
    tt = np.clip(d / np.maximum(rr, 1e-3), 0, 1)
    h = base_h + rr * 1.05 * np.sqrt(np.clip(1 - tt * tt, 0, 1))
    S.put(cov_sd(d - rr, ss) * mask, h, mat)
    if end_ball > 0:
        ex, ey = pts[-1]
        rivet(S, U, V, ex, ey, end_ball, base_h, end_ball * 1.05, mat, ss, mask)


def spike(S, U, V, tip, base, hw, h0, h1, mat, ss, mask, hn=0.0, ink=0.0):
    """Four-faced forged spike: tip at `tip`, widest (hw) at `base`, sharp ridge along its axis."""
    tx, ty = tip
    bx, by = base
    L = math.hypot(bx - tx, by - ty)
    ax, ay = (bx - tx) / L, (by - ty) / L
    a = (U - tx) * ax + (V - ty) * ay
    lat = -(U - tx) * ay + (V - ty) * ax
    s = np.clip(a / L, 0, 1)
    w = hw * s ** 0.8
    inside_a = (a > -0.3) & (a < L)
    sd = np.where(inside_a, np.abs(lat) - w, 9.0)
    put_ink(S, sd, ink, ss, mask)
    ridge = np.clip(1 - np.abs(lat) / np.maximum(w, 1e-3), 0, 1)
    h = h0 + (h1 - h0) * (0.30 + 0.70 * s) * ridge ** 0.75
    S.put(cov_sd(sd, ss) * mask, h + hn, mat)


def leaf(S, U, V, a, b, hw, h0, h1, mat, ss, mask, hn=0.0, ink=0.0, vein=0.5, shape=0.8, skew=0.0):
    """Pointed leaf / petal from a to b, widest in the middle, with an engraved midrib."""
    ax_, ay_ = a
    bx_, by_ = b
    L = math.hypot(bx_ - ax_, by_ - ay_)
    dx, dy = (bx_ - ax_) / L, (by_ - ay_) / L
    s = (U - ax_) * dx + (V - ay_) * dy
    lat = -(U - ax_) * dy + (V - ay_) * dx
    t = np.clip(s / L, 0, 1)
    lat = lat - skew * L * np.sin(np.pi * t) * 0.15
    w = hw * np.clip(np.sin(np.pi * t ** 0.85), 0, 1) ** shape
    inside = (s > 0) & (s < L)
    sd = np.where(inside, np.abs(lat) - w, 9.0)
    put_ink(S, sd, ink, ss, mask)
    prof = np.clip(1 - np.abs(lat) / np.maximum(w, 1e-3), 0, 1)
    h = h0 + (h1 - h0) * prof ** 0.7 - vein * np.exp(-(lat / 0.45) ** 2) * (t > 0.08) * (t < 0.9)
    S.put(cov_sd(sd, ss) * mask, h + hn, mat)


def hexbolt(S, U, V, cu, cv, inr, h0, M, ss, mask, ink=0.6):
    rivet(S, U, V, cu, cv, inr * 1.42, h0, 0.6, M, ss, mask, flat=1.0, ink=ink)   # washer
    plate(S, sd_hex(U, V, cu, cv, inr), h0 + 0.6, h0 + 2.6, 1.0, M, ss, mask, kind='chamfer', ink=0.5)
    S.h -= (0.5 * np.exp(-(np.hypot(U - cu, V - cv) / 0.9) ** 2) * mask).astype(F32)


def lstrap(S, U, V, length, tu, tv, h0, h1, M, ss, mask, hn, ink=0.8, r=1.4, bevel=1.4):
    """L-shaped strap riding on both bands of a corner: arms `length` long, `tv` thick along
    the horizontal band and `tu` thick along the vertical one."""
    sd = np.minimum(sd_box(U, V, length / 2, tv / 2, length / 2, tv / 2, r),
                    sd_box(U, V, tu / 2, length / 2, tu / 2, length / 2, r))
    plate(S, sd, h0, h1, bevel, M, ss, mask, hn, kind='chamfer', ink=ink)


def pyramid_stud(S, U, V, cu, cv, half, h0, h1, M, ss, mask, hn, ink=0.7):
    """Four-faced forged stud: one face catches the light, one falls in shadow."""
    sd = sd_box(U, V, cu, cv, half, half, 0.4)
    put_ink(S, sd, ink, ss, mask)
    t = np.clip(1 - np.maximum(np.abs(U - cu), np.abs(V - cv)) / half, 0, 1)
    S.put(cov_sd(sd, ss) * mask, h0 + (h1 - h0) * t + hn, M)


def corner_top(kind, S, U, V, M, Mo, ss, mask, hn):
    """Top corner ornament. The plaque's dark well (SOCK) is drawn over it afterwards, so
    only the outer strips stay visible: the top band above the well (V < 5) and the left
    band beside it (U < 6), including the outer tip."""
    if kind == 'bracket':
        lstrap(S, U, V, 30.0, 7.0, 6.2, 3.4, 5.4, M, ss, mask, hn)
        for (cu, cv) in ((3.3, 3.1), (27.0, 2.7), (2.9, 27.0)):
            rivet(S, U, V, cu, cv, 1.6, 5.4, 1.4, M, ss, mask, ink=0.45)
    elif kind == 'cap':
        lstrap(S, U, V, 27.0, 7.0, 6.2, 3.3, 5.0, M, ss, mask, hn)
        pyramid_stud(S, U, V, 3.6, 3.4, 3.4, 5.0, 7.4, M, ss, mask, hn)
        for (cu, cv) in ((23.5, 2.7), (2.9, 23.5)):
            rivet(S, U, V, cu, cv, 1.4, 5.0, 1.2, M, ss, mask, ink=0.4)
    elif kind == 'fan':
        rivet(S, U, V, 3.4, 3.2, 2.5, 4.4, 1.9, M, ss, mask, ink=0.5)
        for p in (11.0, 18.5, 26.0):
            rivet(S, U, V, p, 2.7, 1.55, 4.4, 1.3, M, ss, mask, ink=0.45)
            rivet(S, U, V, 2.9, p, 1.55, 4.4, 1.3, M, ss, mask, ink=0.45)
    elif kind == 'spike':
        lstrap(S, U, V, 31.0, 7.0, 6.2, 3.2, 4.8, M, ss, mask, hn, ink=0.7)
        spike(S, U, V, (0.3, 0.3), (8.5, 8.0), 4.4, 4.8, 8.4, Mo, ss, mask, hn, ink=0.7)
        for p_ in (17.5, 25.5):
            pyramid_stud(S, U, V, p_, 2.7, 1.8, 4.8, 7.0, Mo, ss, mask, hn, ink=0.45)
            pyramid_stud(S, U, V, 2.9, p_, 1.8, 4.8, 7.0, Mo, ss, mask, hn, ink=0.45)
    elif kind == 'bolt':
        lstrap(S, U, V, 30.0, 7.0, 6.2, 3.5, 5.6, M, ss, mask, hn, bevel=1.6)
        hexbolt(S, U, V, 3.4, 3.2, 2.0, 5.6, M, ss, mask, ink=0.45)
        for (cu, cv) in ((26.0, 2.7), (2.9, 26.0)):
            hexbolt(S, U, V, cu, cv, 1.25, 5.6, M, ss, mask, ink=0.35)
    elif kind == 'leaf':
        leaf(S, U, V, (8.5, 2.7), (31.4, 2.0), 2.1, 4.4, 6.2, M, ss, mask, hn, ink=0.55, vein=0.25)
        leaf(S, U, V, (2.9, 8.5), (2.2, 31.4), 2.1, 4.4, 6.2, M, ss, mask, hn, ink=0.55, vein=0.25)
        leaf(S, U, V, (8.0, 7.5), (0.4, 0.4), 3.0, 4.8, 7.4, M, ss, mask, hn, ink=0.7)
    elif kind == 'filigree':
        sp = spiral_pts(27.6, 2.6, 2.1, 0.6, math.pi, 0.95, 40, 1.0)
        tube(S, U, V, np.concatenate([[[8.0, 2.6]], sp]), 0.95, 4.2, M, ss, mask, end_ball=1.0, ink=0.45)
        tube(S, U, V, np.concatenate([[[2.8, 8.0]], sp[:, ::-1] + np.array([0.2, 0.0])]), 0.95, 4.2, M, ss,
             mask, end_ball=1.0, ink=0.45)
        rr = np.hypot(U - 3.4, V - 3.2)
        th = np.arctan2(V - 3.2, U - 3.4)
        plate(S, rr - (2.9 + 0.6 * np.cos(6 * th)), 4.0, 5.8, 1.0, M, ss, mask, hn, ink=0.5)
        rivet(S, U, V, 3.4, 3.2, 1.3, 5.8, 1.1, M, ss, mask, ink=0.3)
    elif kind == 'flourish':
        leaf(S, U, V, (12.0, 2.6), (31.4, 1.9), 2.1, 4.4, 6.2, M, ss, mask, hn, ink=0.5, vein=0.25)
        leaf(S, U, V, (2.8, 12.0), (2.0, 31.4), 2.1, 4.4, 6.2, M, ss, mask, hn, ink=0.5, vein=0.25)
        leaf(S, U, V, (8.4, 7.8), (0.3, 0.3), 3.3, 4.8, 8.0, M, ss, mask, hn, ink=0.7)
        leaf(S, U, V, (5.5, 3.0), (12.5, 0.9), 1.4, 4.6, 6.2, M, ss, mask, hn, ink=0.45, vein=0.2, skew=-0.5)
        leaf(S, U, V, (3.0, 5.5), (0.9, 12.5), 1.4, 4.6, 6.2, M, ss, mask, hn, ink=0.45, vein=0.2, skew=0.5)
    elif kind == 'rivet':
        plate(S, sd_box(U, V, 3.8, 3.6, 3.8, 3.6, 1.0), 3.2, 4.8, 1.2, M, ss, mask, hn, kind='chamfer', ink=0.6)
        rivet(S, U, V, 3.5, 3.3, 2.0, 4.8, 1.6, M, ss, mask, ink=0.4)
    elif kind == 'smallplate':
        rivet(S, U, V, 3.6, 3.4, 3.4, 3.6, 0.9, M, ss, mask, flat=1.0, ink=0.55)
        rivet(S, U, V, 3.6, 3.4, 1.8, 4.5, 1.5, M, ss, mask, ink=0.4)
    else:
        raise ValueError(kind)


def corner_bottom(kind, S, U, V, M, Mo, ss, mask, hn):
    """Bottom corner ornament: a chunky piece in the ORN_SQ corner square plus straps or
    small pieces on the bands (bottom arm < ORN_ARM_B thick, side arm < ORN_ARM_S)."""
    if kind == 'bracket':
        lstrap(S, U, V, 29.5, 10.6, 9.2, 3.4, 6.0, M, ss, mask, hn, ink=0.85, r=1.6, bevel=1.6)
        plate(S, sd_box(U, V, 7.8, 7.8, 7.6, 7.6, 1.6), 4.0, 7.2, 1.9, M, ss, mask, hn, kind='chamfer', ink=0.9)
        rivet(S, U, V, 7.8, 7.8, 3.7, 7.2, 2.8, M, ss, mask, ink=0.6)
        for (cu, cv) in ((23.0, 4.6), (5.3, 23.0)):
            rivet(S, U, V, cu, cv, 2.3, 6.0, 2.0, M, ss, mask, ink=0.55)
    elif kind == 'cap':
        lstrap(S, U, V, 28.0, 10.2, 8.8, 3.3, 5.4, M, ss, mask, hn, ink=0.8)
        plate(S, sd_box(U, V, 7.8, 7.8, 7.6, 7.6, 1.0), 3.6, 6.0, 1.6, M, ss, mask, hn, kind='chamfer', ink=0.9)
        pyramid_stud(S, U, V, 7.8, 7.8, 5.6, 6.0, 11.0, M, ss, mask, hn, ink=0.55)
        for (cu, cv) in ((22.0, 4.4), (5.1, 22.0)):
            rivet(S, U, V, cu, cv, 2.0, 5.4, 1.7, M, ss, mask, ink=0.5)
    elif kind == 'fan':
        lstrap(S, U, V, 30.0, 10.0, 9.0, 3.0, 4.8, M, ss, mask, hn, ink=0.75, r=2.0)
        r = np.hypot(U, V)
        sd = r - 15.8
        plate(S, sd, 3.6, 6.4, 2.0, M, ss, mask, hn, ink=0.95)
        S.h -= (0.8 * np.exp(-((r - 11.8) / 0.55) ** 2) * (sd < -1) * mask).astype(F32)
        for ang in (16.0, 45.0, 74.0):
            a = math.radians(ang)
            rivet(S, U, V, 8.0 * math.cos(a), 8.0 * math.sin(a), 2.1, 6.4, 1.8, M, ss, mask, ink=0.5)
        for (cu, cv) in ((24.6, 4.5), (5.0, 24.6)):
            rivet(S, U, V, cu, cv, 2.3, 4.8, 2.0, M, ss, mask, ink=0.55)
    elif kind == 'spike':
        lstrap(S, U, V, 30.0, 10.2, 8.8, 3.2, 5.2, M, ss, mask, hn, ink=0.85, r=1.2)
        plate(S, sd_box(U, V, 7.2, 7.2, 7.0, 7.0, 1.0), 3.6, 5.8, 1.4, M, ss, mask, hn, kind='chamfer', ink=0.85)
        spike(S, U, V, (0.4, 0.4), (14.8, 14.8), 7.2, 5.6, 12.5, Mo, ss, mask, hn, ink=1.0)
        for p_ in (20.5, 27.0):
            pyramid_stud(S, U, V, p_, 4.4, 2.5, 5.2, 8.4, Mo, ss, mask, hn, ink=0.55)
            pyramid_stud(S, U, V, 5.0, p_, 2.5, 5.2, 8.4, Mo, ss, mask, hn, ink=0.55)
    elif kind == 'bolt':
        lstrap(S, U, V, 30.0, 10.6, 9.2, 3.4, 5.8, M, ss, mask, hn, ink=0.9, bevel=1.6)
        plate(S, sd_box(U, V, 7.8, 7.8, 7.6, 7.6, 1.4), 3.8, 6.4, 1.7, M, ss, mask, hn, kind='chamfer', ink=0.95)
        hexbolt(S, U, V, 7.8, 7.8, 4.8, 6.4, M, ss, mask, ink=0.65)
        for (cu, cv) in ((23.6, 4.6), (5.3, 23.6)):
            hexbolt(S, U, V, cu, cv, 2.4, 5.8, M, ss, mask, ink=0.5)
    elif kind == 'leaf':
        leaf(S, U, V, (8.0, 4.5), (30.8, 3.6), 3.2, 4.4, 6.8, M, ss, mask, hn, ink=0.65, vein=0.3)
        leaf(S, U, V, (4.5, 8.0), (3.8, 30.8), 3.2, 4.4, 6.8, M, ss, mask, hn, ink=0.65, vein=0.3)
        leaf(S, U, V, (15.6, 15.6), (0.5, 0.5), 6.3, 5.0, 9.6, M, ss, mask, hn, ink=1.0)
        rivet(S, U, V, 12.6, 12.6, 2.6, 7.0, 2.0, M, ss, mask, ink=0.55)
    elif kind == 'filigree':
        rr = np.hypot(U - 7.6, V - 7.6)
        th = np.arctan2(V - 7.6, U - 7.6)
        plate(S, rr - (6.4 + 1.3 * np.cos(6 * th)), 3.8, 6.4, 1.3, M, ss, mask, hn, ink=0.9)
        rivet(S, U, V, 7.6, 7.6, 2.9, 6.4, 2.1, M, ss, mask, ink=0.5)
        sp1 = np.concatenate([[[14.0, 4.6]], spiral_pts(24.3, 4.8, 3.7, 0.9, math.pi, 1.1, 70, 1.0)])
        sp2 = sp1[:, ::-1] + np.array([0.6, 0.0])
        tube(S, U, V, sp1, 1.5, 4.2, M, ss, mask, end_ball=1.6, ink=0.65)
        tube(S, U, V, sp2, 1.5, 4.2, M, ss, mask, end_ball=1.6, ink=0.65)
    elif kind == 'flourish':
        sp = spiral_pts(24.4, 4.8, 3.4, 0.9, math.pi, 1.0, 60, 1.0)
        tube(S, U, V, np.concatenate([[[12.0, 5.2]], sp]), 1.45, 4.4, M, ss, mask, end_ball=1.5, ink=0.6)
        tube(S, U, V, np.concatenate([[[5.2, 12.0]], sp[:, ::-1] + np.array([0.6, 0.0])]), 1.45, 4.4, M, ss, mask,
             end_ball=1.5, ink=0.6)
        leaf(S, U, V, (15.2, 15.2), (0.4, 0.4), 4.9, 5.0, 9.4, M, ss, mask, hn, ink=0.95)
        leaf(S, U, V, (9.4, 6.8), (18.5, 1.4), 2.3, 4.6, 7.0, M, ss, mask, hn, ink=0.65, vein=0.25, skew=-0.6)
        leaf(S, U, V, (6.8, 9.4), (1.4, 18.5), 2.3, 4.6, 7.0, M, ss, mask, hn, ink=0.65, vein=0.25, skew=0.6)
        # amber cabochon in a gold collar
        r = np.hypot(U - 6.8, V - 6.8)
        rg = 3.4
        tg = np.clip(r / rg, 0, 1)
        gb = np.asarray((0.30, 0.07, 0.004), F32)[None, None, :] * (0.5 + 1.2 * tg ** 2)[..., None]
        put_ink(S, r - rg - 1.0, 0.55, ss, mask)
        S.put(cov_sd(r - rg - 1.0, ss) * mask, 7.4 + 0.0 * r, M)
        S.put(cov_sd(r - rg, ss) * mask, 7.2 + 2.2 * np.sqrt(np.clip(1 - tg * tg, 0, 1)), gem_mat(gb, 0.6, 0.7))
    elif kind == 'rivet':
        lstrap(S, U, V, 27.0, 10.2, 8.8, 3.2, 5.2, M, ss, mask, hn, ink=0.8)
        plate(S, sd_box(U, V, 7.4, 7.4, 7.2, 7.2, 1.0), 3.6, 6.0, 1.5, M, ss, mask, hn, kind='chamfer', ink=0.9)
        rivet(S, U, V, 7.4, 7.4, 3.6, 6.0, 2.7, M, ss, mask, ink=0.55)
        for (cu, cv) in ((20.5, 4.4), (5.1, 20.5)):
            rivet(S, U, V, cu, cv, 2.1, 5.2, 1.8, M, ss, mask, ink=0.5)
    elif kind == 'smallplate':
        rivet(S, U, V, 7.8, 7.8, 7.8, 3.6, 1.6, M, ss, mask, flat=1.0, ink=0.95)
        rivet(S, U, V, 7.8, 7.8, 3.6, 5.2, 2.7, M, ss, mask, ink=0.55)
        for (cu, cv) in ((21.5, 4.5), (5.0, 21.5)):
            rivet(S, U, V, cu, cv, 3.4, 3.6, 1.0, M, ss, mask, flat=1.0, ink=0.65)
            rivet(S, U, V, cu, cv, 1.7, 4.6, 1.5, M, ss, mask, ink=0.45)
    else:
        raise ValueError(kind)


def plaque(S, U, V, X, Y, cfg, M, ss, mask, hn):
    """The plaque's dark well (SOCK) and the plaque in it: a dark glass gem in a blackened,
    ink-outlined bezel. The engine prints a 14 px hotkey / count glyph over it, so the whole
    well stays dark; only the gem's colour and a faint rim light show around the glyph."""
    sdk = sd_socket(U, V, cfg['bezel'])
    # well: a lip of blackened family metal sloping down to a near-black enamel floor
    Mlip = darker(M, 0.16, polish=0.35, gloss=0.4)
    Mlip['rough'] = 0.6
    t = np.clip(-sdk / 1.5, 0, 1)
    hw = 4.0 - 2.8 * np.sqrt(1 - (1 - t) ** 2)
    S.put(cov_sd(sdk, ss) * mask, hw, Mlip)
    Wm = dict(WELL)
    Wm['base'] = np.asarray(cfg['gem'], F32) * 0.30 + 0.004   # deep enamel in the gem's colour
    S.put(cov_sd(sdk + 1.0, ss) * mask, hw, Wm)
    # plaque
    cx, cy = PLAQ_C[0]
    R = PLAQ_R
    shape = cfg['bezel']
    if shape == 'round':
        sd_o = sd_circle(U, V, cx, cy, R)
    elif shape == 'oct':
        sd_o = sd_polygon(U, V, ngon(cx, cy, R, 8, math.pi / 8))
    else:  # hex, flat top (a bolt head)
        sd_o = sd_polygon(U, V, ngon(cx, cy, R, 6, 0.0))
    ink_o, bez = 0.8, 1.7
    inr = R if shape == 'round' else R * math.cos(math.pi / (8 if shape == 'oct' else 6))
    S.put(cov_sd(sd_o, ss) * mask, 1.4 + 0.0 * sd_o, INK)
    # bezel: the family metal, blackened and dulled, low rounded crown
    Mz = darker(M, 0.13, polish=0.45, gloss=0.45)
    Mz['rough'] = 0.5
    sdb = sd_o + ink_o
    t = np.clip(-sdb / bez, 0, 1)
    hbz = 1.6 + 1.1 * np.clip(np.sin(np.pi * np.clip(0.12 + 0.88 * t, 0, 1)), 0, 1) ** 0.6
    S.put(cov_sd(sdb, ss) * mask, hbz + 0.2 * hn, Mz)
    # inner ink line + gem
    sds = sdb + bez
    S.put(cov_sd(sds, ss) * mask, 1.6 + 0.0 * sds, INK)
    sdg = sds + 0.6
    dxp = X - np.where(X < BTN_W / 2, PLAQ_C[0][0], PLAQ_C[1][0])
    dyp = Y - cy
    Rg = inr - ink_o - bez - 0.6  # inscribed radius of the gem
    rr = np.clip(np.hypot(dxp, dyp) / Rg, 0, 1)
    hg = 1.7 + 1.1 * np.sqrt(np.clip(1 - rr * rr, 0, 1))
    proj = (dxp * 0.62 + dyp * 0.78) / Rg
    glow = 0.40 + 1.6 * sstep(-0.1, 1.0, proj) * sstep(0.35, 1.0, rr)
    base = np.asarray(cfg['gem'], F32)[None, None, :] * glow[..., None]
    S.put(cov_sd(sdg, ss) * mask, hg, gem_mat(base, spec=0.12, refl=0.30))


def build_button(name, wood_gain=1.0):
    cfg = SKINS[name]
    W, H, ss = BTN_W, BTN_H, SS
    rng = np.random.default_rng(cfg['seed'] * 1000 + 7)
    X, Y = grid(W, H, ss)
    U = np.minimum(X, W - X)
    V = np.minimum(Y, H - Y)
    top = (Y < H / 2).astype(F32)
    bot = 1.0 - top
    D = np.minimum(U, V)
    hs, ws = X.shape
    T = cfg['band']
    wp = cfg.get('wood_prm', {})
    # ---- wood board
    nz = wood_noises(hs, ws, rng, ss)
    n_grime = pnoise(hs, ws, rng, 20 * ss, 14 * ss, 1.6)
    knots = []
    if rng.uniform() < 0.5:
        knots.append((rng.choice([rng.uniform(30, 55), rng.uniform(115, 140)]), rng.uniform(-30, 30),
                      rng.uniform(5.5, 8), rng.uniform(3.0, 4.2)))
    if wp.get('knots', True) is False:
        knots = []
    z0r = wp.get('z0', (22, 40))
    prm = dict(z0=rng.uniform(*z0r), zamp=rng.uniform(6, 13), wl=rng.uniform(150, 280),
               ph=rng.uniform(0, 2 * np.pi), zn=3.0, pith=rng.choice([-1, 1]) * rng.uniform(0, 80),
               warp=2.0, fine=0.3, sp=rng.uniform(5.0, 6.8), knots=knots, inten=wp.get('inten', 0.70),
               stain_mix=wp.get('stain_mix', 0.22), streak=wp.get('streak', 0.55), low=wp.get('low', 0.10))
    alb, wh, lw = wood_surface(X, Y - H / 2, nz, prm, WOODS[cfg['wood']])
    din = D - T
    vig = 0.50 + 0.50 * sstep(0.0, 26.0, din)
    midband = 1.0 + 0.10 * np.exp(-((Y - H * 0.5) / 24.0) ** 2)
    grime = 1 - cfg.get('grime', 0.22) * sstep(-0.3, 1.6, n_grime) * (1 - 0.6 * sstep(6, 30, din))
    # soft shade on the wood just under each plaque well, where the bottom of a hotkey /
    # count glyph's ring falls (fades out before x = 32 and by y = 40)
    vig = vig * (1 - 0.55 * top * sstep(40.0, 32.5, V) * sstep(3.0, 9.0, U) * (1 - sstep(25.0, 31.5, U)))
    sc = scratch_map(X, Y, rng, 9, (8, 34), 0.25, 0.6, (0.35, 0.7), (20, W - 20), (24, H - 24))
    alb = alb * (vig * midband * grime)[..., None] * wood_gain
    alb = lerp(alb, alb * 1.45 + rgb8(24, 20, 16) * 0.4 * wood_gain, (sc * 0.5)[..., None])
    wh = wh - 0.4 * sc - 1.0 * (1 - sstep(0.0, 1.6, din))
    S = Surf(X.shape)
    S.put(np.ones(X.shape, F32), wh, wood_mat(alb, gloss=0.34, ks=0.06))
    # ---- metal band
    n_mot = pnoise(hs, ws, rng, 6 * ss, 6 * ss, 1.8)
    n_wear = pnoise(hs, ws, rng, 4 * ss, 4 * ss, 1.6)
    n_ham = pnoise(hs, ws, rng, 4.5 * ss, 4.5 * ss, 2.2)
    n_pit = pnoise(hs, ws, rng, 0.8 * ss, 0.8 * ss, 1.6)
    Mm = METALS[cfg['metal']]
    M = metal_mat(cfg['metal'], n_mot, n_wear)
    pits = sstep(2.3, 3.0, n_pit)
    M['wear'] = np.clip(M['wear'] + 0.6 * pits * Mm['pit'], 0, 1)
    hn = Mm['ham'] * 0.12 * n_ham - Mm['pit'] * 0.6 * pits
    hb = band_profile(D, T, cfg['prof'], 4.0) + hn
    Mband = dict(M)
    Mband['polish'] = M['polish'] * cfg.get('band_polish', 0.6)  # ornaments stay brighter than the band
    if name == 'research':  # engraved scrollwork running along the bands
        along = np.where(V <= U, X, Y)
        dd = D - T * 0.5 - 1.25 * np.sin(2 * np.pi * along / 13.0)
        hb = hb - 0.55 * np.exp(-(dd / 0.42) ** 2) * sstep(0.8, 2.0, D) * sstep(0.8, 2.0, T - D)
    put_ink(S, D - T, 0.7, ss)          # dark seam between the band and the wood
    S.put(cov_sd(D - T, ss), hb, Mband)
    base = S.copy()
    S.cov[:] = 0.0
    # ---- corner ornaments (inside the 32x32 corners) and the two plaques. The ornaments are
    # the family metal lifted by `orn` (dark metals get brighter, polished pieces) so they
    # stand out from the band and the wood even in greyscale
    Mc = dict(M)
    Mc['base'] = np.asarray(M['base'], F32) * cfg.get('orn', 1.0)
    Mc['polish'] = 1.0
    Mo = Mc
    if cfg['corner'] == 'spike':  # spikes: blackened iron with bright polished ridges
        Mo = dict(Mc)
        Mo['polish_col'] = np.asarray((0.74, 0.74, 0.78), F32)
        Mo['wear'] = M['wear'] * 0.5
    corner_bottom(cfg['corner'], S, U, V, Mc, Mo, ss, bot, hn)
    corner_top(cfg['corner'], S, U, V, Mc, Mo, ss, top, hn)
    plaque(S, U, V, X, Y, cfg, M, ss, top, hn)
    sky = (0.10 * (1 - Y / H) - 0.03).astype(F32)
    col_b, _ = shade(base, ss, wrap=False, sky=sky, shadow_px=9, tan_e=0.62)
    col_f, _ = shade(S, ss, wrap=False, sky=sky, shadow_px=9, tan_e=0.62)
    # the wells stay dark whatever the lighting does: soft-clip their luminance (hue kept)
    inwell = cov_sd(sd_socket(U, V, cfg['bezel']), ss) * top
    Yf = lum(col_f)
    scale = np.where(Yf > 1e-6, WELL_YMAX * np.tanh(Yf / WELL_YMAX) / np.maximum(Yf, 1e-6), 1.0)
    col_f = col_f * lerp(1.0, scale, inwell)[..., None]
    # keep the ornament render only inside the corners; its shadows fade out before the
    # slice lines, the pieces themselves end before them
    incorner = ((U < CORNER) & (V < CORNER)).astype(F32)
    ramp = (1 - sstep(27.5, CORNER, U)) * (1 - sstep(27.5, CORNER, V))
    wgt = np.maximum(S.cov, ramp) * incorner
    col = lerp(col_b, col_f, wgt[..., None])
    col = col * (0.40 + 0.60 * sstep(0.0, 1.0, D))[..., None]  # crisp dark outer outline
    col = box_down(soft_clip(col), ss)
    return col


def label_stats(col_lin):
    x0, x1, y0, y1 = LABEL_BOX
    L = lum(np.clip(col_lin, 0, 1))[y0:y1, x0:x1]
    return float(L.mean()), float(np.percentile(L, 95))


def make_button(args):
    name, out = args
    gain = 1.0
    t0 = time.time()
    tgt_m, tgt_p = (0.105, 0.150) if name in ('economy', 'defence') else (0.150, 0.285)
    for it in range(4):
        col = build_button(name, gain)
        q = l2s(col)
        q = np.round(q * 255) / 255.0
        m, p95 = label_stats(s2l(q))
        if m <= tgt_m and p95 <= tgt_p:
            break
        gain *= min(tgt_m / m, tgt_p / p95) ** 1.15 * 0.985
    save_rgba(os.path.join(out, 'button_%s.png' % name), col, np.ones(col.shape[:2], F32))
    return name, gain, m, p95, time.time() - t0


# ============================================================================= fx
def zero_border(a):
    a = a.copy()
    a[0, :] = a[-1, :] = 0.0
    a[:, 0] = a[:, -1] = 0.0
    return a


def build_glow(out):
    """Light hugging the inside edge of the button frame: a bright line on the dark seam
    between band and wood, a short tail into the wood (about 0.2 at 3 px in, 0 by 8, so it
    stays off the label), a bloom over the band and a bright rim on its outer edge. 0 at
    the texture border, in the stretched centre and over the two plaque wells, so the
    hotkey and count glyphs keep their contrast while the button is hovered or armed."""
    W, H, ss = BTN_W, BTN_H, SS
    X, Y = grid(W, H, ss)
    U = np.minimum(X, W - X)
    V = np.minimum(Y, H - Y)
    D = np.minimum(U, V)
    T = 7.2
    sdi = sd_box(X, Y, W / 2, H / 2, W / 2 - T, H / 2 - T, 6.0)
    din = -sdi  # > 0 over the wood, < 0 over the band
    di = np.clip(din, 0, None)
    inner = (1.0 * np.exp(-di / 1.6) + 0.10 * np.exp(-di / 3.5)) * (1 - sstep(5.0, 8.0, di))
    outer = (0.40 + 0.60 * np.exp(np.clip(din, None, 0) / 2.5)) * sstep(1.0, 5.0, D)
    a = np.clip(np.where(din >= 0, inner, outer), 0, 1)
    # a bright rim on the frame's dark outer edge: the hover reads on every metal, the
    # bright gold and silver frames included (the texture border texel itself stays 0)
    a = np.maximum(a, 0.95 * np.exp(-((D - 2.0) / 1.6) ** 2) * (din < 0))
    # cut out over the plaque wells: everything of the top corners inside the band strips,
    # with a soft outer edge; it ends exactly at the slice lines (x, y = 32), where the
    # line along the band starts again
    cut = sstep(3.8, 5.2, U) * sstep(2.8, 4.2, V) * ((U < CORNER) & (V < CORNER) & (Y < H / 2))
    a = a * (1 - cut)
    a = zero_border(box_down(a.astype(F32), ss))
    save_rgba(os.path.join(out, 'glow.png'), np.ones(a.shape + (3,), F32), a)


def build_burst(out):
    N, ss = 256, SS
    X, Y = grid(N, N, ss)
    r = np.hypot(X - N / 2, Y - N / 2)
    R0 = 100.0
    dr = r - R0
    core = np.exp(-(dr / 2.4) ** 2)
    soft = np.where(dr < 0, np.exp(-(dr / 18.0) ** 2), np.exp(-(dr / 9.0) ** 2))
    a = np.clip(core * 0.9 + soft * 0.55, 0, 1) * (1 - sstep(118, 127, r))
    a = zero_border(box_down(a.astype(F32), ss))
    save_rgba(os.path.join(out, 'burst_ring.png'), np.ones(a.shape + (3,), F32), a)


def build_spark(out):
    N, ss = 32, 8
    X, Y = grid(N, N, ss)
    dx = X - N / 2
    dy = Y - N / 2
    r = np.hypot(dx, dy)

    def ray(al, ac, length, w0):
        t = np.clip(1 - np.abs(al) / length, 0, 1)
        w = w0 * t + 0.25
        return np.exp(-(ac / w) ** 2) * t ** 1.6
    rays = np.maximum(ray(dx, dy, 15.0, 1.5), ray(dy, dx, 15.0, 1.5))
    q = 1 / math.sqrt(2)
    rays = np.maximum(rays, 0.45 * np.maximum(ray((dx + dy) * q, (dx - dy) * q, 8.0, 1.0),
                                              ray((dx - dy) * q, (dx + dy) * q, 8.0, 1.0)))
    a = np.exp(-(r / 2.4) ** 2) + 0.35 * np.exp(-(r / 5.5) ** 2) + rays
    a = np.clip(a, 0, 1) * (1 - sstep(14.0, 16.0, np.maximum(np.abs(dx), np.abs(dy))))
    a = zero_border(box_down(a.astype(F32), ss))
    save_rgba(os.path.join(out, 'spark.png'), np.ones(a.shape + (3,), F32), a)


def build_shine(out):
    W, H, ss = 64, 128, SS
    X, Y = grid(W, H, ss)
    ang = math.radians(22.0)
    ca, sa = math.cos(ang), math.sin(ang)
    dx = X - W / 2
    dy = Y - H / 2
    ac = dx * ca - dy * sa
    al = dx * sa + dy * ca
    a = 0.50 * np.exp(-(ac / 13.0) ** 2) + 0.50 * np.exp(-(ac / 4.5) ** 2)
    a = a * (1 - sstep(30, 64, np.abs(al)))
    a = a * sstep(0, 10, X) * sstep(0, 10, W - X) * sstep(0, 10, Y) * sstep(0, 10, H - Y)
    a = zero_border(box_down(np.clip(a, 0, 1).astype(F32), ss))
    save_rgba(os.path.join(out, 'shine.png'), np.ones(a.shape + (3,), F32), a)


# ============================================================================= Bevy compositing
def load_tex(path):
    im = np.asarray(Image.open(path).convert('RGBA'), dtype=F32) / 255.0
    return np.concatenate([s2l(im[..., :3]), im[..., 3:4]], -1).astype(F32)


def sample_bilinear(tex, u, v):
    th, tw = tex.shape[:2]
    x = u - 0.5
    y = v - 0.5
    x0 = np.floor(x).astype(np.int32)
    y0 = np.floor(y).astype(np.int32)
    fx = (x - x0)[..., None]
    fy = (y - y0)[..., None]
    x0c = np.clip(x0, 0, tw - 1)
    x1c = np.clip(x0 + 1, 0, tw - 1)
    y0c = np.clip(y0, 0, th - 1)
    y1c = np.clip(y0 + 1, 0, th - 1)
    c00 = tex[y0c, x0c]
    c10 = tex[y0c, x1c]
    c01 = tex[y1c, x0c]
    c11 = tex[y1c, x1c]
    return (c00 * (1 - fx) + c10 * fx) * (1 - fy) + (c01 * (1 - fx) + c11 * fx) * fy


def map_axis(p, il, ih, tl, th, r):
    with np.errstate(divide='ignore', invalid='ignore'):
        lo = (p / il) * tl if il > 0 else np.zeros_like(p)
        hi = th + ((p - ih) / (1 - ih)) * (1 - th) if ih < 1 else np.zeros_like(p)
        mid = tl + np.mod((r * (p - il)) / (ih - il), 1.0) * (th - tl)
    return np.where(p < il, lo, np.where(ih < p, hi, mid))


PANEL_MODE = dict(kind='sliced', border=(40, 40, 40, 40), sides='tile', center='stretch', stretch=0.5)
BAR_MODE = dict(kind='sliced', border=(40, 40, 20, 20), sides='tile', center='stretch', stretch=0.5)
BTN_MODE = dict(kind='sliced', border=(32, 32, 32, 32), sides='stretch', center='stretch')
TILED = dict(kind='tiled', stretch=0.5)


def draw_node(canvas, tex, x0, y0, w, h, mode, tint=(1.0, 1.0, 1.0, 1.0)):
    """Replicates bevy_ui_render 0.19 compute_texture_slices + ui_texture_slice.wgsl."""
    th_, tw_ = tex.shape[:2]
    xa, xb = int(round(x0)), int(round(x0 + w))
    ya, yb = int(round(y0)), int(round(y0 + h))
    xs = np.arange(xa, xb, dtype=F32)
    ys = np.arange(ya, yb, dtype=F32)
    PU, PV = np.meshgrid((xs + 0.5 - x0) / w, (ys + 0.5 - y0) / h)
    if mode['kind'] == 'sliced':
        bl, br, bt, bb = mode['border']
        stretch = mode.get('stretch', 0.5)
        coef = min(w / tw_, h / th_, mode.get('max_corner', 0.5))
        sl = [bl / tw_, bt / th_, 1 - br / tw_, 1 - bb / th_]
        tg = [bl / w * coef, bt / h * coef, 1 - br / w * coef, 1 - bb / h * coef]
        isw, ish = tw_ * (sl[2] - sl[0]), th_ * (sl[3] - sl[1])
        tsw, tsh = w * (tg[2] - tg[0]), h * (tg[3] - tg[1])

        def rep(img_e, tgt_e, m):
            return tgt_e / (img_e * stretch) if m == 'tile' else 1.0
        rsx, rsy = rep(isw, tsw, mode['sides']), rep(ish, tsh, mode['sides'])
        rcx, rcy = rep(isw, tsw, mode['center']), rep(ish, tsh, mode['center'])
        inside = (tg[0] <= PU) & (PU <= tg[2]) & (tg[1] <= PV) & (PV <= tg[3])
        rx = np.where(inside, rcx, rsx)
        ry = np.where(inside, rcy, rsy)
        mu = map_axis(PU, tg[0], tg[2], sl[0], sl[2], rx)
        mv = map_axis(PV, tg[1], tg[3], sl[1], sl[3], ry)
    else:
        stretch = mode.get('stretch', 0.5)
        mu = np.mod(PU * (w / (tw_ * stretch)), 1.0)
        mv = np.mod(PV * (h / (th_ * stretch)), 1.0)
    smp = sample_bilinear(tex, mu * tw_, mv * th_)
    rgb = smp[..., :3] * np.asarray(tint[:3], F32)
    a = smp[..., 3] * tint[3]
    reg = canvas[ya:yb, xa:xb]
    reg[:] = reg * (1 - a[..., None]) + rgb * a[..., None]


_FONTS = {}


def font(size):
    if size not in _FONTS:
        _FONTS[size] = ImageFont.truetype(FONT_PATH, size)
    return _FONTS[size]


def draw_text(canvas, text, x, y, size, color=TEXT_SRGB, anchor='la', alpha=1.0):
    f = font(size)
    l, t, r, b = f.getbbox(text, anchor=anchor)
    pad = 2
    wd, ht = r - l + 2 * pad, b - t + 2 * pad
    m = Image.new('L', (wd, ht), 0)
    ImageDraw.Draw(m).text((pad - l, pad - t), text, fill=255, font=f, anchor=anchor)
    ma = np.asarray(m, F32)[..., None] / 255.0 * alpha
    xi, yi = int(round(x + l - pad)), int(round(y + t - pad))
    H, W = canvas.shape[:2]
    x0, y0 = max(xi, 0), max(yi, 0)
    x1, y1 = min(xi + wd, W), min(yi + ht, H)
    if x1 <= x0 or y1 <= y0:
        return
    ma = ma[y0 - yi:y1 - yi, x0 - xi:x1 - xi]
    c = s2l(np.asarray(color, F32))
    reg = canvas[y0:y1, x0:x1]
    reg[:] = reg * (1 - ma) + c * ma


def wrap_label(text, size, max_w):
    f = font(size)
    words = text.split()
    lines, cur = [], ''
    for w_ in words:
        cand = (cur + ' ' + w_).strip()
        if f.getlength(cand) <= max_w or not cur:
            cur = cand
        else:
            lines.append(cur)
            cur = w_
    if cur:
        lines.append(cur)
    return lines


def fit_label(text, max_w=76, max_lines=3):
    """The engine's card label: 14 px, shrinking to 12 / 11 / 10 so the longest word fits."""
    for size in (14, 12, 11, 10):
        lines = wrap_label(text, size, max_w)
        if max(font(size).getlength(w_) for w_ in text.split()) <= max_w and len(lines) <= max_lines:
            return size, lines
    return 10, wrap_label(text, 10, max_w)


def draw_slot(can, tex, skin, x, y, label, hotkey=None, count=None, state=None, w=84, h=62):
    """One command-card button as the engine draws it (label in the padded box 3..81 x 13..60)."""
    grey = float(s2l(np.array(0.45)))
    gold = tuple(s2l(np.array(GLOW_GOLD)))
    tint = (grey, grey, grey, 1.0) if state == 'disabled' else (1.0, 1.0, 1.0, 1.0)
    draw_node(can, tex['button_' + skin], x, y, w, h, BTN_MODE, tint)
    if state == 'hover':
        draw_node(can, tex['glow'], x, y, w, h, BTN_MODE, gold + (0.35,))
    if state == 'armed':
        draw_node(can, tex['glow'], x, y, w, h, BTN_MODE, gold + (0.80,))
    la = 0.5 if state == 'disabled' else 1.0
    if label:
        size, lines = fit_label(label)
        y_c = y + 36.5
        y_top = y_c - (len(lines) * size) / 2 + size / 2
        for k, ln in enumerate(lines):
            draw_text(can, ln, x + 42, y_top + k * size, size, anchor='mm', alpha=la)
    if hotkey:
        draw_text(can, hotkey, x + TEXT_POS[0][0], y + TEXT_POS[0][1], 14, anchor='mm', alpha=la)
    if count:
        draw_text(can, count, x + TEXT_POS[1][0], y + TEXT_POS[1][1], 14, anchor='mm', alpha=la,
                  color=HIGHLIGHT_SRGB)


def ground_bg(h, w, seed=9):
    rng = np.random.default_rng(seed)
    n1 = pnoise(h, w, rng, 110, 110, 2.0)
    n2 = pnoise(h, w, rng, 22, 22, 1.6)
    n3 = pnoise(h, w, rng, 2.5, 2.5, 1.0)
    n4 = pnoise(h, w, rng, 6, 6, 1.4)
    grass = lerp(rgb8(72, 98, 44), rgb8(50, 76, 33), sstep(-1, 1, n2)[..., None])
    grass = grass * (1 + 0.18 * n3 + 0.1 * n4)[..., None]
    dirt = lerp(rgb8(116, 93, 64), rgb8(88, 70, 48), sstep(-1, 1, n4)[..., None]) * (1 + 0.12 * n3)[..., None]
    t = sstep(0.35, 0.9, n1 + 0.35 * n2)
    return lerp(grass, dirt, t[..., None]).astype(F32)


def minimap_img(h, w, seed=5):
    rng = np.random.default_rng(seed)
    n1 = pnoise(h, w, rng, 18, 18, 2.0)
    n2 = pnoise(h, w, rng, 6, 6, 1.6)
    img = lerp(rgb8(64, 96, 46), rgb8(36, 64, 30), sstep(0.3, 0.8, n2)[..., None])
    img = lerp(img, rgb8(110, 90, 62), sstep(0.6, 0.9, n1)[..., None])
    img = lerp(img, rgb8(36, 62, 96), sstep(-0.9, -1.2, n1)[..., None])
    for (cx, cy, c) in ((40, 140, (60, 120, 230)), (46, 132, (60, 120, 230)), (150, 40, (220, 50, 40)),
                        (140, 52, (220, 50, 40)), (52, 150, (60, 120, 230))):
        yy, xx = np.mgrid[0:h, 0:w]
        m = (np.abs(xx - cx) < 3) & (np.abs(yy - cy) < 3)
        img[m] = rgb8(*c)
    img[90:92, 60:120] = 1.0
    img[130:132, 60:120] = 1.0
    img[90:132, 60:62] = 1.0
    img[90:132, 118:120] = 1.0
    return img


CARD = [  # (skin, label, hotkey, state)
    ('command', 'Move', 'Q', None), ('command', 'Patrol', 'W', None), ('stance', 'Aggressive', 'E', None),
    ('economy', 'Economy', 'R', None), ('military', 'Military', 'T', None),
    ('defence', 'Defence', 'A', None), ('military', 'Barracks', 'S', None), ('train', 'Train Archer', 'D', 'count'),
    ('research', 'Research Iron Casting', 'F', None), ('upgrade', 'Upgrade Stone Wall', 'G', None),
    ('economy', 'Farm', 'Z', 'disabled'), ('defence', 'Stone Wall', 'X', 'hover'), ('command', 'Attack', 'C', 'armed'),
    ('plain', 'Cancel', 'V', None), ('nav', 'Back', 'B', None),
]
CARD_W, CARD_H = 5 * 84 + 4 * 4 + 18, 3 * 62 + 2 * 4 + 18   # 454 x 212, as card_view.rs


def render_preview(pdir, tex):
    W, H = 1280, 720
    can = ground_bg(H, W)
    wood = tex['panel_wood']
    # top bar
    draw_node(can, wood, 0, 0, W, 38, TILED)
    draw_node(can, tex['bar_frame'], 0, 0, W, 38, BAR_MODE)
    x = 26
    for lab in ('Gold 1250', 'Wood 830', 'Stone 400', 'Food 1200', 'Pop 24/50'):
        draw_text(can, lab, x, 19, 14, anchor='lm')
        x += font(14).getlength(lab) + 26
    draw_text(can, 'Feudal Age  12:41', 760, 19, 14, anchor='lm', color=(0.95, 0.85, 0.55))
    draw_node(can, tex['button_plain'], 1100, 8, 24, 22, BTN_MODE)
    draw_text(can, '<', 1112, 19, 12, anchor='mm')
    draw_node(can, tex['button_plain'], 1128, 8, 24, 22, BTN_MODE)
    draw_text(can, '>', 1140, 19, 12, anchor='mm')
    draw_node(can, tex['button_plain'], 1160, 5, 96, 28, BTN_MODE)
    draw_text(can, 'Menu', 1208, 19, 13, anchor='mm')
    # minimap
    mx, my = 0, H - 200
    draw_node(can, wood, mx, my, 200, 200, TILED)
    mm = minimap_img(176, 176)
    can[my + 12:my + 188, mx + 12:mx + 188] = mm
    draw_node(can, tex['panel_frame'], mx, my, 200, 200, PANEL_MODE)
    # selection
    sx, sy = 208, H - 210
    draw_node(can, wood, sx, sy, 600, 210, TILED)
    draw_node(can, tex['panel_frame'], sx, sy, 600, 210, PANEL_MODE)
    draw_node(can, tex['button_plain'], sx + 18, sy + 18, 96, 96, BTN_MODE)
    draw_text(can, 'S', sx + 66, sy + 66, 40, anchor='mm', color=(0.85, 0.80, 0.70))
    draw_text(can, 'Spearman', sx + 130, sy + 24, 18, anchor='la')
    draw_text(can, 'Health 85 / 100', sx + 130, sy + 52, 13, anchor='la')
    can[sy + 72:sy + 80, sx + 130:sx + 330] = s2l(np.array([0.10, 0.08, 0.06]))
    can[sy + 73:sy + 79, sx + 131:sx + 301] = s2l(np.array([0.30, 0.62, 0.22]))
    draw_text(can, 'Attack 9   Armor 2   Range 1', sx + 130, sy + 92, 13, anchor='la')
    draw_text(can, 'Pikes beat cavalry. Slow vs archers.', sx + 130, sy + 112, 12, anchor='la',
              color=(0.80, 0.78, 0.72))
    for i in range(6):
        bx = sx + 18 + i * 48
        draw_node(can, tex['button_plain'], bx, sy + 144, 44, 44, BTN_MODE)
        draw_text(can, str(i + 1), bx + 22, sy + 166, 13, anchor='mm')
    draw_node(can, tex['button_upgrade'], sx + 312, sy + 150, 120, 36, BTN_MODE)
    draw_text(can, 'Age Up', sx + 372, sy + 168, 14, anchor='mm')
    draw_node(can, tex['button_command'], sx + 440, sy + 146, 140, 44, BTN_MODE)
    draw_text(can, 'Stretched 140x44', sx + 510, sy + 168, 12, anchor='mm')
    # command card (454 x 212: 5x3 slots of 84x62, 4 px gaps, 9 px padding)
    cx, cy = W - CARD_W - 4, H - CARD_H - 4
    draw_node(can, wood, cx, cy, CARD_W, CARD_H, TILED)
    draw_node(can, tex['panel_frame'], cx, cy, CARD_W, CARD_H, PANEL_MODE)
    for i, (skin, lab, hk, state) in enumerate(CARD):
        bx = cx + 9 + (i % 5) * 88
        by = cy + 9 + (i // 5) * 66
        draw_slot(can, tex, skin, bx, by, lab, hk, '3' if state == 'count' else None, state)
    img = l2s(np.clip(can, 0, 1))
    img8 = np.round(img * 255).astype(np.uint8)
    Image.fromarray(img8, 'RGB').save(os.path.join(pdir, 'preview_hud.png'))
    crop = img8[cy:cy + CARD_H, cx:cx + CARD_W]
    Image.fromarray(crop, 'RGB').resize((CARD_W * 3, CARD_H * 3), Image.NEAREST).save(
        os.path.join(pdir, 'preview_zoom.png'))


def checker(h, w, s=8):
    yy, xx = np.mgrid[0:h, 0:w]
    c = np.where(((xx // s) + (yy // s)) % 2 == 0, 0.42, 0.52).astype(F32)
    return np.repeat(s2l(c)[..., None], 3, -1)


def half_gpu(tex):
    """bilinear at 0.5 = 2x2 average of straight-alpha texels (what the sampler does)."""
    h, w = tex.shape[:2]
    return tex.reshape(h // 2, 2, w // 2, 2, tex.shape[2]).mean(axis=(1, 3))


def render_sheet(pdir, tex):
    items = [('panel_wood', 'panel_wood'), ('panel_frame', 'panel_frame'), ('bar_frame', 'bar_frame')]
    items += [('button_' + s, s) for s in SKIN_ORDER]
    items += [('glow', 'glow'), ('burst_ring', 'burst_ring'), ('spark', 'spark'), ('shine', 'shine')]
    W = 1500
    pad = 14
    x, y, rowh = pad, pad, 0
    place = []
    for key, lab in items:
        t = tex[key]
        h, w = t.shape[:2]
        cw = max(w + w // 2 + 3 * pad, int(font(12).getlength(lab + '  (2x / 1x)')) + 2 * pad)
        ch = h + 20
        if x + cw > W:
            x = pad
            y += rowh + pad
            rowh = 0
        place.append((key, lab, x, y))
        x += cw
        rowh = max(rowh, ch)
    y_qa = y + rowh + pad
    Ht = y_qa + 300 + 150 + 165 + 170 + 320
    can = checker(Ht, W)
    for key, lab, x, y in place:
        t = tex[key]
        h, w = t.shape[:2]
        draw_text(can, lab + '  (2x / 1x)', x, y, 12, anchor='la')
        for (tt, ox) in ((t, 0), (half_gpu(t), w + pad)):
            hh, ww = tt.shape[:2]
            reg = can[y + 18:y + 18 + hh, x + ox:x + ox + ww]
            reg[:] = reg * (1 - tt[..., 3:4]) + tt[..., :3] * tt[..., 3:4]
    # QA 1: tiling check + frame over wood at 1x
    yb = y_qa
    draw_text(can, 'QA: panel_wood 2x2 tiles at 1x (seam check) | panel_frame 300x200 and bar 520x38 over wood '
                   'at 1x | buttons 84x62 at 1x', pad, yb, 12, anchor='la')
    draw_node(can, tex['panel_wood'], pad, yb + 18, 512, 256, TILED)
    draw_node(can, tex['panel_wood'], 548, yb + 18, 300, 200, TILED)
    draw_node(can, tex['panel_frame'], 548, yb + 18, 300, 200, PANEL_MODE)
    draw_node(can, tex['panel_wood'], 870, yb + 18, 520, 38, TILED)
    draw_node(can, tex['bar_frame'], 870, yb + 18, 520, 38, BAR_MODE)
    draw_text(can, 'Gold 1250   Wood 830   Stone 400', 896, yb + 37, 14, anchor='lm')
    for i, s in enumerate(SKIN_ORDER):
        bx = 870 + (i % 5) * 92
        by = yb + 70 + (i // 5) * 70
        draw_slot(can, tex, s, bx, by, s.capitalize(), 'QWERTASDFG'[i], '3' if i in (5, 9) else None)
    # QA 2: long panel frame (tiling along 900 px) and the top-bar arrows
    yb2 = yb + 300
    draw_text(can, 'QA: panel_frame 900x120 over wood (side tiling) | stretched / small buttons', pad, yb2, 12,
              anchor='la')
    draw_node(can, tex['panel_wood'], pad, yb2 + 18, 900, 120, TILED)
    draw_node(can, tex['panel_frame'], pad, yb2 + 18, 900, 120, PANEL_MODE)
    # QA 3: stretched and small buttons (24x22 arrows, 40x30, 120x36, 160x44, 240x44)
    yb3 = yb2 + 150
    sizes = [(24, 22, '<'), (40, 30, '>'), (120, 36, 'Age Up'), (160, 44, 'Resume Game'), (240, 44, 'Menu')]
    rows = ['plain', 'nav', 'command', 'upgrade', 'military', 'defence']
    for r, s in enumerate(rows):
        col0 = pad + (r % 2) * 740
        yy = yb3 + (r // 2) * 52
        x = col0
        draw_node(can, tex['panel_wood'], x - 4, yy - 4, 732, 52, TILED)
        for (w, h, lab) in sizes:
            draw_node(can, tex['button_' + s], x, yy, w, h, BTN_MODE)
            draw_text(can, lab, x + w / 2, yy + h / 2, 12 if w < 60 else 14, anchor='mm')
            x += w + 10
        draw_text(can, s, x + 4, yy + 20, 12, anchor='lm')
    # QA 4: disabled (x0.45), hover (glow 0.35) and armed (glow 0.8) at 1x
    yb4 = yb3 + 165
    draw_text(can, 'QA: disabled x0.45 (top row) / hover glow 0.35 and armed 0.8 (bottom row), 1x', pad, yb4, 12,
              anchor='la')
    for i, s in enumerate(SKIN_ORDER):
        bx = pad + i * 90
        draw_slot(can, tex, s, bx, yb4 + 18, s.capitalize(), 'QWERTASDFG'[i], None, 'disabled')
    for i, s in enumerate(SKIN_ORDER):
        bx = pad + i * 90
        draw_slot(can, tex, s, bx, yb4 + 86, s.capitalize(), 'QWERTASDFG'[i], None,
                  'hover' if i % 2 == 0 else 'armed')
    # QA 5: how the 14 px hotkey (TEXT) and count (HIGHLIGHT) really sit on the plaques: the top
    # 22 px of a slot at 3x, nearest neighbour, normal and armed
    yb5 = yb4 + 170
    draw_text(can, 'QA: plaque fit at 3x (nearest): 14 px hotkey and count centred as the engine draws them, '
                   'top 22 px of each 84x62 slot; normal / armed (glow 0.8) below it', pad, yb5, 12, anchor='la')
    for i, s in enumerate(SKIN_ORDER):
        for j, st in enumerate((None, 'armed')):
            slot = np.zeros((62, 84, 3), F32)
            draw_slot(slot, tex, s, 0, 0, None, CARD_HOTKEYS[i], '3', st)
            big = np.repeat(np.repeat(slot[:22], 3, 0), 3, 1)
            x = pad + (i % 5) * 270
            y = yb5 + 18 + (i // 5) * 150 + j * 70
            can[y:y + 66, x:x + 252] = big
    img = np.round(l2s(np.clip(can, 0, 1)) * 255).astype(np.uint8)
    Image.fromarray(img, 'RGB').save(os.path.join(pdir, 'sheet.png'))


# ============================================================================= measurements
def lstar(Y):
    return np.where(Y > 0.008856, 116 * np.cbrt(Y) - 16, 903.3 * Y)


def lab_of(lin):
    """CIE L*a*b* (D65) of linear sRGB."""
    M = np.array([[0.4124, 0.3576, 0.1805], [0.2126, 0.7152, 0.0722], [0.0193, 0.1192, 0.9505]])
    xyz = lin @ M.T
    xyz = xyz / np.array([0.95047, 1.0, 1.08883])
    f = np.where(xyz > 0.008856, np.cbrt(xyz), 7.787 * xyz + 16 / 116)
    return np.stack([116 * f[..., 1] - 16, 500 * (f[..., 0] - f[..., 1]), 200 * (f[..., 1] - f[..., 2])], -1)


def glyph_mask(text, cx, cy, size, mode='box', w=84, h=62):
    """Coverage (> 25 %) of one line of text centred at (cx, cy) on a 1x slot. mode 'box':
    the line box (ascender..descender) is centred, which is how a centred Bevy text node
    lands (Fira Mono's cap letters sit 0.1 px above the box centre); 'ink': the glyphs'
    ink box is centred instead, a stricter check for Q, whose tail then lifts it."""
    m = Image.new('L', (w, h), 0)
    d = ImageDraw.Draw(m)
    f = font(size)
    if mode == 'ink':
        l, t, r, b = d.textbbox((0, 0), text, font=f)
        d.text((round(cx - (l + r) / 2), round(cy - (t + b) / 2)), text, font=f, fill=255)
    else:
        d.text((cx, cy), text, font=f, fill=255, anchor='mm')
    return np.asarray(m) > 64


def label_mask(text, w=84, h=62):
    """A card label laid out as draw_slot does (fit_label, lines centred on y 36.5)."""
    size, lines = fit_label(text)
    m = Image.new('L', (w, h), 0)
    d = ImageDraw.Draw(m)
    y_top = 36.5 - len(lines) * size / 2 + size / 2
    for k, ln in enumerate(lines):
        d.text((42, y_top + k * size), ln, font=font(size), fill=255, anchor='mm')
    return np.asarray(m) > 64


def ring_of(gm):
    return ndimage.binary_dilation(gm, iterations=2) & ~gm


def ring_p10(bg_lin, ring, color=TEXT_SRGB):
    """p10 contrast of text in `color` against the 1..2 px ring around its glyphs."""
    lt = float(lum(s2l(np.array(color))))
    return float(np.percentile((lt + 0.05) / (lum(bg_lin)[ring] + 0.05), 10))


def readability(pdir, tex):
    Lt = float(lum(s2l(np.array(TEXT_SRGB))))
    gold = s2l(np.array(GLOW_GOLD))
    L = ['Readability and QA metrics (computed from the PNGs as written)',
         'white label text sRGB (0.93,0.92,0.88): L = %.3f' % Lt, '']
    fails = []

    def chk(ok, what):
        if not ok:
            fails.append(what)
        return 'OK' if ok else 'FAIL'

    # 1. contract metric
    L.append('== 1. contract metric: WCAG relative luminance of the 2x texture ==')
    L.append('buttons: label area x 16..152, y 26..112 px; limit mean <= %.2f, p95 <= %.2f '
             '(economy / defence watch limit p95 <= %.2f)' % (LIM_MEAN, LIM_P95, WATCH_P95))
    for s in SKIN_ORDER:
        m, p = label_stats(tex['button_' + s][..., :3])
        ok = m <= LIM_MEAN and p <= LIM_P95 and (s not in ('economy', 'defence') or p <= WATCH_P95)
        L.append('button_%-9s mean %.3f  p95 %.3f  contrast(mean) %4.1f:1  contrast(p95) %4.1f:1  %s'
                 % (s, m, p, (Lt + 0.05) / (m + 0.05), (Lt + 0.05) / (p + 0.05), chk(ok, 'label ' + s)))
    Lw = lum(tex['panel_wood'][..., :3])
    m, p = float(Lw.mean()), float(np.percentile(Lw, 95))
    L.append('panel_wood (whole tile; limit mean <= %.2f, p95 <= %.2f): mean %.3f  p95 %.3f  contrast(mean) '
             '%.1f:1  %s' % (LIM_WOOD_MEAN, LIM_WOOD_P95, m, p, (Lt + 0.05) / (m + 0.05),
                             chk(m <= LIM_WOOD_MEAN and p <= LIM_WOOD_P95, 'panel_wood')))
    L.append('')
    # 2. text on the slot at 1x, in every state
    g1 = half_gpu(tex['glow'])
    states = {}
    for s in SKIN_ORDER:
        b = half_gpu(tex['button_' + s])[..., :3]
        states[s] = [b] + [b * (1 - g1[..., 3:4] * k) + gold * g1[..., 3:4] * k for k in (0.35, 0.80)]
    hk = {(c, md): ring_of(glyph_mask(c, TEXT_POS[0][0], TEXT_POS[0][1], 14, md))
          for c in CARD_HOTKEYS for md in ('box', 'ink')}
    cn = {n: ring_of(glyph_mask(n, TEXT_POS[1][0], TEXT_POS[1][1], 14)) for n in ('3', '8', '12')}
    lb = {t: ring_of(label_mask(t)) for t in QA_LABELS}

    def worst(bg, rings, color):
        return min((ring_p10(bg, r, color), k) for k, r in rings.items())
    L.append('== 2. text on an 84x62 slot at 1x (2x2 average): p10 of (Lt+.05)/(Lbg+.05) over the 1..2 px '
             'ring around the glyphs, states normal / hover (glow x gold 0.35) / armed (0.8) ==')
    L.append('hotkey: 14 px centred at (%.0f, %.2f), worst of the 15 card hotkeys %s, in TEXT and in TEXT_DIM '
             '(the engine\'s colour today); "box" centres the line box (Bevy), "ink" the ink box (stricter for '
             'Q)' % (TEXT_POS[0] + (CARD_HOTKEYS,)))
    L.append('count: 14 px at (%.0f, %.2f) in HIGHLIGHT, worst of "3" / "8"; "12" for information (16.8 px '
             'wide, it runs onto the side band whatever the art does)' % TEXT_POS[1])
    L.append('label: worst of %d card labels, 1-3 lines, 14 down to 11 px, up to 76 px wide (%s)'
             % (len(QA_LABELS), ', '.join(QA_LABELS)))
    L.append('limits: hotkey TEXT (box) >= %.1f in every state, TEXT_DIM (box) >= %.1f normal, count >= %.1f in '
             'every state, label >= %.1f in every state' % (LIM_HK_TEXT, LIM_HK_DIM, LIM_COUNT, LIM_GLYPH_LABEL))
    L.append('skin      | hotkey TEXT box n/h/a | TEXT ink | DIM box | DIM ink | count n/h/a     | "12"  | '
             'label n/h/a')
    rows = []
    for s in SKIN_ORDER:
        b, hv, ar = states[s]
        box = {k: r for k, r in hk.items() if k[1] == 'box'}
        ink = {k: r for k, r in hk.items() if k[1] == 'ink'}
        ht = [worst(bg, box, TEXT_SRGB) for bg in (b, hv, ar)]
        hti = worst(b, ink, TEXT_SRGB)
        hd = worst(b, box, TEXT_DIM_SRGB)
        hdi = worst(b, ink, TEXT_DIM_SRGB)
        c3 = [min(ring_p10(bg, cn[n], HIGHLIGHT_SRGB) for n in ('3', '8')) for bg in (b, hv, ar)]
        c12 = ring_p10(b, cn['12'], HIGHLIGHT_SRGB)
        lab = [worst(bg, lb, TEXT_SRGB) for bg in (b, hv, ar)]
        ok = (min(x[0] for x in ht) >= LIM_HK_TEXT and hd[0] >= LIM_HK_DIM and min(c3) >= LIM_COUNT
              and min(x[0] for x in lab) >= LIM_GLYPH_LABEL)
        rows.append([x[0] for x in ht] + [hti[0], hd[0], hdi[0]] + c3 + [c12] + [x[0] for x in lab])
        L.append('%-9s | %5.2f %5.2f %5.2f %s | %5.2f %s  | %5.2f %s | %5.2f %s | %5.2f %5.2f %5.2f | %5.2f | '
                 '%5.2f %5.2f %5.2f  %s' % (s, ht[0][0], ht[1][0], ht[2][0], min(ht)[1][0], hti[0], hti[1][0],
                                             hd[0], hd[1][0], hdi[0], hdi[1][0], c3[0], c3[1], c3[2], c12,
                                             lab[0][0], lab[1][0], lab[2][0], chk(ok, 'glyph ' + s)))
    mn = np.min(np.array(rows), axis=0)
    L.append('MIN       | %5.2f %5.2f %5.2f   | %5.2f    | %5.2f   | %5.2f   | %5.2f %5.2f %5.2f | %5.2f | '
             '%5.2f %5.2f %5.2f' % tuple(mn))
    L.append('worst label per state: ' + '; '.join(
        '%s %s' % (st, min((worst(states[s][i], lb, TEXT_SRGB), s) for s in SKIN_ORDER)[0][1])
        for i, st in enumerate(('normal', 'hover', 'armed'))))
    L.append('')
    # 3. states
    L.append('== 3. states at 1x (gold glow; ring = outer 6 px; label area x 8..76, y 13..56) ==')
    L.append('skin       ring dL* hover  ring dE76 hover  ring dL* armed-hover  label dL* hover  label meanY hover')
    gst = []
    for s in SKIN_ORDER:
        b, hv, ar = states[s]
        L0, Lh, La = lstar(lum(b)), lstar(lum(hv)), lstar(lum(ar))
        ring = np.zeros((62, 84), bool)
        ring[:6] = ring[-6:] = True
        ring[:, :6] = ring[:, -6:] = True
        la = np.zeros((62, 84), bool)
        la[13:56, 8:76] = True
        dE = np.linalg.norm(lab_of(hv) - lab_of(b), axis=-1)
        v = (np.abs(Lh - L0)[ring].mean(), dE[ring].mean(), np.abs(La - Lh)[ring].mean(), (Lh - L0)[la].mean(),
             lum(hv)[la].mean())
        gst.append(v)
        L.append('%-9s %13.2f %16.2f %21.2f %16.2f %18.4f' % ((s,) + v))
    gst = np.array(gst)
    L.append('MEAN      %13.2f %16.2f %21.2f %16.2f %18.4f' % tuple(gst.mean(0)))
    L.append('MIN       %13.2f %16.2f %21.2f %16.2f %18.4f' % tuple(gst.min(0)))
    ga = g1[..., 3]
    gfull = tex['glow'][..., 3]
    border = np.concatenate([gfull[0], gfull[-1], gfull[:, 0], gfull[:, -1]])
    wells = max(float(gfull[5:32, 6:32].max()), float(gfull[5:32, 136:162].max()))
    L.append('glow alpha at 1x: centre %.3f, max %.3f, row0 mean %.3f, row1 %.3f, row3 %.3f, row8 %.3f; '
             'texture border max %.3f  %s; over the plaque wells (2x, x 6..32 / 136..162, y 5..32) max %.3f  %s'
             % (ga[31, 42], ga.max(), ga[0].mean(), ga[1, 20:64].mean(), ga[3, 20:64].mean(), ga[8, 20:64].mean(),
                border.max(), chk(border.max() == 0, 'glow border'), wells, chk(wells <= 0.02, 'glow wells')))
    L.append('')
    # 4. category separation
    L.append('== 4. category separation at 1x ==')
    labs = {}
    for s in SKIN_ORDER:
        b = half_gpu(tex['button_' + s])[..., :3]
        labs[s] = lab_of(b[13:56, 8:76].reshape(-1, 3)).mean(0)
    pairs = sorted(((float(np.linalg.norm(labs[a] - labs[b])), a, b) for i, a in enumerate(SKIN_ORDER)
                    for b in SKIN_ORDER[i + 1:]))
    L.append('wood (label area mean L*a*b*): ' + '  '.join('%s %.0f/%.0f/%.0f C%.0f' % (
        s, labs[s][0], labs[s][1], labs[s][2], math.hypot(labs[s][1], labs[s][2])) for s in SKIN_ORDER))
    L.append('closest wood pairs dE76: ' + ', '.join('%s-%s %.1f' % (a, b, d) for d, a, b in pairs[:6]))
    hsl = []
    for s in SKIN_ORDER:
        b = l2s(tex['button_' + s][44:80, 1:6, :3]).reshape(-1, 3).mean(0)
        mx_, mn_ = b.max(), b.min()
        li = (mx_ + mn_) / 2
        sat = 0 if mx_ == mn_ else (mx_ - mn_) / (1 - abs(2 * li - 1))
        r_, g_, b_ = b
        if mx_ == mn_:
            hue = 0.0
        elif mx_ == r_:
            hue = 60 * (((g_ - b_) / (mx_ - mn_)) % 6)
        elif mx_ == g_:
            hue = 60 * ((b_ - r_) / (mx_ - mn_) + 2)
        else:
            hue = 60 * ((r_ - g_) / (mx_ - mn_) + 4)
        hsl.append('%s h%.0f s%.2f l%.2f' % (s, hue, sat, li))
    L.append('side band metal (left band x 1..5, y 44..80, sRGB HSL): ' + '  '.join(hsl))
    bl = {}
    for s in SKIN_ORDER:
        b = half_gpu(tex['button_' + s])[..., :3]
        px = np.concatenate([b[22:40, 0:3].reshape(-1, 3), b[59:62, 22:62].reshape(-1, 3)])
        bl[s] = lab_of(px).mean(0)
    pairs = sorted((float(np.linalg.norm(bl[a] - bl[b])), a, b) for i, a in enumerate(SKIN_ORDER)
                   for b in SKIN_ORDER[i + 1:])
    L.append('closest band metal pairs at 1x (left and bottom band, L*a*b* dE76; limit >= %.0f): %s  %s'
             % (LIM_BAND_DE, ', '.join('%s-%s %.1f' % (a, b, d) for d, a, b in pairs[:6]),
                chk(pairs[0][0] >= LIM_BAND_DE, 'band metal separation')))
    L.append('')
    # 5. seams / slicing
    L.append('== 5. seams and slicing (sRGB 0-255; premultiplied for frames) ==')
    w8 = np.round(l2s(tex['panel_wood'][..., :3]) * 255)
    for ax, nm in ((1, 'x'), (0, 'y')):
        wrap = np.abs(np.take(w8, -1, axis=ax) - np.take(w8, 0, axis=ax)).mean()
        inter = np.abs(np.diff(w8, axis=ax)).mean()
        if ax == 0:  # the y wrap falls on a plank groove: compare with the interior grooves
            grooves = np.mean([np.abs(w8[64 * k - 1] - w8[64 * k]).mean() for k in range(1, 8)])
            L.append('panel_wood wrap y: seam |diff| %.2f vs interior plank grooves %.2f (plain interior '
                     'neighbour %.2f)' % (wrap, grooves, inter))
        else:
            L.append('panel_wood wrap x: seam |diff| %.2f vs interior neighbour %.2f' % (wrap, inter))
    for fn, (bl, br, bt, bb) in (('panel_frame', (40, 40, 40, 40)), ('bar_frame', (40, 40, 20, 20))):
        t = tex[fn]
        pm = np.concatenate([np.round(l2s(t[..., :3]) * 255) * t[..., 3:4], np.round(t[..., 3:4] * 255)], -1)
        Hh, Ww = pm.shape[:2]
        segs = {'top': (pm[0:bt, bl:Ww - br], 1), 'bottom': (pm[Hh - bb:, bl:Ww - br], 1),
                'left': (pm[bt:Hh - bb, 0:bl], 0), 'right': (pm[bt:Hh - bb, Ww - br:], 0)}
        parts = []
        for sn, (seg, ax) in segs.items():
            wrap = np.abs(np.take(seg, -1, axis=ax) - np.take(seg, 0, axis=ax))
            inter = np.abs(np.diff(seg, axis=ax)).mean()
            parts.append('%s wrap %.2f (max %.0f) / interior %.2f' % (sn, wrap.mean(), wrap.max(), inter))
        cen = np.round(t[bt:Hh - bb, bl:Ww - br, 3] * 255)
        L.append('%s: %s; centre alpha max %.0f  %s' % (fn, '; '.join(parts), cen.max(),
                                                      chk(cen.max() == 0, fn + ' centre')))
        if fn == 'bar_frame':
            for sn in ('left', 'right'):
                seg = segs[sn][0]
                dev = float(np.abs(seg - seg[:1]).max())
                L.append('bar_frame %s 8 px segment: row uniformity max dev %.1f  %s'
                         % (sn, dev, chk(dev == 0, 'bar rows ' + sn)))
    nonop = {s: int(np.count_nonzero(np.round(tex['button_' + s][..., 3] * 255) < 255)) for s in SKIN_ORDER}
    L.append('buttons: non-opaque texels %s  %s' % (sum(nonop.values()),
                                                     chk(sum(nonop.values()) == 0, 'button alpha')))
    sm = []
    for s in SKIN_ORDER:
        t8 = np.round(l2s(tex['button_' + s][..., :3]) * 255)
        sm.append('%s %.0f' % (s, np.abs(t8[0:32, 32] - t8[0:32, 84]).mean()))
    L.append('stretch check, mean |column 32 - column 84| over rows 0..31 (worn v1 was 45-67): ' + ', '.join(sm))
    # bottom ornaments stay out of the label's way; the plaque wells stay dark
    zs, ws_ = [], []
    for s in SKIN_ORDER:
        Yb = lum(tex['button_' + s][..., :3])
        z = np.concatenate([Yb[92:112, 16:32].ravel(), Yb[92:112, 136:152].ravel()])
        lab95 = float(np.percentile(Yb[60:92, 32:136], 95))
        zs.append((s, float(np.percentile(z, 95)), lab95))
        gy, gx = np.mgrid[0:BTN_H, 0:BTN_W] + 0.5
        inw = (sd_socket(np.minimum(gx, BTN_W - gx), gy, SKINS[s]['bezel']) < -0.5) & (gy < BTN_H / 2)
        ws_.append((s, float(Yb[inw].max()), float(Yb[inw].mean())))
    zok = all(z <= max(1.1 * c, c + 0.02) for _, z, c in zs)
    L.append('label-safe zone beside the bottom ornaments (x 16..32 / 136..152, y 92..112): p95 Y must stay '
             'within the p95 of the plain wood (x 32..136, y 60..92) + 10 %: ' + ', '.join(
                 '%s %.3f/%.3f' % v for v in zs) + '  ' + chk(zok, 'ornament zone'))
    wok = all(mx <= WELL_YMAX + 0.005 for _, mx, _ in ws_)
    L.append('plaque wells (2x texels inside SOCK) luminance max/mean, ceiling %.2f: ' % WELL_YMAX
             + ', '.join('%s %.3f/%.3f' % v for v in ws_) + '  ' + chk(wok, 'plaque wells'))
    # panel_frame: nothing (shadow included) more than 18 px (9 logical) from an outer edge
    pa = tex['panel_frame'][..., 3]
    yy, xx = np.mgrid[0:pa.shape[0], 0:pa.shape[1]]
    dd = np.minimum(np.minimum(xx, pa.shape[1] - 1 - xx), np.minimum(yy, pa.shape[0] - 1 - yy))
    reach = int(dd[pa > 0].max()) + 1
    L.append('panel_frame reach from the outer edge (alpha > 0): %d px = %.1f logical (card padding 9)  %s'
             % (reach, reach / 2, chk(reach <= 18, 'panel_frame reach')))
    # tiny buttons (top-bar arrows): the 1 px frame ring must stand out from the wood around it
    parts = []
    for s in ('plain', 'nav', 'command'):
        for (w, h) in ((24, 22), (40, 30)):
            can = np.zeros((h + 8, w + 8, 3), F32)
            draw_node(can, tex['panel_wood'], 0, 0, w + 8, h + 8, TILED)
            draw_node(can, tex['button_' + s], 4, 4, w, h, BTN_MODE)
            Y = lum(can)
            ring = np.zeros(Y.shape, bool)
            ring[4:4 + h, 4:4 + w] = True
            ring[5:3 + h, 5:3 + w] = False
            ins = Y[8:h, 8:w].mean()
            outs = Y[0:3].mean()
            r = Y[ring].mean()
            ok = r >= 1.8 * max(ins, outs)
            parts.append('%s %dx%d ring %.3f / wood in %.3f out %.3f %s' % (s, w, h, r, ins, outs,
                                                                         chk(ok, 'tiny frame %s' % s)))
    L.append('tiny buttons (frame ring >= 1.8x the wood inside and outside): ' + '; '.join(parts))
    for fn in ('glow', 'burst_ring', 'spark', 'shine'):
        A = np.round(tex[fn][..., 3] * 255)
        bd = np.concatenate([A[0], A[-1], A[:, 0], A[:, -1]]).max()
        L.append('%s: border alpha max %.0f  %s' % (fn, bd, chk(bd == 0, fn + ' border')))
    L.append('')
    L.append('ALL PASS' if not fails else 'FAIL: ' + ', '.join(fails))
    txt = '\n'.join(L) + '\n'
    with open(os.path.join(pdir, 'readability.txt'), 'w', newline='\n') as f:
        f.write(txt)
    return txt


# ============================================================================= main
TEX_KEYS = ['panel_wood', 'panel_frame', 'bar_frame', 'glow', 'burst_ring', 'spark', 'shine'] + \
           ['button_' + s for s in SKIN_ORDER]


def main():
    ap = argparse.ArgumentParser(description='Paint the Aetherfall HUD textures.')
    ap.add_argument('out', help='texture folder, e.g. assets/ui')
    ap.add_argument('--only', default='',
                    help='comma list of panel_wood, panel_frame, bar_frame, buttons, button_<skin>, fx')
    ap.add_argument('--no-preview', action='store_true', help='skip sheet / previews / readability')
    ap.add_argument('--jobs', type=int, default=10, help='parallel button workers')
    ap.add_argument('--preview-dir', default=os.path.join(HERE, 'out'),
                    help='where sheet.png, preview_*.png and readability.txt go (default art/ui/out)')
    a = ap.parse_args()
    out = a.out
    os.makedirs(out, exist_ok=True)
    only = set(x for x in a.only.split(',') if x)

    def want(n):
        return not only or n in only
    t0 = time.time()
    if want('panel_wood'):
        build_panel_wood(out)
        print('panel_wood %.1fs' % (time.time() - t0), flush=True)
    if want('panel_frame'):
        build_panel_frame(out)
        print('panel_frame %.1fs' % (time.time() - t0), flush=True)
    if want('bar_frame'):
        build_bar_frame(out)
        print('bar_frame %.1fs' % (time.time() - t0), flush=True)
    names = [s for s in SKIN_ORDER if want('button_' + s) or want('buttons')]
    if names:
        if a.jobs > 1 and len(names) > 1:
            from multiprocessing import Pool
            with Pool(min(a.jobs, len(names))) as pool:
                res = pool.map(make_button, [(n, out) for n in names])
        else:
            res = [make_button((n, out)) for n in names]
        for (n, g, m, p, dt) in res:
            print('button_%-9s gain %.3f mean %.3f p95 %.3f  %.1fs' % (n, g, m, p, dt), flush=True)
    if want('fx'):
        build_glow(out)
        build_burst(out)
        build_spark(out)
        build_shine(out)
        print('fx %.1fs' % (time.time() - t0), flush=True)
    if not a.no_preview:
        pdir = a.preview_dir
        os.makedirs(pdir, exist_ok=True)
        tex = {k: load_tex(os.path.join(out, k + '.png')) for k in TEX_KEYS}
        print(readability(pdir, tex))
        render_sheet(pdir, tex)
        render_preview(pdir, tex)
        print('previews %.1fs' % (time.time() - t0), flush=True)


if __name__ == '__main__':
    main()
