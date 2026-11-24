//! AI upscale for low-resolution artwork: Real-ESRGAN Compact (SRVGGNetCompact,
//! `realesr-general-x4v3`, Xintao Wang et al., BSD-3-Clause, see models/LICENSE-Real-ESRGAN).
//!
//! The network is small (34 3x3 convolutions, 1.2 M weights), so it runs here in plain
//! Rust on the CPU. Each convolution uses Winograd F(2x2, 3x3): 2 x 2 output pixels from a
//! 4 x 4 input patch with 16 instead of 36 multiplications per channel pair, as 16 matrix
//! products (`gemm`, AVX2 / AVX-512 picked at run time). The image is cut into tiles with
//! 34 px of context (the network's full reach), so tiles leave no seams: tiled = whole
//! image (but for a rare last-bit rounding). Tiles run in parallel.
//!
//! `denoise` blends the two published weight sets like Real-ESRGAN's `--denoise_strength`:
//! 1 = smooth (cleans JPEG noise), 0 = keeps fine texture and noise.

use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{bail, Result};
use rayon::prelude::*;

use crate::io::Rgba8;

/// the network enlarges 4 times; `Scale::X2` averages 2 x 2 blocks of that (clamped)
pub const NET_SCALE: usize = 4;
const FEAT: usize = 64;
const BODY: usize = 32;
const OUT_CH: usize = 3 * NET_SCALE * NET_SCALE;
/// input pixels per tile side
pub const TILE: usize = 256;
/// context around a tile: every output pixel sees 34 input pixels around it
/// (34 convolutions of 3 x 3), so with this margin tiles equal the whole image
pub const PAD: usize = 34;
/// largest result accepted (pixels): the app prepares the image at ~60 bytes per pixel
pub const MAX_OUT_PX: usize = 40_000_000;
/// Winograd tiles per block of matrix products (keeps the block in cache)
const WBLOCK: usize = 256;

static GENERAL: &[u8] = include_bytes!("../models/realesr-general-x4v3.bin");
static WDN: &[u8] = include_bytes!("../models/realesr-general-wdn-x4v3.bin");

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Scale {
    X2,
    X4,
}

impl Scale {
    pub fn factor(self) -> usize {
        match self {
            Scale::X2 => 2,
            Scale::X4 => 4,
        }
    }
}

struct Conv {
    cin: usize,
    cout: usize,
    /// Winograd filter U = G g G^T: per element (0..16) a [cin][cout] matrix
    u: Vec<f32>,
    b: Vec<f32>,
}

pub struct Model {
    convs: Vec<Conv>,
    /// PReLU slope per channel after every conv but the last
    prelu: Vec<Vec<f32>>,
    /// per conv: weights in torch layout [cout][cin][3][3] and bias (for the GPU)
    #[cfg_attr(not(feature = "gpu"), allow(dead_code))]
    pub(crate) raw: Vec<(usize, usize, Vec<f32>, Vec<f32>)>,
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// G g G^T for one 3 x 3 kernel (row-major), G = [1 0 0; .5 .5 .5; .5 -.5 .5; 0 0 1]
fn filter_transform(g: &[f32]) -> [f32; 16] {
    let gm = [[1.0, 0.0, 0.0], [0.5, 0.5, 0.5], [0.5, -0.5, 0.5], [0.0, 0.0, 1.0f32]];
    let mut t = [[0.0f32; 3]; 4]; // G g
    for i in 0..4 {
        for j in 0..3 {
            t[i][j] = (0..3).map(|k| gm[i][k] * g[k * 3 + j]).sum();
        }
    }
    let mut u = [0.0f32; 16]; // (G g) G^T
    for i in 0..4 {
        for j in 0..4 {
            u[i * 4 + j] = (0..3).map(|k| t[i][k] * gm[j][k]).sum();
        }
    }
    u
}

impl Model {
    /// `denoise` 0..1 (Real-ESRGAN's default is 0.5)
    pub fn new(denoise: f32) -> Model {
        let dn = denoise.clamp(0.0, 1.0);
        let (g, n) = (floats(GENERAL), floats(WDN));
        // weights = dn * general + (1 - dn) * wdn, in f32 like torch
        let all: Vec<f32> = g.iter().zip(&n).map(|(a, b)| dn * a + (1.0 - dn) * b).collect();
        let mut at = 0;
        let mut take = |len: usize| {
            let s = all[at..at + len].to_vec();
            at += len;
            s
        };
        let mut convs = Vec::new();
        let mut prelu = Vec::new();
        let mut raws = Vec::new();
        let shapes = std::iter::once((3, FEAT))
            .chain(std::iter::repeat_n((FEAT, FEAT), BODY))
            .chain(std::iter::once((FEAT, OUT_CH)));
        for (i, (cin, cout)) in shapes.enumerate() {
            let raw = take(cout * cin * 9); // torch layout [cout][cin][3][3]
            let mut u = vec![0.0; 16 * cin * cout];
            for co in 0..cout {
                for ci in 0..cin {
                    let k = filter_transform(&raw[(co * cin + ci) * 9..][..9]);
                    for (e, v) in k.iter().enumerate() {
                        u[(e * cin + ci) * cout + co] = *v;
                    }
                }
            }
            let b = take(cout);
            raws.push((cin, cout, raw, b.clone()));
            convs.push(Conv { cin, cout, u, b });
            if i <= BODY {
                prelu.push(take(FEAT));
            }
        }
        assert_eq!(at, all.len(), "model file size");
        Model { convs, prelu, raw: raws }
    }

    /// PReLU slopes after conv `i` (None after the last)
    #[cfg_attr(not(feature = "gpu"), allow(dead_code))]
    pub(crate) fn prelu(&self, i: usize) -> Option<&[f32]> {
        self.prelu.get(i).map(|v| v.as_slice())
    }

    /// run on one tile: `rgb` h x w x 3 in 0..1, zero padding at the tile edge like the
    /// reference; returns the 4x result (not clamped) of the input pixels `keep`
    /// (x0, y0, x1, y1), (4 (x1 - x0)) x (4 (y1 - y0)) x 3
    pub fn forward(&self, rgb: &[f32], w: usize, h: usize, keep: [usize; 4]) -> Vec<f32> {
        // even size for the 2 x 2 output tiles, one zero pixel around: (h2 + 2) x (w2 + 2)
        let (h2, w2) = (h + (h & 1), w + (w & 1));
        let pw = w2 + 2;
        let npx = (h2 + 2) * pw;
        let at = |y: usize, x: usize| y * pw + x;
        let mut cur = vec![0.0f32; npx * 3];
        for y in 0..h {
            let d = at(y + 1, 1) * 3;
            cur[d..d + w * 3].copy_from_slice(&rgb[y * w * 3..(y + 1) * w * 3]);
        }
        // borders are never written, so they stay zero; the two feature buffers swap
        let mut fa = vec![0.0f32; npx * FEAT];
        let mut fb = vec![0.0f32; npx * FEAT];
        let mut scratch = Scratch::default();
        let last = self.convs.len() - 1;
        for (i, c) in self.convs.iter().enumerate() {
            let (src, dst): (&[f32], &mut [f32]) = if i == 0 { (&cur, &mut fa) } else { (&fa, &mut fb[..npx * c.cout]) };
            conv3x3(src, dst, c, w2, h2, &mut scratch);
            if i == last {
                break;
            }
            // the extra row / column of an odd size stays zero (zero padding)
            if h2 != h {
                dst[at(h + 1, 0) * FEAT..at(h + 2, 0) * FEAT].fill(0.0);
            }
            if w2 != w {
                for y in 1..=h {
                    dst[at(y, w + 1) * FEAT..at(y, w + 2) * FEAT].fill(0.0);
                }
            }
            let s = &self.prelu[i];
            for v in dst[at(1, 0) * FEAT..at(h + 1, 0) * FEAT].chunks_exact_mut(FEAT) {
                for (x, &k) in v.iter_mut().zip(s) {
                    if *x < 0.0 {
                        *x *= k;
                    }
                }
            }
            if i > 0 {
                std::mem::swap(&mut fa, &mut fb);
            }
        }
        shuffle(&fb[..npx * OUT_CH], |x, y| at(y + 1, x + 1), rgb, w, keep)
    }
}

/// pixel shuffle (channel c * 16 + i * 4 + j -> pixel (4y + i, 4x + j)) plus the input
/// pixel, for the input pixels `keep`; `at(x, y)` = pixel index in `feat` (48 per pixel)
pub(crate) fn shuffle(feat: &[f32], at: impl Fn(usize, usize) -> usize, rgb: &[f32], w: usize, keep: [usize; 4]) -> Vec<f32> {
    let [x0, y0, x1, y1] = keep;
    let (s, ow) = (NET_SCALE, (x1 - x0) * NET_SCALE);
    let mut out = vec![0.0f32; ow * (y1 - y0) * s * 3];
    for y in y0..y1 {
        for x in x0..x1 {
            let f = &feat[at(x, y) * OUT_CH..][..OUT_CH];
            let base = &rgb[(y * w + x) * 3..][..3];
            for i in 0..s {
                for j in 0..s {
                    let o = (((y - y0) * s + i) * ow + (x - x0) * s + j) * 3;
                    for c in 0..3 {
                        out[o + c] = f[c * 16 + i * 4 + j] + base[c];
                    }
                }
            }
        }
    }
    out
}

#[derive(Default)]
struct Scratch {
    /// transformed input, per Winograd tile 16 x cin
    v: Vec<f32>,
    /// products, per Winograd tile 16 x cout
    m: Vec<f32>,
}

/// channels handled together by the transforms (one AVX register)
const LANES: usize = 8;

/// input transform V = B^T d B, B^T = [1 0 -1 0; 0 1 1 0; 0 -1 1 0; 0 1 0 -1], for
/// channels ch..ch + N of the 4 x 4 patch d (row-major); V element e at v[e * cin + ch]
#[inline(always)]
fn in_tf<const N: usize>(d: &[&[f32]; 16], ch: usize, v: &mut [f32], cin: usize) {
    let x = |i: usize| -> [f32; N] { d[i][ch..ch + N].try_into().unwrap() };
    let add = |a: [f32; N], b: [f32; N]| -> [f32; N] { std::array::from_fn(|i| a[i] + b[i]) };
    let sub = |a: [f32; N], b: [f32; N]| -> [f32; N] { std::array::from_fn(|i| a[i] - b[i]) };
    let mut t = [[0.0f32; N]; 16];
    for q in 0..4 {
        t[q] = sub(x(q), x(8 + q));
        t[4 + q] = add(x(4 + q), x(8 + q));
        t[8 + q] = sub(x(8 + q), x(4 + q));
        t[12 + q] = sub(x(4 + q), x(12 + q));
    }
    for r in 0..4 {
        let (a, b, c, e) = (t[r * 4], t[r * 4 + 1], t[r * 4 + 2], t[r * 4 + 3]);
        for (q, val) in [sub(a, c), add(b, c), sub(c, b), sub(b, e)].iter().enumerate() {
            v[(r * 4 + q) * cin + ch..][..N].copy_from_slice(val);
        }
    }
}

/// output transform Y = A^T M A + bias, A^T = [1 1 1 0; 0 1 -1 -1], for channels
/// ch..ch + N; M element e at m[e * cout + ch]; the 2 x 2 results go to dst at o[0..4]
#[inline(always)]
fn out_tf<const N: usize>(m: &[f32], ch: usize, cout: usize, bias: &[f32], dst: &mut [f32], o: [usize; 4]) {
    let x = |e: usize| -> [f32; N] { m[e * cout + ch..][..N].try_into().unwrap() };
    let add = |a: [f32; N], b: [f32; N]| -> [f32; N] { std::array::from_fn(|i| a[i] + b[i]) };
    let sub = |a: [f32; N], b: [f32; N]| -> [f32; N] { std::array::from_fn(|i| a[i] - b[i]) };
    let b: [f32; N] = bias[ch..ch + N].try_into().unwrap();
    let mut s = [[0.0f32; N]; 8];
    for q in 0..4 {
        s[q] = add(add(x(q), x(4 + q)), x(8 + q));
        s[4 + q] = sub(sub(x(4 + q), x(8 + q)), x(12 + q));
    }
    for i in 0..2 {
        let r = &s[i * 4..i * 4 + 4];
        let y0 = add(add(add(r[0], r[1]), r[2]), b);
        let y1 = add(sub(sub(r[1], r[2]), r[3]), b);
        dst[o[i * 2] + ch..][..N].copy_from_slice(&y0);
        dst[o[i * 2 + 1] + ch..][..N].copy_from_slice(&y1);
    }
}

/// 3 x 3 convolution (+ bias) of an image of h x w (both even) pixels stored with a
/// one-pixel zero border, channels last; writes the interior of `dst` (stride cout)
fn conv3x3(src: &[f32], dst: &mut [f32], c: &Conv, w: usize, h: usize, sc: &mut Scratch) {
    #[cfg(target_arch = "x86_64")]
    if std::arch::is_x86_feature_detected!("avx2") {
        // SAFETY: the CPU has AVX2
        return unsafe { conv3x3_avx2(src, dst, c, w, h, sc) };
    }
    conv3x3_impl(src, dst, c, w, h, sc)
}

/// the same code compiled for AVX2 (the transforms vectorise over 8 channels)
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn conv3x3_avx2(src: &[f32], dst: &mut [f32], c: &Conv, w: usize, h: usize, sc: &mut Scratch) {
    conv3x3_impl(src, dst, c, w, h, sc)
}

#[inline(always)]
fn conv3x3_impl(src: &[f32], dst: &mut [f32], c: &Conv, w: usize, h: usize, sc: &mut Scratch) {
    let (cin, cout) = (c.cin, c.cout);
    let pw = w + 2;
    let tw = w / 2;
    let tiles = tw * (h / 2);
    sc.v.resize(WBLOCK * 16 * cin, 0.0);
    sc.m.resize(WBLOCK * 16 * cout, 0.0);
    let (vl, ml) = (cin - cin % LANES, cout - cout % LANES);
    let mut t0 = 0;
    while t0 < tiles {
        let nb = WBLOCK.min(tiles - t0);
        for k in 0..nb {
            let (ty, tx) = ((t0 + k) / tw, (t0 + k) % tw);
            let d: [&[f32]; 16] = std::array::from_fn(|e| &src[((2 * ty + e / 4) * pw + 2 * tx + e % 4) * cin..][..cin]);
            let v = &mut sc.v[k * 16 * cin..(k + 1) * 16 * cin];
            for ch in (0..vl).step_by(LANES) {
                in_tf::<LANES>(&d, ch, v, cin);
            }
            for ch in vl..cin {
                in_tf::<1>(&d, ch, v, cin);
            }
        }
        // 16 products M_e = V_e U_e: rows = Winograd tiles (stride 16 x channels)
        for e in 0..16 {
            let v = &sc.v[..nb * 16 * cin];
            let m = &mut sc.m[..nb * 16 * cout];
            let u = &c.u[e * cin * cout..][..cin * cout];
            // SAFETY: row r of V_e is v[(r * 16 + e) * cin..][..cin] and of M_e is
            // m[(r * 16 + e) * cout..][..cout], r < nb: inside the slices; u is cin x cout
            unsafe {
                gemm::gemm(
                    nb,
                    cout,
                    cin,
                    m.as_mut_ptr().add(e * cout),
                    1,
                    (16 * cout) as isize,
                    false,
                    v.as_ptr().add(e * cin),
                    1,
                    (16 * cin) as isize,
                    u.as_ptr(),
                    1,
                    cout as isize,
                    0.0f32,
                    1.0f32,
                    false,
                    false,
                    false,
                    gemm::Parallelism::None,
                );
            }
        }
        for k in 0..nb {
            let (ty, tx) = ((t0 + k) / tw, (t0 + k) % tw);
            let at = |i: usize, j: usize| ((2 * ty + i + 1) * pw + 2 * tx + j + 1) * cout;
            let o = [at(0, 0), at(0, 1), at(1, 0), at(1, 1)];
            let m = &sc.m[k * 16 * cout..(k + 1) * 16 * cout];
            for ch in (0..ml).step_by(LANES) {
                out_tf::<LANES>(m, ch, cout, &c.b, dst, o);
            }
            for ch in ml..cout {
                out_tf::<1>(m, ch, cout, &c.b, dst, o);
            }
        }
        t0 += nb;
    }
}

/// result size for an input size
pub fn out_size(w: usize, h: usize, scale: Scale) -> (usize, usize) {
    (w * scale.factor(), h * scale.factor())
}

/// where the AI ran
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Report {
    /// graphics card name, None = processor only
    pub gpu: Option<String>,
    pub gpu_tiles: usize,
    pub cpu_tiles: usize,
    /// why the graphics card stopped (the rest ran on the processor)
    pub fallback: Option<String>,
}

#[cfg(feature = "gpu")]
pub use crate::upscale_gpu::Gpu;
#[cfg(feature = "gpu")]
#[doc(hidden)]
pub use crate::upscale_gpu::SHADER as GPU_SHADER;

/// stand-in when the engine is built without the `gpu` feature: never opens
#[cfg(not(feature = "gpu"))]
pub struct Gpu;

#[cfg(not(feature = "gpu"))]
impl Gpu {
    pub fn open(_allow_software: bool) -> Result<Gpu> {
        bail!("tanpa dukungan kartu grafis")
    }
    pub fn name(&self) -> &str {
        ""
    }
    pub fn self_test(&self) -> Result<()> {
        bail!("tanpa dukungan kartu grafis")
    }
    pub fn tile(&self) -> usize {
        TILE
    }
    #[doc(hidden)]
    pub fn testing(&self, _tile: usize, _fail_after: usize) {}
    pub(crate) fn features(&self, _net: &Model, _rgb: &[f32], _w: usize, _h: usize) -> Result<Vec<f32>> {
        bail!("tanpa dukungan kartu grafis")
    }
}

/// upscale an RGBA8 image on the processor. Colour runs through the network; alpha (only
/// when the image has transparent pixels) runs through it too, as grey (like
/// Real-ESRGAN). `progress` gets (tiles done, tiles total) and returns false to cancel.
pub fn upscale(img: &Rgba8, scale: Scale, denoise: f32, progress: impl Fn(usize, usize) -> bool + Sync) -> Result<Rgba8> {
    upscale_with(img, scale, denoise, None, TILE, progress).map(|r| r.0)
}

/// `upscale` with a chosen tile size (tests: the result does not depend on it)
pub fn upscale_tiled(
    img: &Rgba8,
    scale: Scale,
    denoise: f32,
    tile: usize,
    progress: impl Fn(usize, usize) -> bool + Sync,
) -> Result<Rgba8> {
    upscale_with(img, scale, denoise, None, tile, progress).map(|r| r.0)
}

/// `upscale` on the graphics card when `gpu` is given (its own tile size), tile by tile;
/// if the card fails, the remaining tiles run on the processor (same result)
pub fn upscale_on(
    img: &Rgba8,
    scale: Scale,
    denoise: f32,
    gpu: Option<&Gpu>,
    progress: impl Fn(usize, usize) -> bool + Sync,
) -> Result<(Rgba8, Report)> {
    let tile = gpu.map_or(TILE, |g| g.tile());
    upscale_with(img, scale, denoise, gpu, tile, progress)
}

fn upscale_with(
    img: &Rgba8,
    scale: Scale,
    denoise: f32,
    gpu: Option<&Gpu>,
    tile: usize,
    progress: impl Fn(usize, usize) -> bool + Sync,
) -> Result<(Rgba8, Report)> {
    let (w, h) = (img.w, img.h);
    if w == 0 || h == 0 || img.px.len() != w * h * 4 {
        bail!("gambar kosong");
    }
    let (ow, oh) = out_size(w, h, scale);
    if ow.saturating_mul(oh) > MAX_OUT_PX {
        bail!(
            "Hasil {ow}×{oh} px terlalu besar (maks. {} Mpx). Gambar ini sudah cukup besar; Perbesar AI untuk gambar kecil.",
            MAX_OUT_PX / 1_000_000
        );
    }
    let tile = tile.max(8);
    let model = Model::new(denoise);
    let alpha = img.has_alpha && img.px.chunks_exact(4).any(|p| p[3] != 255);
    let rgb: Vec<f32> = img.px.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).map(|v| v as f32 / 255.0).collect();
    let grey: Vec<f32> = if alpha {
        img.px.chunks_exact(4).flat_map(|p| [p[3]; 3]).map(|v| v as f32 / 255.0).collect()
    } else {
        Vec::new()
    };
    let tiles: Vec<(usize, usize)> = (0..h).step_by(tile).flat_map(|y| (0..w).step_by(tile).map(move |x| (x, y))).collect();
    let total = tiles.len() * if alpha { 2 } else { 1 };
    let done = AtomicUsize::new(0);
    if !progress(0, total) {
        bail!("dibatalkan");
    }
    let k = scale.factor();
    let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let out = std::sync::Mutex::new(vec![255u8; ow * oh * 4]);
    // one tile: network, averaged down to the output factor, written into `out`
    // (channels 0..3 for colour, channel 3 for alpha)
    // network on one tile (+ margin): on the card when `card`, else on the processor
    let net = |src: &[f32], (x0, y0): (usize, usize), card: Option<&Gpu>| -> Result<Vec<f32>> {
        let (x1, y1) = ((x0 + tile).min(w), (y0 + tile).min(h));
        let (xa, ya, xb, yb) = (x0.saturating_sub(PAD), y0.saturating_sub(PAD), (x1 + PAD).min(w), (y1 + PAD).min(h));
        let (tw, th) = (xb - xa, yb - ya);
        let mut t = Vec::with_capacity(tw * th * 3);
        for y in ya..yb {
            t.extend_from_slice(&src[(y * w + xa) * 3..(y * w + xb) * 3]);
        }
        let keep = [x0 - xa, y0 - ya, x1 - xa, y1 - ya];
        Ok(match card {
            Some(g) => {
                let feat = g.features(&model, &t, tw, th)?;
                shuffle(&feat, |x, y| y * tw + x, &t, tw, keep)
            }
            None => model.forward(&t, tw, th, keep),
        })
    };
    let run = |(x0, y0): (usize, usize), alpha_pass: bool, r: Vec<f32>| -> bool {
        let (x1, y1) = ((x0 + tile).min(w), (y0 + tile).min(h));
        let f = NET_SCALE / k;
        let rw = (x1 - x0) * NET_SCALE;
        let (cw, ch) = ((x1 - x0) * k, (y1 - y0) * k);
        let mut rows = vec![0u8; cw * ch * 4];
        for y in 0..ch {
            for x in 0..cw {
                let mut v = [0.0f32; 3];
                for dy in 0..f {
                    for dx in 0..f {
                        let q = &r[((y * f + dy) * rw + x * f + dx) * 3..][..3];
                        for c in 0..3 {
                            v[c] += q[c].clamp(0.0, 1.0);
                        }
                    }
                }
                let n = (f * f) as f32;
                let d = &mut rows[(y * cw + x) * 4..][..4];
                if alpha_pass {
                    d[3] = to8((v[0] / n + v[1] / n + v[2] / n) / 3.0);
                } else {
                    for c in 0..3 {
                        d[c] = to8(v[c] / n);
                    }
                }
            }
        }
        drop(r);
        {
            let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
            for (yy, row) in rows.chunks_exact(cw * 4).enumerate() {
                let dst = &mut o[((y0 * k + yy) * ow + x0 * k) * 4..][..cw * 4];
                if alpha_pass {
                    for (a, b) in dst.chunks_exact_mut(4).zip(row.chunks_exact(4)) {
                        a[3] = b[3];
                    }
                } else {
                    for (a, b) in dst.chunks_exact_mut(4).zip(row.chunks_exact(4)) {
                        a[..3].copy_from_slice(&b[..3]);
                    }
                }
            }
        }
        let n = done.fetch_add(1, Ordering::AcqRel) + 1;
        progress(n, total)
    };
    // processor: tiles in parallel within ~600 MB of working memory (~560 bytes per
    // tile pixel incl. margin), at most 8 at once
    let side = tile.min(w.max(h)) + 2 * PAD;
    let per_tile = side * side * 560;
    let threads = rayon::current_num_threads().clamp(1, 8).min((600 << 20) / per_tile.max(1)).max(1);
    let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).build()?;
    let mut report = Report { gpu: gpu.map(|g| g.name().to_string()), ..Report::default() };
    let mut card = gpu;
    let passes: &[(&Vec<f32>, bool)] = if alpha { &[(&rgb, false), (&grey, true)] } else { &[(&rgb, false)] };
    for &(src, alpha_pass) in passes {
        // graphics card: one tile after the other (the card runs each tile in parallel)
        let mut next = 0;
        while let Some(g) = card {
            let Some(&t) = tiles.get(next) else { break };
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| net(src, t, Some(g)))) {
                Ok(Ok(r)) => {
                    report.gpu_tiles += 1;
                    next += 1;
                    if !run(t, alpha_pass, r) {
                        bail!("dibatalkan");
                    }
                }
                Ok(Err(e)) => {
                    report.fallback = Some(format!("{e:#}"));
                    card = None;
                }
                Err(_) => {
                    report.fallback = Some("kesalahan di driver kartu grafis".into());
                    card = None;
                }
            }
        }
        let rest = &tiles[next..];
        report.cpu_tiles += rest.len();
        let ok = pool.install(|| rest.par_iter().all(|&t| net(src, t, None).is_ok_and(|r| run(t, alpha_pass, r))));
        if !ok {
            bail!("dibatalkan");
        }
    }
    let px = out.into_inner().unwrap_or_else(|e| e.into_inner());
    Ok((Rgba8 { w: ow, h: oh, px, has_alpha: alpha }, report))
}
