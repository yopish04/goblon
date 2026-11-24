"""Generate the shared engine assets and the golden test set for the Rust engine.

  python make_golden.py                       # writes ../engine/assets and ../engine/tests/golden
  python make_golden.py --expected /tmp/exp   # also writes the expected PNGs (for debugging diffs)

Assets: blue-noise and green-noise threshold tiles as ranks (so Rust uses exactly the
same thresholds). Golden cases: synthetic 8-bit inputs + parameters + SHA-256 of the
expected RGBA output of studiozea_ref.render().
"""
import argparse
import hashlib
import json
import math
import os
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import studiozea_ref as Z  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
ENGINE = os.path.join(HERE, '..', 'engine')


def write_assets():
    d = os.path.join(ENGINE, 'assets'); os.makedirs(d, exist_ok=True)
    bn = Z.blue_noise()
    ranks = np.round(bn.astype(np.float64) * 128 * 128 - 0.5).astype('<u2')
    assert np.array_equal(((ranks.astype(np.float64) + 0.5) / 16384).astype(np.float32), bn)
    ranks.tofile(os.path.join(d, 'blue_noise_128.u16'))
    for name, (s1, s2) in Z.GRAIN.items():
        g = Z.green_noise_tile(s1=s1, s2=s2)
        r = np.round(g.astype(np.float64) * 512 * 512 - 0.5).astype('<u4')
        assert np.array_equal(((r.astype(np.float64) + 0.5) / (512 * 512)).astype(np.float32), g)
        r.tofile(os.path.join(d, f'green_noise_512_{name}.u32'))


# ------------------------------------------------------------------ synthetic inputs (8-bit PNG)
def chart(patch=118):
    import uji_lpi_bentuk as U
    cols = U.chart_colours()
    img = np.zeros((6 * patch, 10 * patch, 3), np.uint8)
    for i, c in enumerate(cols):
        y, x = divmod(i, 10)
        img[y * patch:(y + 1) * patch, x * patch:(x + 1) * patch] = np.round(c * 255)
    return Image.fromarray(img)


def ramp(w=1200, h=80):
    r = np.round(np.tile(np.linspace(0, 255, w), (h, 1))).astype(np.uint8)
    return Image.fromarray(np.dstack([r, r, r]))


def art(bg, w=900, h=700):
    """sky gradient, glow, hard-edged letters, thin lines, small text"""
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    sky = np.zeros((h, w, 3))
    t = y / h
    sky[..., 0] = 0.15 + 0.5 * t; sky[..., 1] = 0.25 + 0.3 * np.sin(x / w * math.pi); sky[..., 2] = 0.8 - 0.4 * t
    r = np.hypot(x - w * 0.6, y - h * 0.45) / (0.35 * w)
    glow = np.clip(1 - r, 0, 1) ** 2
    sky = sky * (1 - glow[..., None]) + np.array([1.0, 0.95, 0.7]) * glow[..., None]
    mask = np.clip(1.2 - np.hypot(x - w / 2, y - h / 2) / (0.42 * w), 0, 1)   # fades into the bg
    base = np.asarray(bg, np.float64)[None, None] * (1 - mask[..., None]) + sky * mask[..., None]
    im = Image.fromarray(np.round(np.clip(base, 0, 1) * 255).astype(np.uint8))
    d = ImageDraw.Draw(im)
    d.polygon([(80, 80), (260, 80), (260, 130), (130, 130), (130, 190), (230, 190), (230, 240), (130, 240), (130, 330), (80, 330)], fill=(40, 220, 90))
    d.polygon([(300, 330), (380, 80), (460, 330), (420, 330), (380, 200), (340, 330)], fill=(220, 30, 40))
    d.ellipse([500, 80, 660, 240], outline=(250, 250, 250), width=12)
    for i in range(12):
        d.line([(80 + i * 60, 620), (120 + i * 60, 520)], fill=(30, 30, 30) if bg[0] > 0.5 else (230, 230, 230), width=1 + i % 3)
    d.text((80, 400), 'StudioZea DTF 0123456789', fill=(250, 200, 60))
    d.rectangle([600, 420, 860, 560], fill=(120, 120, 120))
    return im


def art_rgba(w=700, h=500):
    """design with soft edges and a 50 % black drop shadow on transparency"""
    base = art((1, 1, 1), w, h).convert('RGB')
    m = Image.new('L', (w, h), 0); d = ImageDraw.Draw(m)
    d.rounded_rectangle([60, 50, w - 60, h - 70], 60, fill=255)
    m = m.filter(ImageFilter.GaussianBlur(1.2))
    sh = Image.new('L', (w, h), 0); ImageDraw.Draw(sh).rounded_rectangle([80, 70, w - 40, h - 50], 60, fill=128)
    sh = sh.filter(ImageFilter.GaussianBlur(8))
    a = np.asarray(m).astype(np.float64) / 255; s = np.asarray(sh).astype(np.float64) / 255
    A = a + s * (1 - a)
    C = np.asarray(base).astype(np.float64) / 255 * a[..., None] / np.maximum(A, 1e-6)[..., None]
    out = np.dstack([np.clip(C, 0, 1), A])
    return Image.fromarray(np.round(out * 255).astype(np.uint8), 'RGBA')


def art_teal(w=700, h=500):
    rgba = np.asarray(art_rgba(w, h)).astype(np.float64) / 255
    a = rgba[..., 3:]; a = np.where(a > 0.6, a, 0)          # drop the shadow, keep soft edge
    teal = np.array([0.17, 0.47, 0.50])
    img = rgba[..., :3] * a + teal * (1 - a)
    return Image.fromarray(np.round(img * 255).astype(np.uint8))


def art_navy(w=700, h=500):
    """design on a flat navy background with black outlines and dark shadows touching
    the background (they must stay opaque black, not turn see-through)"""
    navy = (16 / 255, 25 / 255, 61 / 255)
    im = art(navy, w, h)
    d = ImageDraw.Draw(im)
    d.rectangle([40, 40, w - 40, h - 40], outline=(5, 5, 5), width=4)
    d.polygon([(470, 300), (560, 260), (650, 300), (560, 340)], fill=(240, 240, 235), outline=(0, 0, 0), width=5)
    d.ellipse([90, 360, 250, 470], fill=(8, 10, 14))
    return im.filter(ImageFilter.GaussianBlur(0.7))


# ------------------------------------------------------------------ cases
CASES = [
    # name, input, params
    ('hitam_bulat35_rapi', 'chart', dict(shirt='hitam', lpi=35, shape='bulat', grid='rapi')),
    ('hitam_kotak55', 'chart', dict(shirt='hitam', lpi=55, shape='kotak')),
    ('hitam_silang85', 'chart', dict(shirt='hitam', lpi=85, shape='silang')),
    ('hitam_garis25', 'chart', dict(shirt='hitam', lpi=25, shape='garis')),
    ('putih_bulat35_rapi', 'chart', dict(shirt='putih', lpi=35, shape='bulat', grid='rapi')),
    ('putih_elips65', 'chart', dict(shirt='putih', lpi=65, shape='elips')),
    ('putih_berlian45', 'chart', dict(shirt='putih', lpi=45, shape='berlian')),
    ('putih_gcr35', 'chart', dict(shirt='putih', lpi=35, k='gcr', grid='rapi')),
    ('putih_kgaris35', 'chart', dict(shirt='putih', lpi=35, k='garis', grid='rapi')),
    ('hitam_bulatmurni40_600dpi', 'ramp', dict(shirt='hitam', lpi=40, shape='bulat-murni', dpi=600)),
    ('putih_ramp30', 'ramp', dict(shirt='putih', lpi=30, shape='bulat')),
    ('hitam_art_tepi', 'art_black', dict(shirt='hitam', lpi=35, grid='rapi', edges=True)),
    ('putih_art_tepi', 'art_white', dict(shirt='putih', lpi=35, grid='rapi', edges=True)),
    ('hitam_art_fm', 'art_black', dict(shirt='hitam', pattern='fm')),
    ('putih_art_grain', 'art_white', dict(shirt='putih', pattern='grain-sedang')),
    ('hitam_art_grainkasar', 'art_black', dict(shirt='hitam', pattern='grain-kasar')),
    ('navy_rgba', 'art_rgba', dict(shirt=[0x1C / 255, 0x25 / 255, 0x41 / 255], lpi=35, grid='rapi')),
    ('merah_rgba_knockout', 'art_rgba', dict(shirt=[0xB0 / 255, 0x1E / 255, 0x28 / 255], lpi=35, grid='rapi', knockout=0.25)),
    ('putih_rgba', 'art_rgba', dict(shirt='putih', lpi=35, grid='rapi')),
    ('teal_hapusbg_putih', 'art_teal', dict(shirt='putih', lpi=35, grid='rapi', remove_bg=True)),
    ('navy_hapusbg_warna', 'art_navy', dict(shirt=[16 / 255, 25 / 255, 61 / 255], lpi=35, grid='rapi', remove_bg=True)),
    ('resize_up_hitam', 'art_rgba', dict(shirt='hitam', lpi=35, grid='rapi', resize=[959, 685])),
    ('resize_down_putih', 'art_black', dict(shirt='putih', lpi=45, resize=[549, 427])),
    # wavy lines, crosshatch, rough (analog) dots
    ('hitam_gelombang35', 'chart', dict(shirt='hitam', lpi=35, shape='gelombang')),
    ('putih_gelombang25_rapi', 'chart', dict(shirt='putih', lpi=25, shape='gelombang', grid='rapi')),
    ('putih_gelombang55_600dpi', 'ramp', dict(shirt='putih', lpi=55, shape='gelombang', dpi=600)),
    ('hitam_arsir25_rapi', 'chart', dict(shirt='hitam', lpi=25, shape='arsir', grid='rapi')),
    ('putih_arsir45', 'chart', dict(shirt='putih', lpi=45, shape='arsir')),
    ('hitam_arsir65_600dpi', 'ramp', dict(shirt='hitam', lpi=65, shape='arsir', dpi=600)),
    ('hitam_kasar35_rapi', 'chart', dict(shirt='hitam', lpi=35, shape='kasar', grid='rapi')),
    ('putih_kasar55', 'chart', dict(shirt='putih', lpi=55, shape='kasar')),
    ('hitam_kasar45_600dpi_rapi', 'ramp', dict(shirt='hitam', lpi=45, shape='kasar', dpi=600, grid='rapi')),
    ('putih_art_kasar_gcr', 'art_white', dict(shirt='putih', lpi=35, shape='kasar', grid='rapi', k='gcr', edges=True)),
    ('navy_rgba_arsir', 'art_rgba', dict(shirt=[0x1C / 255, 0x25 / 255, 0x41 / 255], lpi=30, shape='arsir')),
    # below 300 dpi (the calibration ramp used to be empty there)
    ('putih_bulat35_150dpi', 'chart', dict(shirt='putih', lpi=35, dpi=150)),
    ('hitam_elips30_200dpi', 'art_black', dict(shirt='hitam', lpi=30, shape='elips', dpi=200)),
    # screen angle / rosette options, big decorative dots (per-class threshold table)
    ('putih_bulat35_sudut45', 'chart', dict(shirt='putih', lpi=35, angle=45)),
    ('putih_dotondot35', 'chart', dict(shirt='putih', lpi=35, rosette=False)),
    ('hitam_rosette30_sudut15', 'chart', dict(shirt='hitam', lpi=30, angle=15, rosette=True)),
    ('hitam_bulat5_besar', 'art_black', dict(shirt='hitam', lpi=5)),
    ('putih_garis8_600dpi_sudut120', 'ramp', dict(shirt='putih', lpi=8, shape='garis', dpi=600, angle=120)),
    ('hitam_kasar6', 'art_black', dict(shirt='hitam', lpi=6, shape='kasar')),
    ('navy_elips12_sudut75', 'art_rgba', dict(shirt=[0x1C / 255, 0x25 / 255, 0x41 / 255], lpi=12, shape='elips', angle=75)),
    # decorative shapes
    ('hitam_bintang6', 'art_black', dict(shirt='hitam', lpi=6, shape='bintang', angle=0)),
    ('putih_segitiga12_sudut30', 'chart', dict(shirt='putih', lpi=12, shape='segitiga', angle=30)),
    ('putih_segitiga20_sudut200', 'chart', dict(shirt='putih', lpi=20, shape='segitiga', angle=200)),
    ('hitam_rantai25_600dpi', 'ramp', dict(shirt='hitam', lpi=25, shape='rantai', dpi=600)),
    ('putih_bintang18_rapi', 'chart', dict(shirt='putih', lpi=18, shape='bintang', grid='rapi')),
    # halftone only on fades (true colours elsewhere)
    ('hitam_gradasi50', 'art_black', dict(shirt='hitam', lpi=35, grid='rapi', halftone_area=0.5)),
    ('putih_gradasi30', 'chart', dict(shirt='putih', lpi=35, halftone_area=0.3)),
    ('navy_gradasi60_knockout', 'art_rgba', dict(shirt=[0x1C / 255, 0x25 / 255, 0x41 / 255], lpi=35, grid='rapi', knockout=0.2, halftone_area=0.6)),
    ('hitam_gradasi100_fm', 'art_black', dict(shirt='hitam', pattern='fm', halftone_area=1.0)),
    ('putih_gradasi40_grain_tepi', 'art_white', dict(shirt='putih', pattern='grain-halus', edges=True, halftone_area=0.4)),
    ('hitam_gradasi50_tepi', 'art_black', dict(shirt='hitam', lpi=35, edges=True, halftone_area=0.5)),
    ('hitam_gradasi50_bintang8', 'art_black', dict(shirt='hitam', lpi=8, shape='bintang', angle=0, halftone_area=0.5)),
    ('putih_gradasi70_600dpi', 'ramp', dict(shirt='putih', lpi=45, dpi=600, halftone_area=0.7)),
    # no halftone at all (true colours, soft edges flattened onto the shirt)
    ('hitam_tanpa_halftone', 'art_black', dict(shirt='hitam', halftone_area=0.0)),
    ('putih_tanpa_halftone_tepi', 'chart', dict(shirt='putih', edges=True, halftone_area=0.0)),
    ('navy_tanpa_halftone_knockout', 'art_rgba', dict(shirt=[0x1C / 255, 0x25 / 255, 0x41 / 255], knockout=0.2, halftone_area=0.0)),
    # bicubic resize (up and down)
    ('resize_up_bicubic_hitam', 'art_rgba', dict(shirt='hitam', lpi=35, grid='rapi', resize=[1517, 1084], resample='bicubic')),
    ('resize_down_bicubic_putih', 'art_black', dict(shirt='putih', lpi=45, resize=[611, 475], resample='bicubic')),
    ('resize_up_bicubic_mati', 'art_rgba', dict(shirt='hitam', resize=[1210, 864], resample='bicubic', halftone_area=0.0)),
    ('hitam_tanpa_halftone_ambang30', 'art_black', dict(shirt='hitam', halftone_area=0.0, shirt_cut=0.3)),
    ('putih_tanpa_halftone_ambang0', 'art_white', dict(shirt='putih', halftone_area=0.0, shirt_cut=0.0)),
    # eraser / restore brush (after background removal, before resize)
    ('hitam_coretan_hapus', 'art_black', dict(shirt='hitam', lpi=35, grid='rapi', erase=[
        dict(mode='hapus', r=18.5, pts=[[120.25, 140.5], [300.75, 180.0], [420.0, 420.5], [610.5, 400.25]]),
        dict(mode='hapus', r=40.0, pts=[[700.5, 300.5]]),
        dict(mode='hapus', r=25.0, pts=[[-30.0, 650.0], [120.0, 760.0]]),
        dict(mode='kembalikan', r=9.0, pts=[[300.0, 120.0], [300.0, 260.0]])])),
    ('teal_hapusbg_coretan_kembalikan', 'art_teal', dict(shirt='putih', lpi=35, grid='rapi', remove_bg=True, erase=[
        dict(mode='kembalikan', r=30.0, pts=[[20.0, 20.0], [200.0, 60.5]]),
        dict(mode='hapus', r=12.25, pts=[[350.0, 100.0], [350.5, 400.0], [500.0, 420.0]]),
        dict(mode='kembalikan', r=0.5, pts=[[600.5, 450.5]]),
        dict(mode='hapus', r=60.0, pts=[[690.0, 10.0], [690.0, 10.0]])], resize=[1050, 750])),
    # dither patterns and the neat interlock halftone
    ('hitam_bayer', 'art_black', dict(shirt='hitam', pattern='bayer')),
    ('putih_bayer_600dpi', 'ramp', dict(shirt='putih', pattern='bayer', dpi=600)),
    ('hitam_difusi', 'art_black', dict(shirt='hitam', pattern='difusi')),
    ('putih_difusi_chart', 'chart', dict(shirt='putih', pattern='difusi')),
    ('navy_difusi_knockout', 'art_rgba', dict(shirt=[0x1C / 255, 0x25 / 255, 0x41 / 255], pattern='difusi', knockout=0.2)),
    ('putih_difusi_360dpi', 'ramp', dict(shirt='putih', pattern='difusi', dpi=360)),
    ('hitam_interlock35', 'art_black', dict(shirt='hitam', pattern='interlock', lpi=35)),
    ('putih_interlock_chart', 'chart', dict(shirt='putih', pattern='interlock', lpi=30)),
    ('putih_interlock_sudut15_kasar', 'ramp', dict(shirt='putih', pattern='interlock', lpi=20, angle=15, shape='kasar')),
    ('navy_interlock', 'art_rgba', dict(shirt=[0x1C / 255, 0x25 / 255, 0x41 / 255], pattern='interlock', lpi=35)),
    ('hitam_gradasi_difusi', 'art_black', dict(shirt='hitam', pattern='difusi', halftone_area=0.5)),
    ('hitam_gradasi_interlock', 'art_black', dict(shirt='hitam', pattern='interlock', halftone_area=0.5)),
    ('putih_gradasi_bayer', 'chart', dict(shirt='putih', pattern='bayer', halftone_area=0.4)),
    # turn / mirror (before resize and halftone: the screen angle stays as set)
    ('hitam_putar90', 'art_black', dict(shirt='hitam', lpi=35, grid='rapi', rotate=90)),
    ('putih_putar270_cermin', 'chart', dict(shirt='putih', lpi=30, rotate=270, mirror=True)),
    ('navy_putar180_resize', 'art_rgba', dict(shirt=[0x1C / 255, 0x25 / 255, 0x41 / 255], lpi=35, rotate=180, resize=[910, 650])),
    ('hitam_cermin_gradasi', 'art_black', dict(shirt='hitam', mirror=True, halftone_area=0.5)),
]
INPUTS = {
    'chart': chart, 'ramp': ramp,
    'art_black': lambda: art((0, 0, 0)), 'art_white': lambda: art((1, 1, 1)),
    'art_rgba': art_rgba, 'art_teal': art_teal, 'art_navy': art_navy,
}


def run_case(inp_path, p):
    c, a = Z.load(inp_path)
    c0, a0 = c, a
    extra = {}
    if p.get('remove_bg'):
        c, a, bg = Z.remove_bg(c)
        extra['bg'] = [float(v) for v in bg]
    if p.get('erase'):
        c, a = Z.apply_strokes(c, a, c0, a0, p['erase'])
    if p.get('rotate') or p.get('mirror'):
        c, a = Z.orient(c, a, p.get('rotate', 0), p.get('mirror', False))
    if p.get('resize'):
        c, a = Z.premul_resize(c, a, tuple(p['resize']), p.get('resample', 'lanczos'))
    shirt = p['shirt'] if isinstance(p['shirt'], str) else np.array(p['shirt'], np.float32)
    o = Z.render(c, a, shirt, lpi=p.get('lpi', 35), shape=p.get('shape', 'bulat'), dpi=p.get('dpi', 300),
                 grid=p.get('grid', 'presisi'), pattern=p.get('pattern', 'am'), k=p.get('k', 'off'),
                 knockout=p.get('knockout', 0.0), edges=p.get('edges', False),
                 angle=p.get('angle'), rosette=p.get('rosette'), halftone_area=p.get('halftone_area'),
                 shirt_cut=p.get('shirt_cut', 0.12))
    box = Z.autotrim(c, a, shirt)
    return o, extra, box


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--expected', default=None)
    a = ap.parse_args()
    write_assets()
    gdir = os.path.join(ENGINE, 'tests', 'golden'); os.makedirs(gdir, exist_ok=True)
    for name, fn in INPUTS.items():
        fn().save(os.path.join(gdir, f'in_{name}.png'))
    out = []
    for name, inp, p in CASES:
        o, extra, box = run_case(os.path.join(gdir, f'in_{inp}.png'), p)
        ch = Z.checks(o, p.get('dpi', 300), pure=p.get('halftone_area') is None)
        assert ch == dict(semi=0, impure=0, tiny=0, hidden=0), (name, ch)
        h = hashlib.sha256(np.ascontiguousarray(o).tobytes()).hexdigest()
        out.append(dict(name=name, input=f'in_{inp}.png', params=p, sha256=h, width=int(o.shape[1]),
                        height=int(o.shape[0]), autotrim=box, **extra))
        if a.expected:
            os.makedirs(a.expected, exist_ok=True)
            Image.fromarray(o, 'RGBA').save(os.path.join(a.expected, f'{name}.png'))
        print(name, o.shape, h[:16], flush=True)
    json.dump(out, open(os.path.join(gdir, 'cases.json'), 'w'), indent=1)


if __name__ == '__main__':
    main()
