"""Command line for the reference engine.

Examples
  python render.py desain.png hasil.png --kaos hitam --lpi 35 --lebar-cm 30
  python render.py desain.png hasil.png --kaos putih --bentuk elips --lpi 45 --k gcr
  python render.py desain.png hasil.png --kaos "#1C2541" --hapus-bg warna
"""
import argparse
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import studiozea_ref as Z  # noqa: E402


def parse_shirt(s):
    if s in ('hitam', 'putih'):
        return s
    s = s.lstrip('#')
    return np.array([int(s[i:i + 2], 16) for i in (0, 2, 4)], np.float32) / 255


def main(argv=None):
    p = argparse.ArgumentParser(description='StudioZea reference engine (DTF)')
    p.add_argument('input'); p.add_argument('output')
    p.add_argument('--kaos', default='hitam', help="hitam | putih | #RRGGBB")
    p.add_argument('--lpi', type=float, default=35)
    p.add_argument('--bentuk', default='bulat', choices=list(Z.SPOT))
    p.add_argument('--pola', default='am', help='am | fm | grain-halus | grain-sedang | grain-kasar')
    p.add_argument('--grid', default='auto', choices=['auto', 'rapi', 'presisi'])
    p.add_argument('--dpi', type=int, default=300)
    p.add_argument('--lebar-cm', type=float, default=None, help='print width; height follows the ratio')
    p.add_argument('--hapus-bg', default='otomatis', choices=['otomatis', 'warna', 'tidak'],
                   help="otomatis: shirt colour does it (black/white bg) | warna: flat coloured bg")
    p.add_argument('--k', default='off', choices=['off', 'garis', 'gcr'])
    p.add_argument('--tepi-tajam', action='store_true')
    p.add_argument('--knockout', type=float, default=0.0, help='coloured shirt: knock out shirt-like colours')
    p.add_argument('--trim', action='store_true', help='crop to the printable content')
    a = p.parse_args(argv)

    shirt = parse_shirt(a.kaos)
    if a.grid == 'auto':
        a.grid = Z.choose_grid(a.dpi, a.lpi)
    c, al = Z.load(a.input)
    if a.hapus_bg == 'warna':
        c, al, bg = Z.remove_bg(c)
        print('background', (bg * 255).round().astype(int))
    if a.trim:
        box = Z.autotrim(c, al, shirt)
        if box:
            x0, y0, x1, y1 = box; c, al = c[y0:y1, x0:x1], al[y0:y1, x0:x1]
    if a.lebar_cm:
        w = round(a.lebar_cm / 2.54 * a.dpi); h = round(w * c.shape[0] / c.shape[1])
        c, al = Z.premul_resize(c, al, (w, h))
    o = Z.render(c, al, shirt, lpi=a.lpi, shape=a.bentuk, dpi=a.dpi, grid=a.grid, pattern=a.pola,
                 k=a.k, knockout=a.knockout, edges=a.tepi_tajam)
    Z.save_png(o, a.output, a.dpi)
    info = Z.screen_info(a.dpi, a.lpi, 22.5, a.grid)
    print(f"{o.shape[1]}x{o.shape[0]} px = {o.shape[1] / a.dpi * 2.54:.1f} x {o.shape[0] / a.dpi * 2.54:.1f} cm @ {a.dpi} dpi; "
          f"screen {info['lpi']:.2f} LPI {info['angle']:.2f} deg ({info['levels']} levels); checks {Z.checks(o, a.dpi)}")


if __name__ == '__main__':
    sys.exit(main())
