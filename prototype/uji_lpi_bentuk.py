"""Sweep test: every shape x LPI x shirt on a 60-colour chart and a 0-100 % ramp.
Reproduces docs/RENCANA-DTF.md section 9b.

  python uji_lpi_bentuk.py                 # all 196 combos @300 dpi (~8 min)
  python uji_lpi_bentuk.py --lpi 35 55     # only some LPIs
"""
import argparse
import json
import os
import sys

import numpy as np
from scipy import ndimage as ndi

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import studiozea_ref as Z  # noqa: E402

f32 = np.float32
P, GX, GY = 236, 10, 6          # 2 cm patches @300 dpi


def chart_colours():
    cols = [(g, g, g) for g in np.linspace(0, 1, 11)]
    for c in [(1, 0, 0), (0, 1, 0), (0, 0, 1), (0, 1, 1), (1, 0, 1), (1, 1, 0)]:
        for k in (0.25, 0.5, 0.75, 1.0):
            cols.append(tuple(np.array(c) * k))
    cols += [(0.96, 0.80, 0.69), (0.87, 0.67, 0.53), (0.76, 0.53, 0.38), (0.55, 0.36, 0.24), (0.36, 0.22, 0.15),
             (0.98, 0.85, 0.88), (0.80, 0.92, 0.98), (0.88, 0.98, 0.84), (1.0, 0.96, 0.75), (0.90, 0.85, 1.0),
             (0.11, 0.15, 0.25), (0.25, 0.10, 0.10), (0.10, 0.22, 0.12), (0.22, 0.16, 0.30), (0.18, 0.18, 0.18),
             (0.80, 0.40, 0.10), (0.20, 0.50, 0.70), (0.55, 0.70, 0.20), (0.60, 0.25, 0.55), (0.95, 0.75, 0.20),
             (0.45, 0.30, 0.20), (0.30, 0.65, 0.60), (0.85, 0.20, 0.25), (0.40, 0.40, 0.75), (0.70, 0.60, 0.45)]
    return np.array(cols[:60], f32)


def run(mode, lpi, shape, dpi=300):
    cols = chart_colours()
    chart = np.zeros((GY * P, GX * P, 3), f32)
    for i, c in enumerate(cols):
        y, x = divmod(i, GX); chart[y * P:(y + 1) * P, x * P:(x + 1) * P] = c
    one = np.ones(chart.shape[:2], f32)
    shirt_rgb = (0, 0, 0) if mode == 'hitam' else (1, 1, 1)
    o = Z.render(chart, one, mode, lpi, shape, dpi)
    img = Z.on_shirt(o, shirt_rgb)
    m = np.array([img[y * P + 30:(y + 1) * P - 30, x * P + 30:(x + 1) * P - 30].reshape(-1, 3).mean(0)
                  for y, x in (divmod(i, GX) for i in range(len(cols)))])
    dE = np.linalg.norm(Z.lab(np.clip(m, 0, 1)) - Z.lab(cols), axis=-1)
    ramp = np.tile(np.linspace(0, 1, 3000, dtype=f32), (160, 1))
    orr = Z.render(np.dstack([ramp] * 3), np.ones(ramp.shape, f32), mode, lpi, shape, dpi)
    al = orr[..., 3] > 0; t = np.linspace(0, 1, 3000); worst = 0.0
    for c in range(3):
        on = (orr[..., c] > 127) & al if mode == 'hitam' else (orr[..., c] < 128) & al
        cov = ndi.uniform_filter1d(on.mean(0).astype(f32), 61)
        worst = max(worst, float(np.abs(cov - (t if mode == 'hitam' else 1 - t))[100:-100].max()))
    return dict(kaos=mode, lpi=lpi, bentuk=shape, dpi=dpi, dE_mean=float(dE.mean()), dE_max=float(dE.max()),
                galat_nada=worst, **Z.checks(o, dpi))


if __name__ == '__main__':
    ap = argparse.ArgumentParser()
    ap.add_argument('--lpi', type=int, nargs='*', default=[15, 20, 25, 30, 35, 40, 45, 50, 55, 60, 65, 75, 85])
    ap.add_argument('--bentuk', nargs='*', default=list(Z.SPOT))
    ap.add_argument('--kaos', nargs='*', default=['hitam', 'putih'])
    ap.add_argument('--out', default='hasil_uji.json')
    a = ap.parse_args()
    res = []
    for mode in a.kaos:
        for sh in a.bentuk:
            for l in a.lpi:
                r = run(mode, l, sh); res.append(r)
                print(f"{mode:5s} {sh:12s} {l:3d} LPI  dE {r['dE_mean']:.2f} (maks {r['dE_max']:.1f})  "
                      f"galat nada {r['galat_nada'] * 100:.1f}%  semi {r['semi']}  titik kecil {r['tiny']}", flush=True)
    json.dump(res, open(a.out, 'w'), indent=1)
