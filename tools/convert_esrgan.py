"""Convert the Real-ESRGAN Compact weights (realesr-general-x4v3 + its "wdn" twin, BSD-3-Clause,
https://github.com/xinntao/Real-ESRGAN/releases/tag/v0.2.5.0) into the raw format engine/src/upscale.rs
embeds: little-endian f32 in state_dict order (per conv: weight [out, in, 3, 3] then bias [out];
per PReLU: weight [64]). Needs torch (any version), no network.

    python tools/convert_esrgan.py realesr-general-x4v3.pth engine/models/realesr-general-x4v3.bin
    python tools/convert_esrgan.py realesr-general-wdn-x4v3.pth engine/models/realesr-general-wdn-x4v3.bin
"""
import sys
import numpy as np
import torch

src, dst = sys.argv[1:3]
sd = torch.load(src, map_location='cpu', weights_only=True)
sd = sd.get('params_ema', sd.get('params', sd))
keys = sorted(sd, key=lambda k: (int(k.split('.')[1]), k.split('.')[2] != 'weight'))
convs = prelus = 0
with open(dst, 'wb') as f:
    for k in keys:
        a = sd[k].float().numpy().astype('<f4')
        if k.endswith('weight') and a.ndim == 4:
            convs += 1
        elif a.ndim == 1 and not k.endswith('bias'):
            prelus += 1
        f.write(a.tobytes())
print(dst, 'convs', convs, 'prelus', prelus)
