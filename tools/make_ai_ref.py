"""Reference data for engine/tests/upscale.rs: synthetic test images (no third-party
artwork) and their Real-ESRGAN Compact x4 result from PyTorch (whole image, no tiles).

Needs torch + spandrel and the two original weight files (see tools/convert_esrgan.py):
    python tools/make_ai_ref.py realesr-general-x4v3.pth realesr-general-wdn-x4v3.pth
"""
import sys
import numpy as np
import torch
import spandrel
from PIL import Image

OUT = 'engine/tests/data'


def synthetic(w, h, seed, alpha):
    rng = np.random.default_rng(seed)
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    img = np.zeros((h, w, 4), np.float32)
    img[..., 0] = x / w
    img[..., 1] = y / h
    img[..., 2] = 0.5 + 0.5 * np.sin(x / 3.0) * np.cos(y / 4.0)
    # a hard-edged disc, a stripe pattern and some JPEG-like noise
    disc = (x - w * 0.6) ** 2 + (y - h * 0.4) ** 2 < (min(w, h) * 0.25) ** 2
    img[disc, :3] = [0.95, 0.85, 0.1]
    img[((x.astype(int) // 2) % 2 == 0) & (y > h * 0.7), :3] *= 0.2
    img[..., :3] += rng.normal(0, 0.03, (h, w, 3))
    img[..., 3] = 1.0
    if alpha:
        img[..., 3] = np.clip((x - w * 0.2) / (w * 0.3), 0, 1)
        img[: h // 3, : w // 4, 3] = 0.0
    return (np.clip(img, 0, 1) * 255).round().astype(np.uint8)


def model(general, wdn, dn):
    a = spandrel.ModelLoader().load_from_file(general)
    b = spandrel.ModelLoader().load_from_file(wdn)
    sa, sb = a.model.state_dict(), b.model.state_dict()
    a.model.load_state_dict({k: dn * sa[k] + (1 - dn) * sb[k] for k in sa})
    return a.model.eval()


def run(m, rgb):
    x = torch.from_numpy(np.ascontiguousarray(rgb)).permute(2, 0, 1)[None]
    with torch.inference_mode():
        return m(x)[0].permute(1, 2, 0).numpy()


def to8(v):
    return (np.clip(v, 0, 1) * 255).round().astype(np.uint8)


if __name__ == '__main__':
    general, wdn = sys.argv[1:3]
    for name, (w, h, seed, alpha, dn) in {
        'ai_in_rgb': (52, 38, 1, False, 0.5),
        'ai_in_rgba': (40, 34, 2, True, 1.0),
    }.items():
        px = synthetic(w, h, seed, alpha)
        Image.fromarray(px if alpha else px[..., :3]).save(f'{OUT}/{name}.png')
        f = px.astype(np.float32) / 255
        m = model(general, wdn, dn)
        out = to8(run(m, f[..., :3]))
        if alpha:
            g = run(m, np.repeat(f[..., 3:4], 3, axis=2))
            out = np.dstack([out, to8(g.mean(axis=2))])
        Image.fromarray(out).save(f'{OUT}/{name.replace("_in_", "_x4_")}_dn{dn}.png')
        print(name, w, h, '->', out.shape)
