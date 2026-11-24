//! Lanczos-3 resize identical to Pillow's float ('F') path, premultiplied alpha,
//! and auto-trim. Port of `premul_resize` / `autotrim` in studiozea_ref.py.

use rayon::prelude::*;

use crate::render::Shirt;

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    let x = x * std::f64::consts::PI;
    x.sin() / x
}

fn lanczos(x: f64) -> f64 {
    if (-3.0..3.0).contains(&x) {
        sinc(x) * sinc(x / 3.0)
    } else {
        0.0
    }
}

/// resampling filter for scaling to the print size (Pillow's float path)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Resample {
    /// Lanczos-3: sharpest
    #[default]
    Lanczos,
    /// bicubic (a = -0.5, like Photoshop's bicubic): softer
    Bicubic,
}

impl Resample {
    fn support(self) -> f64 {
        match self {
            Resample::Lanczos => 3.0,
            Resample::Bicubic => 2.0,
        }
    }

    /// Pillow lanczos_filter / bicubic_filter
    fn weight(self, x: f64) -> f64 {
        match self {
            Resample::Lanczos => lanczos(x),
            Resample::Bicubic => {
                const A: f64 = -0.5;
                let x = x.abs();
                if x < 1.0 {
                    ((A + 2.0) * x - (A + 3.0)) * x * x + 1.0
                } else if x < 2.0 {
                    (((x - 5.0) * x + 8.0) * x - 4.0) * A
                } else {
                    0.0
                }
            }
        }
    }
}

pub(crate) struct Coeffs {
    pub ksize: usize,
    pub bounds: Vec<(usize, usize)>,
    pub k: Vec<f64>,
}

/// Pillow precompute_coeffs (in0 = 0, in1 = in_size)
pub(crate) fn precompute(in_size: usize, out_size: usize, f: Resample) -> Coeffs {
    let scale = in_size as f32 as f64 / out_size as f64;
    let filterscale = if scale < 1.0 { 1.0 } else { scale };
    let support = f.support() * filterscale;
    let ksize = support.ceil() as usize * 2 + 1;
    let inv = 1.0 / filterscale;
    let mut bounds = Vec::with_capacity(out_size);
    let mut k = vec![0f64; out_size * ksize];
    for xx in 0..out_size {
        let center = 0.0 + (xx as f64 + 0.5) * scale;
        let mut xmin = (center - support + 0.5) as i64;
        if xmin < 0 {
            xmin = 0;
        }
        let mut xmax = (center + support + 0.5) as i64;
        if xmax > in_size as i64 {
            xmax = in_size as i64;
        }
        xmax -= xmin;
        let kk = &mut k[xx * ksize..(xx + 1) * ksize];
        let mut ww = 0.0f64;
        for x in 0..xmax as usize {
            let wv = f.weight(((x as i64 + xmin) as f64 - center + 0.5) * inv);
            kk[x] = wv;
            ww += wv;
        }
        if ww != 0.0 {
            for v in kk.iter_mut().take(xmax as usize) {
                *v /= ww;
            }
        }
        bounds.push((xmin as usize, xmax as usize));
    }
    Coeffs { ksize, bounds, k }
}

/// Pillow Image.resize((ow, oh), LANCZOS | BICUBIC) for one float plane
pub fn resize_plane(src: &[f32], w: usize, h: usize, ow: usize, oh: usize, f: Resample) -> Vec<f32> {
    let mut cur = src.to_vec();
    let mut cw = w;
    if ow != w {
        let co = precompute(w, ow, f);
        let mut out = vec![0f32; ow * h];
        out.par_chunks_mut(ow).enumerate().for_each(|(y, row)| {
            let line = &cur[y * w..(y + 1) * w];
            for (xx, o) in row.iter_mut().enumerate() {
                let (xmin, xmax) = co.bounds[xx];
                let k = &co.k[xx * co.ksize..];
                let mut ss = 0.0f64;
                for x in 0..xmax {
                    ss += line[x + xmin] as f64 * k[x];
                }
                *o = ss as f32;
            }
        });
        cur = out;
        cw = ow;
    }
    if oh != h {
        let co = precompute(h, oh, f);
        let mut out = vec![0f32; cw * oh];
        out.par_chunks_mut(cw).enumerate().for_each(|(yy, row)| {
            let (ymin, ymax) = co.bounds[yy];
            let k = &co.k[yy * co.ksize..];
            for (xx, o) in row.iter_mut().enumerate() {
                let mut ss = 0.0f64;
                for y in 0..ymax {
                    ss += cur[(y + ymin) * cw + xx] as f64 * k[y];
                }
                *o = ss as f32;
            }
        });
        cur = out;
    }
    cur
}

/// Lanczos resize with premultiplied colour (no halo on soft edges)
pub fn premul_resize(rgb: &[f32], alpha: &[f32], w: usize, h: usize, ow: usize, oh: usize) -> (Vec<f32>, Vec<f32>) {
    premul_resize_with(rgb, alpha, w, h, ow, oh, Resample::Lanczos)
}

/// `premul_resize` with a chosen filter
pub fn premul_resize_with(
    rgb: &[f32],
    alpha: &[f32],
    w: usize,
    h: usize,
    ow: usize,
    oh: usize,
    f: Resample,
) -> (Vec<f32>, Vec<f32>) {
    let n = w * h;
    let planes: Vec<Vec<f32>> = (0..4)
        .map(|c| {
            if c == 3 {
                alpha.to_vec()
            } else {
                (0..n).map(|p| rgb[p * 3 + c] * alpha[p]).collect()
            }
        })
        .collect();
    let rs: Vec<Vec<f32>> = planes
        .par_iter()
        .map(|pl| resize_plane(pl, w, h, ow, oh, f))
        .collect();
    let on = ow * oh;
    let a: Vec<f32> = rs[3].iter().map(|&v| v.clamp(0.0, 1.0)).collect();
    let mut c = vec![0f32; on * 3];
    for p in 0..on {
        let d = a[p].max(1e-4);
        for ch in 0..3 {
            c[p * 3 + ch] = (rs[ch][p] / d).clamp(0.0, 1.0);
        }
    }
    (c, a)
}

/// bounding box (x0, y0, x1, y1) of pixels that would print
pub fn autotrim(
    c: &[f32],
    a: &[f32],
    w: usize,
    h: usize,
    shirt: Shirt,
) -> Option<(usize, usize, usize, usize)> {
    let mut b: Option<(usize, usize, usize, usize)> = None;
    for y in 0..h {
        for x in 0..w {
            let p = y * w + x;
            let on = match shirt {
                Shirt::Hitam => (0..3).map(|i| c[p * 3 + i] * a[p]).fold(f32::MIN, f32::max) > 0.06,
                Shirt::Putih => {
                    (0..3)
                        .map(|i| (1.0 - c[p * 3 + i]) * a[p])
                        .fold(f32::MIN, f32::max)
                        > 0.06
                }
                Shirt::Warna(_) => a[p] > 0.06,
            };
            if on {
                b = Some(match b {
                    None => (x, y, x + 1, y + 1),
                    Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1)),
                });
            }
        }
    }
    b
}

/// turn clockwise by `rotate` degrees (0, 90, 180, 270), then mirror left-right (if
/// `mirror`, as seen after turning); returns the new colour, alpha, width, height.
/// Port of `orient`.
/// `orient` for one plane (same pixel mapping)
pub fn orient_plane(a: &[f32], w: usize, h: usize, rotate: u16, mirror: bool) -> Vec<f32> {
    let turn = (rotate / 90) % 4;
    let (ow, oh) = if turn % 2 == 1 { (h, w) } else { (w, h) };
    let mut ra = vec![0f32; ow * oh];
    for r in 0..oh {
        for col in 0..ow {
            let c2 = if mirror { ow - 1 - col } else { col };
            let (sy, sx) = match turn {
                1 => (h - 1 - c2, r),
                2 => (h - 1 - r, w - 1 - c2),
                3 => (c2, w - 1 - r),
                _ => (r, c2),
            };
            ra[r * ow + col] = a[sy * w + sx];
        }
    }
    ra
}

pub fn orient(c: &[f32], a: &[f32], w: usize, h: usize, rotate: u16, mirror: bool) -> (Vec<f32>, Vec<f32>, usize, usize) {
    let turn = (rotate / 90) % 4;
    let (ow, oh) = if turn % 2 == 1 { (h, w) } else { (w, h) };
    let mut rc = vec![0f32; ow * oh * 3];
    let mut ra = vec![0f32; ow * oh];
    for r in 0..oh {
        for col in 0..ow {
            let c2 = if mirror { ow - 1 - col } else { col };
            // source pixel of the turned image (numpy rot90 with k = -turn)
            let (sy, sx) = match turn {
                1 => (h - 1 - c2, r),
                2 => (h - 1 - r, w - 1 - c2),
                3 => (c2, w - 1 - r),
                _ => (r, c2),
            };
            let (q, d) = (sy * w + sx, r * ow + col);
            ra[d] = a[q];
            rc[d * 3..d * 3 + 3].copy_from_slice(&c[q * 3..q * 3 + 3]);
        }
    }
    (rc, ra, ow, oh)
}
