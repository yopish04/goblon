"""StudioZea - reference implementation (Python prototype) of the DTF engine.

This file is the *specification* for the Rust engine: every function here has been
tested in docs/RENCANA-DTF.md (sections 3, 4, 7, 9b). The Rust port must produce the
same pixels for the same settings (same thresholds, same order of operations).

Not optimised and not the app. Needs: numpy, scipy, pillow.

Pipeline (render):
  tone per channel (RGB on black shirt, CMY on white/colour shirt)
  -> noise clip on TOTAL ink (< 2 %)
  -> automatic tone calibration (per shape x LPI x DPI)
  -> per-channel screen: ranked rational AM tile, FM outside the printable tone zone
  -> compose 8 pure colours + binary alpha
  -> DTF clean-up (islands / holes below 0.15 mm)
"""
import math
from math import gcd

import numpy as np
from PIL import Image
from scipy import ndimage as ndi

f32 = np.float32

# --------------------------------------------------------------------------- shapes
TAU = 2 * math.pi
SPOT = {
    'bulat':       lambda u, v: 2 - np.cos(TAU * u) - np.cos(TAU * v),   # Euclidean, default
    'bulat-murni': lambda u, v: u * u + v * v,
    'elips':       lambda u, v: (u / 0.8) ** 2 + (v * 0.8) ** 2,
    'kotak':       lambda u, v: np.maximum(abs(u), abs(v)),
    'berlian':     lambda u, v: abs(u) + abs(v),
    'garis':       lambda u, v: abs(v) + abs(u) * 1e-3,
    'silang':      lambda u, v: np.minimum(abs(u), abs(v)),
    # wavy lines along u (periodic in u and v, so the waves join across cells)
    'gelombang':   lambda u, v: 1 - np.cos(TAU * (v + WAVE * np.sin(TAU * u))) + 1e-3 * (1 - np.cos(TAU * u)),
    # crosshatch: lines along u first, the perpendicular set joins at 50 % tone; both
    # sets close at the same time, so the last holes are small near-square gaps
    'arsir':       lambda u, v: np.minimum(1 - np.cos(TAU * v), HATCH[0] + HATCH[1] * (1 - np.cos(TAU * u))),
    # rough / analog dot: Euclidean dot + per pixel-class jitter (see JITTER, class_noise)
    'kasar':       lambda u, v: 2 - np.cos(TAU * u) - np.cos(TAU * v),
    # decorative shapes for big dots (pop art): five-point star (one point up at 0 deg),
    # triangle (point up) and chain (diamonds that link up along u first)
    'bintang':     lambda u, v: _star(u, v),
    'segitiga':    lambda u, v: np.maximum(np.maximum(v, 0.866 * u - 0.5 * v), -0.866 * u - 0.5 * v),
    'rantai':      lambda u, v: abs(u) / 1.3 + abs(v) * 1.3,
}
WAVE = 0.16                                  # wave amplitude of 'gelombang' (cell units)
STAR = 0.38                                  # 'bintang': depth of the star points


def _star(u, v):
    """r (1 + STAR sin 5a) with sin 5a = Im((u + iv)^5) / r^5: polynomials and one sqrt
    only, so Rust gives the same bits as numpy (no trig library differences)"""
    u2, v2 = u * u, v * v
    r2 = u2 + v2
    p5 = v * (5 * u2 * u2 - 10 * u2 * v2 + v2 * v2)
    with np.errstate(divide='ignore', invalid='ignore'):
        s = np.sqrt(r2) + STAR * p5 / (r2 * r2)
    return np.where(r2 > 0, s, 0.0)

HATCH = (1.0, 0.5)                           # 'arsir': offset, slope of the 2nd line set
JITTER = {'kasar': 0.8}                      # shape -> amplitude of the class noise
JITTER_CLASSES = 1600                        # max pixel classes of a jittered super-cell


def class_noise(A, B):
    """deterministic noise in [-0.5, 0.5) per pixel class (A, B) of the super-cell:
    splitmix64 of (A << 32 | B), top 53 bits. Integer only, so numpy and Rust agree
    bit for bit (no trig hash)."""
    z = (A.astype(np.uint64) << np.uint64(32)) | B.astype(np.uint64)
    z = z + np.uint64(0x9E3779B97F4A7C15)
    z = (z ^ (z >> np.uint64(30))) * np.uint64(0xBF58476D1CE4E5B9)
    z = (z ^ (z >> np.uint64(27))) * np.uint64(0x94D049BB133111EB)
    z = z ^ (z >> np.uint64(31))
    return (z >> np.uint64(11)).astype(np.float64) * 2.0 ** -53 - 0.5

# --------------------------------------------------------------------------- DTF limits
MIN_DOT_MM = 0.15


def mindot_px(dpi, mm=MIN_DOT_MM):
    """edge of the smallest FM cluster (300 dpi -> 2 px, 600 dpi -> 4 px)"""
    return max(1, math.ceil(mm * dpi / 25.4))


def minarea_px(dpi, mm=MIN_DOT_MM):
    """smallest ink island / hole that is kept (300 dpi -> 3 px, 600 dpi -> 10 px)"""
    return max(3, round(math.pi * (mm / 2 * dpi / 25.4) ** 2))


# --------------------------------------------------------------------------- rational screen
def angle_diff(a, b):
    """|a - b| in radians, the short way round (screen angles may be 0-360 deg)"""
    d = abs(a - b) % (2 * math.pi)          # a in (-pi, pi], b in [0, 2 pi): |a - b| < 3 pi
    return 2 * math.pi - d if d > math.pi else d


def rational(cellpx, deg, maxn=6, max_tile=1600):
    """Rational-tangent screen (P, Q, n): the cell vector is (P, Q)/n pixels.
    maxn=1 -> 'Rapi' grid (same family as the reference, 35 LPI 22.5 deg = 8:3).
    maxn=6 -> 'Presisi': score = geometric error + 1/levels, so screens with many grey
    levels win (55 LPI @300: 29 -> 1069 levels). Tile size P^2+Q^2 is capped."""
    a0 = math.radians(deg)
    best = None
    for n in range(1, maxn + 1):
        tx, ty = cellpx * n * math.cos(a0), cellpx * n * math.sin(a0)
        for P in {math.floor(tx), math.ceil(tx)}:
            for Q in {math.floor(ty), math.ceil(ty)}:
                if P == 0 and Q == 0:
                    continue
                if gcd(gcd(abs(P), abs(Q)), n) != 1:
                    continue
                c = math.hypot(P, Q) / n
                err = abs(c - cellpx) / cellpx + angle_diff(math.atan2(Q, P), a0) * 0.5
                if (maxn > 1 and err > 0.03) or P * P + Q * Q > max_tile:
                    continue
                score = err + 1.0 / (P * P + Q * Q)
                if best is None or score < best[0] - 1e-12:
                    best = (score, P, Q, n)
    if best is None:
        return round(cellpx * math.cos(a0)), round(cellpx * math.sin(a0)), 1
    return best[1:]


def choose_grid(dpi, lpi, deg=22.5, tol=0.03, min_levels=64):
    """'auto': the reference-style 'rapi' grid when it is close enough, else 'presisi'
    (35 LPI @300 -> rapi 8:3 like the reference; 45 LPI -> presisi, rapi would be 26.6 deg;
    55 LPI -> presisi, rapi would have only 29 grey levels)"""
    cellpx = dpi / lpi
    P, Q, n = rational(cellpx, deg, 1)
    err = abs(math.hypot(P, Q) - cellpx) / cellpx + angle_diff(math.atan2(Q, P), math.radians(deg)) * 0.5
    return 'rapi' if err <= tol and P * P + Q * Q >= min_levels else 'presisi'


def screen_info(dpi, lpi, deg, grid='presisi'):
    P, Q, n = rational(dpi / lpi, deg, 1 if grid == 'rapi' else 6)
    c = math.hypot(P, Q) / n
    return dict(P=P, Q=Q, n=n, lpi=dpi / c, angle=math.degrees(math.atan2(Q, P)), levels=P * P + Q * Q)


_TILE = {}


class ScreenTable:
    """Thresholds of a rational screen, one per pixel class (O(S) memory instead of an
    S x S tile, so big cells / low LPI work). Pixel (x, y) belongs to class
        ((x mod g) * g + (y mod g)) * s1 + ((x div g) * P1 + (y div g) * Q1) mod s1
    with g = gcd(P, Q), P = g P1, Q = g Q1, s1 = P1^2 + Q1^2 (g^2 s1 = S classes): two
    pixels are in the same class exactly when they differ by a vector of the lattice
    spanned by (P, Q) and (-Q, P)."""

    def __init__(self, g, P1, Q1, s1, table):
        self.g, self.P1, self.Q1, self.s1, self.table = g, P1, Q1, s1, table

    def index(self, x, y):
        g, s1 = self.g, self.s1
        return ((x % g) * g + (y % g)) * s1 + ((x // g) * self.P1 + (y // g) * self.Q1) % s1


def threshold_tile(dpi, lpi, deg, shape, grid='presisi'):
    """Exact threshold of every pixel-position class of the super-cell, each with a
    UNIQUE rank -> no tied thresholds (this was the 55 LPI bug) and the dots of the n^2
    cells grow in balance. Returns a ScreenTable (use tile_to)."""
    key = (dpi, lpi, deg, shape, grid)
    if key in _TILE:
        return _TILE[key]
    P, Q, n = rational(dpi / lpi, deg, 1 if grid == 'rapi' else 6)
    if shape in JITTER:
        # same screen (cell vector (kP, kQ)/(kn) = (P, Q)/n), k^2 times more cells per
        # super-cell, so neighbouring rough dots differ (k^2 * S <= JITTER_CLASSES)
        k = 1
        while (k + 1) * (k + 1) * (P * P + Q * Q) <= JITTER_CLASSES:
            k += 1
        P, Q, n = k * P, k * Q, k * n
    S = P * P + Q * Q
    g = gcd(abs(P), abs(Q))
    P1, Q1 = P // g, Q // g
    s1 = P1 * P1 + Q1 * Q1
    # one pixel per class: x = g X + xm, y = ym (X P1 runs through every residue mod s1)
    X = np.arange(s1, dtype=np.int64)
    xm, ym = np.meshgrid(np.arange(g, dtype=np.int64), np.arange(g, dtype=np.int64), indexing='ij')
    x = (g * X[None, None, :] + xm[..., None]).ravel()
    y = np.broadcast_to(ym[..., None], (g, g, s1)).ravel()
    A = (x * P + y * Q) % S
    B = (-x * Q + y * P) % S
    o = np.argsort(A * S + B)                # classes in the order of their id A*S + B
    A, B, x, y = A[o], B[o], x[o], y[o]
    u = (A * n % S) / S - 0.5
    v = (B * n % S) / S - 0.5
    sc = SPOT[shape](u + 0.5 / S, v + 0.5 / S)
    if shape in JITTER:
        sc = sc + JITTER[shape] * class_noise(A, B)
    cc = ((A * n // S) * n + (B * n // S)).astype(np.float64)
    ang = np.arctan2(v, u)
    order = np.lexsort((ang, (cc * 0.618) % 1, np.round(sc, 9)))
    rank = np.empty(S); rank[order] = np.arange(S)
    T = ScreenTable(g, P1, Q1, s1, np.empty(S, f32))
    T.table[T.index(x, y)] = ((rank + 0.5) / S).astype(f32)
    _TILE[key] = T
    return T


def tile_to(T, h, w, ox=0, oy=0):
    if isinstance(T, ScreenTable):
        x = np.arange(w, dtype=np.int64)[None, :] + ox
        y = np.arange(h, dtype=np.int64)[:, None] + oy
        return T.table[T.index(x, y)]
    S0, S1 = T.shape
    return T[np.ix_((np.arange(h) + oy) % S0, (np.arange(w) + ox) % S1)]


# --------------------------------------------------------------------------- FM / blue noise
_BN = {}


def blue_noise(n=128, sigma=1.6, seed=1):
    """void-and-cluster blue-noise threshold tile (deterministic)"""
    key = (n, sigma, seed)
    if key in _BN:
        return _BN[key]
    rng = np.random.default_rng(seed)
    f = np.fft.fftfreq(n)
    G = np.exp(-2 * (math.pi * sigma) ** 2 * (f[:, None] ** 2 + f[None, :] ** 2))

    def energy(b):
        return np.real(np.fft.ifft2(np.fft.fft2(b) * G))
    b = (rng.random((n, n)) < 0.1).astype(float)
    while True:
        e = energy(b); c = np.argmax(np.where(b > 0, e, -1e9)); b.flat[c] = 0
        e = energy(b); v = np.argmin(np.where(b == 0, e, 1e9)); b.flat[v] = 1
        if c == v:
            break
    rank = np.zeros(n * n, int); ones = int(b.sum()); p = b.copy()
    for r in range(ones - 1, -1, -1):
        e = energy(p); c = np.argmax(np.where(p > 0, e, -1e9)); p.flat[c] = 0; rank[c] = r
    p = b.copy()
    for r in range(ones, n * n):
        e = energy(p); v = np.argmin(np.where(p == 0, e, 1e9)); p.flat[v] = 1; rank[v] = r
    _BN[key] = ((rank.reshape(n, n) + 0.5) / (n * n)).astype(f32)
    return _BN[key]


def block_means(t, k):
    """k x k block means (edge blocks padded with the edge value)"""
    h, w = t.shape
    H, W = -(-h // k), -(-w // k)
    return np.pad(t, ((0, H * k - h), (0, W * k - w)), mode='edge').reshape(H, k, W, k).mean((1, 3))


def fm_bits(t, dpi, ox=0, oy=0):
    """FM with clusters of the minimum printable size (k x k px)"""
    k = mindot_px(dpi)
    h, w = t.shape
    H, W = -(-h // k), -(-w // k)
    p = block_means(t, k)
    b = p > tile_to(blue_noise(), H, W, ox, oy)
    return np.kron(b, np.ones((k, k), bool))[:h, :w]


def _bayer(n=8):
    m = np.zeros((1, 1), int)
    while m.shape[0] < n:
        m = np.block([[4 * m, 4 * m + 2], [4 * m + 3, 4 * m + 1]])
    return ((m + 0.5) / m.size).astype(f32)


BAYER = _bayer(8)


def bayer_bits(t, dpi, ox=0, oy=0):
    """ordered (Bayer 8 x 8) dither on k x k blocks: a regular, retro grid of dots"""
    k = mindot_px(dpi)
    h, w = t.shape
    H, W = -(-h // k), -(-w // k)
    b = block_means(t, k) > tile_to(BAYER, H, W, ox // k, oy // k)
    return np.kron(b, np.ones((k, k), bool))[:h, :w]


ED_TILE, ED_WARM = 32, 16                    # error diffusion tile and warm-up (blocks)
ED_JITTER = 0.4                              # threshold 0.5 +- 0.2 by blue noise


def ed_bits(t, dpi, ox=0, oy=0):
    """Floyd-Steinberg error diffusion (serpentine) on k x k blocks. Diffused per tile of
    ED_TILE blocks (absolute block grid) after a warm-up of ED_WARM blocks above and on
    both sides, so any window with enough halo gives the same dots as the full print.
    The threshold varies a little by a blue-noise tile (absolute block position): no
    worm / repeating patterns in flat areas, no seams between the tiles.
    Arithmetic in float64 (same in Rust)."""
    k = mindot_px(dpi)
    h, w = t.shape
    H, W = -(-h // k), -(-w // k)
    p = block_means(t, k).astype(np.float64)
    bx0, by0 = ox // k, oy // k
    T, M = ED_TILE, ED_WARM
    out = np.zeros((H, W), bool)
    bn = blue_noise()
    n = bn.shape[0]
    for ty in range(by0 // T, (by0 + H - 1) // T + 1):
        for tx in range(bx0 // T, (bx0 + W - 1) // T + 1):
            r0, r1 = max(ty * T - M, by0), min(ty * T + T, by0 + H)
            c0, c1 = max(tx * T - M, bx0), min(tx * T + T + M, bx0 + W)
            if r1 <= r0 or c1 <= c0:
                continue
            v = [list(map(float, p[r - by0, c0 - bx0:c1 - bx0])) for r in range(r0, r1)]
            nw = c1 - c0
            for i, r in enumerate(range(r0, r1)):
                row = v[i]
                nxt = v[i + 1] if i + 1 < len(v) else None
                ltr = r % 2 == 0
                cols = range(nw) if ltr else range(nw - 1, -1, -1)
                d = 1 if ltr else -1
                for j in cols:
                    c = c0 + j
                    old = row[j]
                    on = old > 0.5 + ED_JITTER * (float(bn[r % n, c % n]) - 0.5)
                    e = old - (1.0 if on else 0.0)
                    if tx * T <= c < tx * T + T and r >= ty * T:
                        out[r - by0, c - bx0] = on
                    if 0 <= j + d < nw:
                        row[j + d] += e * 0.4375
                    if nxt is not None:
                        if 0 <= j - d < nw:
                            nxt[j - d] += e * 0.1875
                        nxt[j] += e * 0.3125
                        if 0 <= j + d < nw:
                            nxt[j + d] += e * 0.0625
    return np.kron(out, np.ones((k, k), bool))[:h, :w]


def interlock_snap(t, lo, hi):
    """interlock: outside the printable AM zone the tone jumps to none / the smallest
    printable dot (and to the smallest printable hole / solid), so every dot keeps the
    same neat shape (no FM speckles)"""
    lo32, hi32 = np.float32(lo), np.float32(hi)
    t = np.where(t < lo, np.where(t < lo / 2, f32(0), lo32), t)
    t = np.where(t > hi, np.where(t > (1 + hi) / 2, f32(1), hi32), t)
    return t.astype(f32)


def green_noise_tile(n=512, s1=1.2, s2=4.0, seed=5):
    """band-pass noise for the 'grain' pattern: grain-sized clusters, no big blotches"""
    w = np.random.default_rng(seed).random((n, n))
    f = np.fft.fftfreq(n); r2 = f[:, None] ** 2 + f[None, :] ** 2

    def g(s):
        return np.exp(-2 * (math.pi * s) ** 2 * r2)
    v = np.real(np.fft.ifft2(np.fft.fft2(w) * (g(s1) - g(s2))))
    return ((np.argsort(np.argsort(v.ravel())) + 0.5).reshape(n, n) / (n * n)).astype(f32)


GRAIN = {'halus': (0.9, 3.0), 'sedang': (1.2, 4.0), 'kasar': (1.8, 6.0)}

# --------------------------------------------------------------------------- AM + hybrid
_ZONE = {}


def safe_zone(dpi, lpi, deg, shape, grid='presisi', tol=0.02):
    """Tone range where the AM screen of this shape makes ONLY printable islands/holes.
    Outside it the channel switches to FM (examples @300 dpi, 35 LPI:
    bulat 4-94 %, kotak 4-76 %, garis 14-86 %, silang 24-94 %)."""
    key = (dpi, lpi, deg, shape, grid)
    if key in _ZONE:
        return _ZONE[key]
    T = threshold_tile(dpi, lpi, deg, shape, grid)
    n = min(int(6 * dpi / lpi) + 64, 1024)
    Tt = tile_to(T, n, n)
    amin = minarea_px(dpi)
    levels = np.linspace(0.005, 0.995, 100)

    def tiny_frac(m):
        lb, k = ndi.label(m)
        if k == 0:
            return 0.0
        sz = np.bincount(lb.ravel()); sz[0] = 0
        small = sz < amin; small[0] = False
        return small[lb].sum() / max(1, m.sum())
    bad = np.array([max(tiny_frac(t > Tt), tiny_frac(~(t > Tt))) for t in levels])
    ok = bad <= tol
    lo = levels[np.argmax(ok)] if ok.any() else 0.5
    hi = levels[len(ok) - 1 - np.argmax(ok[::-1])] if ok.any() else 0.5
    _ZONE[key] = (float(lo), float(hi))
    return _ZONE[key]


def channel(t, dpi, lpi, deg, shape, grid='presisi', hybrid=True):
    h, w = t.shape
    am = t > tile_to(threshold_tile(dpi, lpi, deg, shape, grid), h, w)
    if not hybrid:
        return am
    lo, hi = safe_zone(dpi, lpi, deg, shape, grid)
    ts = ndi.uniform_filter(t, max(3, int(dpi / lpi)))
    return np.where((ts < lo) | (ts > hi), fm_bits(t, dpi), am)


_CAL = {}


def calib_curve(lpi, shape, dpi, angles, grid='presisi'):
    """Measured tone->coverage curve of the screen (AM + FM zones, WITHOUT clean-up),
    inverted, so printed coverage == requested tone for grey AND colours."""
    key = (lpi, shape, dpi, tuple(angles), grid)
    if key in _CAL:
        return _CAL[key]
    k = max(1, int(dpi) // 300)
    cell = dpi / lpi
    W, H = max(2048 * k, math.ceil(64 * cell)), max(96 * k, math.ceil(4 * cell))
    t = np.linspace(0, 1, W, dtype=f32)
    ramp = np.tile(t, (H, 1))
    covs = [ndi.uniform_filter1d(channel(ramp, dpi, lpi, angles[c], shape, grid).mean(0).astype(f32),
                                 max(31, int(4 * dpi / lpi))) for c in range(3)]
    cov = np.maximum.accumulate(np.mean(covs, 0)); cov[0] = 0; cov[-1] = max(cov[-1], 1.0)
    lut_t = np.linspace(0, 1, 1025, dtype=f32)
    inv = np.interp(lut_t, cov + np.arange(W) * 1e-7, t).astype(f32)
    _CAL[key] = (lut_t, inv)
    return _CAL[key]


def calibrate(t, lpi, shape, dpi, angles, grid='presisi'):
    lut_t, inv = calib_curve(lpi, shape, dpi, angles, grid)
    return np.interp(t, lut_t, inv).astype(f32)


# --------------------------------------------------------------------------- DTF clean-up
def clean(o, dpi):
    """remove ink islands < min area, fill holes < min area with the nearest ink colour,
    and keep RGB = 0 under every transparent pixel. Alpha stays 0/255."""
    a = o.copy()
    op = a[..., 3] == 255
    amin = minarea_px(dpi)
    lab, _ = ndi.label(op)
    sz = np.bincount(lab.ravel()); small = sz < amin; small[0] = False
    op2 = op & ~small[lab]
    hl, _ = ndi.label(~op2)
    hs = np.bincount(hl.ravel()); tiny = hs < amin; tiny[0] = False
    fill = tiny[hl]
    idx = ndi.distance_transform_edt(~op2, return_distances=False, return_indices=True)
    a[..., :3] = np.where(fill[..., None], a[..., :3][idx[0], idx[1]], a[..., :3])
    op3 = op2 | fill
    a[..., 3] = np.where(op3, 255, 0).astype(np.uint8)
    a[~op3, :3] = 0
    return a


# --------------------------------------------------------------------------- sharp edges (option)
def sharp_edges(tone, bits, dpi=300, strength=0.45):
    """solid 2 px (>= 0.15 mm) ring on HARD edges, stray specks just outside removed;
    soft gradients are not touched"""
    band = max(2.2, MIN_DOT_MM * dpi / 25.4 + 0.4)
    ink = tone.max(-1); S = ink > 0.5
    rng = ndi.maximum_filter(ink, 5) - ndi.minimum_filter(ink, 5); hard = rng > strength
    B = hard & S & (ndi.distance_transform_edt(S) <= band)
    O = hard & ~S & (ndi.distance_transform_edt(~S) <= 3)
    bits = bits.copy(); bits[B] = tone[B] > 0.5; bits[O] = False
    return bits


# --------------------------------------------------------------------------- render
REF_ANGLES_LIGHT = [22.5, 67.5, 22.5]     # C, M, Y like the Powerslave reference


def auto_angles(mode, lpi, angles=None, angle=None, rosette=None):
    """per-channel screen angles: explicit `angles`, else from the base `angle` (default
    22.5): rosette C/Y a, M a+45 (like the reference), or dot-on-dot (all a). Rosette by
    default except on black shirts and from 55 LPI (no gaps between screens)."""
    if angles:
        return angles
    a = 22.5 if angle is None else angle % 360
    if rosette is None:
        rosette = not (mode == 'hitam' or lpi >= 55)
    return [a, (a + 45) % 360, a] if rosette else [a] * 3


def _screen1(t, dpi, lpi, shape, ang, angs, grid, pattern, hybrid, cal):
    """one channel of the non-AM / interlock patterns"""
    if pattern == 'bayer':
        return bayer_bits(t, dpi)
    if pattern == 'difusi':
        return ed_bits(t, dpi)
    if pattern == 'interlock':
        if cal:
            t = calibrate(t, lpi, shape, dpi, angs, grid)
        lo, hi = safe_zone(dpi, lpi, ang, shape, grid)
        return channel(interlock_snap(t, lo, hi), dpi, lpi, ang, shape, grid, False)
    raise ValueError(pattern)


def _screen3(tone, dpi, lpi, shape, ang, grid, pattern, hybrid, cal):
    h, w = tone.shape[:2]
    if pattern in ('bayer', 'difusi', 'interlock'):
        return np.stack([_screen1(tone[..., c], dpi, lpi, shape, ang[c], ang, grid, pattern, hybrid, cal)
                         for c in range(3)], -1)
    if pattern == 'am':
        if cal:
            tone = np.stack([calibrate(tone[..., c], lpi, shape, dpi, ang, grid) for c in range(3)], -1)
        return np.stack([channel(tone[..., c], dpi, lpi, ang[c], shape, grid, hybrid) for c in range(3)], -1)
    if pattern == 'fm':                                      # synced FM (all channels same tile)
        return np.stack([fm_bits(tone[..., c], dpi) for c in range(3)], -1)
    if pattern.startswith('grain'):
        s1, s2 = GRAIN[pattern.split('-')[1] if '-' in pattern else 'sedang']
        T = tile_to(green_noise_tile(s1=s1, s2=s2), h, w)      # synced on every shirt
        return tone > T[..., None]
    raise ValueError(pattern)


def render(c, a, shirt='hitam', lpi=35, shape='bulat', dpi=300, grid='presisi', pattern='am',
           angles=None, k='off', gcr=1.0, knockout=0.0, edges=False, hybrid=True, cal=True, clip=0.02,
           angle=None, rosette=None, halftone_area=None, shirt_cut=0.12):
    """c: HxWx3 float colour (0..1, background already un-mixed), a: HxW alpha (0..1).
    shirt: 'hitam' | 'putih' | (r, g, b) floats for a coloured shirt.
    Returns RGBA uint8, alpha only 0/255, colours only 0/255 per channel
    (true colours with halftone_area, see render_fades).
    pattern 'interlock': Euclidean dots, dot-on-dot at one angle (default 45), integer
    grid, no FM at the tone ends (see interlock_snap) - the neatest AM look."""
    if pattern == 'interlock':
        shape, grid = 'bulat', 'rapi'
        angles = angles or [45.0 if angle is None else angle % 360] * 3
    if halftone_area is not None:
        return render_fades(c, a, shirt, halftone_area, lpi, shape, dpi, grid, pattern, angles, knockout,
                            edges, hybrid, cal, clip, angle, rosette, shirt_cut)
    if isinstance(shirt, str) and shirt == 'hitam':
        tone = (c * a[..., None]).astype(f32)
        tone[tone.max(-1) < clip] = 0
        ang = auto_angles('hitam', lpi, angles, angle, rosette)
        b = _screen3(tone, dpi, lpi, shape, ang, grid, pattern, hybrid, cal)
        if edges:
            b = sharp_edges(tone, b, dpi)
        alpha = b.any(-1)
        o = np.dstack([(b & alpha[..., None]).astype(np.uint8) * 255, (alpha * 255).astype(np.uint8)])
        return clean(o, dpi)
    coloured = not isinstance(shirt, str)
    ink = ((1 - c) * (1 if coloured else a[..., None])).astype(f32)
    ink[ink.max(-1) < clip] = 0
    ang = auto_angles('putih', lpi, angles, angle, rosette)
    K = np.zeros(ink.shape[:2], f32)
    if k == 'gcr':
        K = (gcr * ink.min(-1)).astype(f32)
    elif k == 'garis':
        K = np.where(ink.min(-1) > 0.9, ink.min(-1), 0).astype(f32)
    inkp = np.clip((ink - K[..., None]) / np.maximum(1 - K, 1e-3)[..., None], 0, 1)
    b = _screen3(inkp, dpi, lpi, shape, ang, grid, pattern, hybrid, cal)
    if edges:
        b = sharp_edges(inkp, b, dpi)
    kb = np.zeros(K.shape, bool)
    if k != 'off':
        kt = calibrate(K, lpi, shape, dpi, [45] * 3, grid) if cal else K
        kb = channel(kt, dpi, lpi, 45, shape, grid, hybrid)
    rgb = ~b; rgb[kb] = False
    if coloured:
        sh = np.asarray(shirt, f32)
        o_ = a
        if knockout > 0:                                       # 'hemat tinta' option
            d = np.linalg.norm(c - sh, axis=-1)
            o_ = a * np.clip((d - knockout + 0.06) / 0.06, 0, 1)
        alpha = fm_bits(o_.astype(f32), dpi)                   # opacity screen = FM
    else:
        alpha = b.any(-1) | kb
    o = np.dstack([(rgb & alpha[..., None]).astype(np.uint8) * 255, (alpha * 255).astype(np.uint8)])
    return clean(o, dpi)


def render_fades(c, a, shirt, area, lpi=35, shape='bulat', dpi=300, grid='presisi', pattern='am', angles=None,
                 knockout=0.0, edges=False, hybrid=True, cal=True, clip=0.02, angle=None, rosette=None,
                 shirt_cut=0.12):
    """'Halftone only on fades': the design prints in its true colours; only where it
    fades into the shirt colour does it become a halftone. o = normalised distance of the
    seen colour from the shirt colour (0 = shirt, 1 = as far as possible); from o >= area
    the print is solid, below it the opacity is screened with coverage o / area and the
    dots get the colour that keeps the average (shirt + (seen - shirt) / coverage), so
    there is no seam at the border. Alpha stays 0/255.
    area == 0: no halftone at all and nothing half-way: a pixel prints in its own full
    colour where it is at least half opaque (and differs from the shirt colour), else not
    at all - no darkened / faded edge colours that still look see-through. Colours closer
    to the shirt colour than shirt_cut (near-black shadows and JPEG noise on a black shirt)
    are not printed: the shirt shows the same colour without ink and white underbase."""
    coloured = not isinstance(shirt, str)
    S = (np.asarray(shirt, f32) if coloured else np.zeros(3, f32) if shirt == 'hitam'
         else np.ones(3, f32))
    if coloured and knockout > 0:                              # 'hemat tinta' option
        d = np.linalg.norm(c - S, axis=-1)
        a = (a * np.clip((d - knockout + 0.06) / 0.06, 0, 1)).astype(f32)
    if area == 0:
        own = (np.abs(c - S) / np.maximum(S, 1 - S)).max(-1)
        b = (a >= 0.5) & (own >= max(clip, f32(shirt_cut)))
        rgb = np.round(np.clip(c, 0, 1) * 255).astype(np.uint8) * b[..., None]
        return clean(np.dstack([rgb, (b * 255).astype(np.uint8)]), dpi)
    a3 = a[..., None]
    seen = (a3 * c + (1 - a3) * S).astype(f32)
    o = (np.abs(seen - S) / np.maximum(S, 1 - S)).max(-1)
    o[o < clip] = 0
    cov = np.minimum(o / f32(area), 1).astype(f32)
    ang = auto_angles('hitam', lpi, angles, angle, rosette)[0]
    if pattern == 'am':
        t = calibrate(cov, lpi, shape, dpi, [ang] * 3, grid) if cal else cov
        b = channel(t, dpi, lpi, ang, shape, grid, hybrid)
    elif pattern == 'fm':
        b = fm_bits(cov, dpi)
    elif pattern in ('bayer', 'difusi', 'interlock'):
        b = _screen1(cov, dpi, lpi, shape, ang, [ang] * 3, grid, pattern, hybrid, cal)
    else:
        s1, s2 = GRAIN[pattern.split('-')[1] if '-' in pattern else 'sedang']
        b = cov > tile_to(green_noise_tile(s1=s1, s2=s2), *cov.shape)
    if edges:
        b = sharp_edges(np.repeat(cov[..., None], 3, -1), np.repeat(b[..., None], 3, -1), dpi).any(-1)
    col = np.clip(S + (seen - S) / np.maximum(cov, f32(1e-6))[..., None], 0, 1)
    rgb = np.round(col * 255).astype(np.uint8) * b[..., None]
    return clean(np.dstack([rgb, (b * 255).astype(np.uint8)]), dpi)


# --------------------------------------------------------------------------- background removal
def remove_bg(img, t_core=0.04, t1=0.30, band=3, win=7, pockets=True, min_pocket=4):
    """Coloured / flat background removal with local foreground matting.
    bg colour = median of the border; zone = bg-like pixels connected to the border
    (+ enclosed pockets of exactly the bg colour >= 4 px), dilated; inside the zone
    alpha = projection of (p - bg) on (F - bg) with F the most foreground-like
    neighbour, raised to the minimum alpha that keeps the un-mixed colour in gamut;
    the bg colour is then un-mixed out of the edge pixels."""
    h, w, _ = img.shape
    Bc = np.median(np.r_[img[0], img[-1], img[:, 0], img[:, -1]], 0)
    d = np.linalg.norm(img - Bc, axis=-1)
    lb, _ = ndi.label(d < t1)
    ids = np.unique(np.r_[lb[0], lb[-1], lb[:, 0], lb[:, -1]]); ids = ids[ids > 0]
    zone = np.isin(lb, ids)
    if pockets:
        lc, _ = ndi.label(d < t_core)
        sz = np.bincount(lc.ravel()); keep = sz >= min_pocket; keep[0] = False
        zone |= keep[lc]
    zone = ndi.binary_dilation(zone, iterations=band)
    idx = np.arange(h * w, dtype=np.int64).reshape(h, w)
    key = np.round(d * 4096).astype(np.int64) * (1 << 32) + idx
    best = ndi.maximum_filter(key, size=win) % (1 << 32)
    F = img.reshape(-1, 3)[best]
    FB = F - Bc
    a = np.clip(((img - Bc) * FB).sum(-1) / np.maximum((FB * FB).sum(-1), 1e-6), 0, 1).astype(f32)
    # minimum alpha for which the un-mixed colour stays inside [0,1] (colour-to-alpha):
    # a pixel that is not a mix of bg and F (black outline on navy, shadows) stays opaque
    up = (img - Bc) / np.maximum(1 - Bc, 1e-6)
    dn = (Bc - img) / np.maximum(Bc, 1e-6)
    a = np.maximum(a, np.clip(np.maximum(up, dn).max(-1), 0, 1)).astype(f32)
    a[d < t_core] = 0
    a = np.where(zone, a, 1).astype(f32); a[a < 0.04] = 0
    c = np.clip((img - (1 - a[..., None]) * Bc) / np.maximum(a, 1e-3)[..., None], 0, 1)
    return c.astype(f32), a, Bc


# --------------------------------------------------------------------------- eraser
def apply_strokes(c, a, orig_c, orig_a, strokes):
    """Eraser / restore brush (hard edge). A stroke = dict(mode='hapus'|'kembalikan',
    r=radius px, pts=[[x, y], ...]) in source pixels: every pixel whose centre lies within
    r of the polyline is erased (alpha 0) or gets its original pixel back. Applied in
    order after background removal, before crop / trim."""
    c = c.copy(); a = a.copy()
    h, w = a.shape
    for s in strokes:
        r = float(s['r']); pts = [(float(x), float(y)) for x, y in s['pts']]
        segs = [(pts[0], pts[0])] if len(pts) == 1 else list(zip(pts[:-1], pts[1:]))
        for (ax, ay), (bx, by) in segs:
            x0 = int(max(math.floor(min(ax, bx) - r), 0.0)); y0 = int(max(math.floor(min(ay, by) - r), 0.0))
            x1 = int(min(math.ceil(max(ax, bx) + r) + 1.0, w)); y1 = int(min(math.ceil(max(ay, by) + r) + 1.0, h))
            if x1 <= x0 or y1 <= y0:
                continue
            dx, dy = bx - ax, by - ay
            l2 = dx * dx + dy * dy
            px = (np.arange(x0, x1, dtype=np.float64) + 0.5)[None, :]
            py = (np.arange(y0, y1, dtype=np.float64) + 0.5)[:, None]
            t = np.clip(((px - ax) * dx + (py - ay) * dy) / l2, 0.0, 1.0) if l2 > 0 else np.zeros((y1 - y0, x1 - x0))
            qx = ax + t * dx - px
            qy = ay + t * dy - py
            m = qx * qx + qy * qy <= r * r
            if s['mode'] == 'hapus':
                a[y0:y1, x0:x1][m] = 0
            elif s['mode'] == 'kembalikan':  # 'teks' / 'bukan-teks' mark text only (Rust)
                a[y0:y1, x0:x1][m] = orig_a[y0:y1, x0:x1][m]
                c[y0:y1, x0:x1][m] = orig_c[y0:y1, x0:x1][m]
    return c, a


def orient(c, a, rotate=0, mirror=False):
    """turn clockwise by rotate degrees (0/90/180/270), then mirror left-right (as seen
    after turning); applied after crop / trim, before resize and halftone"""
    k = (rotate // 90) % 4
    c, a = np.rot90(c, -k), np.rot90(a, -k)
    if mirror:
        c, a = c[:, ::-1], a[:, ::-1]
    return np.ascontiguousarray(c), np.ascontiguousarray(a)


# --------------------------------------------------------------------------- load / size
def load(path):
    """-> colour (float 0..1), alpha (float 0..1)"""
    im = Image.open(path)
    if im.mode in ('RGBA', 'LA', 'P'):
        x = np.asarray(im.convert('RGBA')).astype(f32) / 255
        return x[..., :3], x[..., 3]
    x = np.asarray(im.convert('RGB')).astype(f32) / 255
    return x, np.ones(x.shape[:2], f32)


def premul_resize(rgb, alpha, size, resample='lanczos'):
    """Lanczos (or bicubic) resize with premultiplied colour (no halo on soft edges)"""
    pm = np.dstack([rgb * alpha[..., None], alpha]).astype(f32)
    f = Image.LANCZOS if resample == 'lanczos' else Image.BICUBIC
    ch = [np.asarray(Image.fromarray(pm[..., i]).resize(size, f)) for i in range(4)]
    a = np.clip(ch[3], 0, 1)
    return np.clip(np.stack(ch[:3], -1) / np.maximum(a, 1e-4)[..., None], 0, 1).astype(f32), a.astype(f32)


def autotrim(c, a, shirt):
    """bounding box of pixels that would print"""
    if isinstance(shirt, str) and shirt == 'hitam':
        m = (c * a[..., None]).max(-1) > 0.06
    elif isinstance(shirt, str):
        m = ((1 - c) * a[..., None]).max(-1) > 0.06
    else:
        m = a > 0.06
    ys, xs = np.nonzero(m)
    return (int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1) if len(xs) else None


def save_png(o, path, dpi):
    """final check before writing: refuse semi-transparent or impure pixels"""
    al = o[..., 3]
    assert not ((al > 0) & (al < 255)).any(), 'semi-transparent pixels'
    rgb = o[..., :3][al == 255]
    assert not ((rgb != 0) & (rgb != 255)).any(), 'impure colours'
    assert not (o[..., :3][al == 0] != 0).any(), 'hidden colour under transparency'
    Image.fromarray(o, 'RGBA').save(path, dpi=(dpi, dpi))


# --------------------------------------------------------------------------- metrics
def lab(rgb):
    c = np.where(rgb <= 0.04045, rgb / 12.92, ((rgb + 0.055) / 1.055) ** 2.4)
    M = np.array([[0.4124, 0.3576, 0.1805], [0.2126, 0.7152, 0.0722], [0.0193, 0.1192, 0.9505]])
    xyz = c @ M.T / np.array([0.9505, 1, 1.089])
    f = np.where(xyz > 0.008856, np.cbrt(xyz), 7.787 * xyz + 16 / 116)
    return np.stack([116 * f[..., 1] - 16, 500 * (f[..., 0] - f[..., 1]), 200 * (f[..., 1] - f[..., 2])], -1)


def on_shirt(o, shirt_rgb):
    a = o[..., 3:].astype(f32) / 255
    return o[..., :3].astype(f32) / 255 * a + np.asarray(shirt_rgb, f32) * (1 - a)


def delta_e(img, ideal, sigma=5.0):
    """colour difference seen from viewing distance: both blurred (sigma px), Lab dE76.
    returns (mean, 95th percentile)"""
    def blur(x):
        return np.stack([ndi.gaussian_filter(x[..., i], sigma) for i in range(3)], -1)
    d = np.linalg.norm(lab(blur(img)) - lab(blur(ideal)), axis=-1)
    return float(d.mean()), float(np.percentile(d, 95))


def checks(o, dpi, pure=True):
    """pure=False (true-colour output of render_fades): colours are not counted"""
    al = o[..., 3]
    op = al == 255
    lb, _ = ndi.label(op)
    sz = np.bincount(lb.ravel())[1:]
    rgb = o[..., :3][op]
    return dict(semi=int(((al > 0) & (al < 255)).sum()),
                impure=int(((rgb != 0) & (rgb != 255)).any(-1).sum()) if pure else 0,
                tiny=int((sz < minarea_px(dpi)).sum()),
                hidden=int((o[..., :3][~op] != 0).sum()))
