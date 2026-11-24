//! Flat / coloured background removal with local foreground matting.
//! Port of `remove_bg` in studiozea_ref.py.

use rayon::prelude::*;

use crate::filters::{dilate_cross, label, max_filter};

pub struct BgResult {
    pub rgb: Vec<f32>,
    pub alpha: Vec<f32>,
    pub bg: [f32; 3],
}

fn median(mut v: Vec<f32>) -> f32 {
    v.sort_by(|a, b| a.total_cmp(b));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// user controls: colour tolerance of the background flood and whether enclosed
/// areas of exactly the background colour inside the design are removed too
#[derive(Clone, Copy, Debug)]
pub struct BgParams {
    pub tolerance: f32,
    pub pockets: bool,
}

impl Default for BgParams {
    fn default() -> Self {
        BgParams {
            tolerance: 0.30,
            pockets: true,
        }
    }
}

/// median colour of the image border (the background colour used by remove_bg)
pub fn border_colour(img: &[f32], w: usize, h: usize) -> [f32; 3] {
    let mut border: Vec<usize> = Vec::with_capacity(2 * (w + h));
    border.extend(0..w);
    border.extend((h - 1) * w..h * w);
    border.extend((0..h).map(|y| y * w));
    border.extend((0..h).map(|y| y * w + w - 1));
    [0, 1, 2].map(|c| median(border.iter().map(|&p| img[p * 3 + c]).collect()))
}

/// fraction of border pixels within `tol` (RGB distance) of the border median
pub fn border_flatness(img: &[f32], w: usize, h: usize, tol: f32) -> f32 {
    let bc = border_colour(img, w, h);
    let mut n = 0usize;
    let mut ok = 0usize;
    let mut look = |p: usize| {
        let d = (0..3).map(|c| (img[p * 3 + c] - bc[c]).powi(2)).sum::<f32>().sqrt();
        n += 1;
        ok += (d < tol) as usize;
    };
    for x in 0..w {
        look(x);
        look((h - 1) * w + x);
    }
    for y in 0..h {
        look(y * w);
        look(y * w + w - 1);
    }
    ok as f32 / n.max(1) as f32
}

/// the image has a flat background: at least half of the border has one colour
/// (gradient designs like a yellow sky fading to black at the bottom still count)
pub fn has_flat_border(img: &[f32], w: usize, h: usize) -> bool {
    w > 2 && h > 2 && border_flatness(img, w, h, 0.10) >= 0.5
}

/// colour of a flat background of an 8-bit image (the shirt colour that suits the design):
/// None for images with transparency or a busy border. Same test as `has_flat_border`.
pub fn flat_background(img: &crate::io::Rgba8) -> Option<[f32; 3]> {
    let (w, h) = (img.w, img.h);
    if w <= 2 || h <= 2 || (img.has_alpha && img.px.chunks_exact(4).any(|p| p[3] < 255)) {
        return None;
    }
    let mut border: Vec<usize> = Vec::with_capacity(2 * (w + h));
    border.extend(0..w);
    border.extend((h - 1) * w..h * w);
    border.extend((0..h).map(|y| y * w));
    border.extend((0..h).map(|y| y * w + w - 1));
    let px = |p: usize, c: usize| img.px[p * 4 + c] as f32 / 255.0;
    let bc = [0, 1, 2].map(|c| median(border.iter().map(|&p| px(p, c)).collect()));
    let flat = border
        .iter()
        .filter(|&&p| (0..3).map(|c| (px(p, c) - bc[c]).powi(2)).sum::<f32>().sqrt() < 0.10)
        .count();
    (flat * 2 >= border.len()).then_some(bc)
}

/// near-white / near-black background (what a white / black shirt removes by itself)
pub fn is_white(c: [f32; 3]) -> bool {
    c.iter().all(|&v| v > 0.9)
}
pub fn is_black(c: [f32; 3]) -> bool {
    c.iter().all(|&v| v < 0.1)
}

impl BgParams {
    /// automatic choice: white or black backgrounds often have the same colour inside
    /// the design (highlights, linework) -> tight tolerance, keep enclosed areas;
    /// other flat colours -> standard tolerance, remove enclosed areas (letter holes)
    pub fn auto_for(bg: [f32; 3]) -> BgParams {
        let extreme = is_white(bg) || is_black(bg);
        if extreme {
            BgParams {
                tolerance: 0.06,
                pockets: false,
            }
        } else {
            BgParams::default()
        }
    }
}

pub fn remove_bg(img: &[f32], w: usize, h: usize) -> BgResult {
    remove_bg_with(img, w, h, BgParams::default())
}

pub fn remove_bg_with(img: &[f32], w: usize, h: usize, prm: BgParams) -> BgResult {
    let (t_core, t1, band, win, min_pocket) = (0.04f32, prm.tolerance, 3usize, 7usize, 4u32);
    let n = w * h;
    // border pixels (top row, bottom row, left column, right column)
    let mut border: Vec<usize> = Vec::with_capacity(2 * (w + h));
    border.extend(0..w);
    border.extend((h - 1) * w..h * w);
    border.extend((0..h).map(|y| y * w));
    border.extend((0..h).map(|y| y * w + w - 1));
    let bc = [0, 1, 2].map(|c| median(border.iter().map(|&p| img[p * 3 + c]).collect()));
    let d: Vec<f32> = (0..n)
        .map(|p| {
            let x = img[p * 3] - bc[0];
            let y = img[p * 3 + 1] - bc[1];
            let z = img[p * 3 + 2] - bc[2];
            (x * x + y * y + z * z).sqrt()
        })
        .collect();
    let cand: Vec<bool> = d.iter().map(|&v| v < t1).collect();
    let (lb, _) = label(&cand, w, h);
    let mut on_border = vec![false; lb.iter().copied().max().unwrap_or(0) as usize + 1];
    for &p in &border {
        on_border[lb[p] as usize] = true;
    }
    on_border[0] = false;
    let mut zone: Vec<bool> = lb.iter().map(|&l| on_border[l as usize]).collect();
    if prm.pockets {
        let core: Vec<bool> = d.iter().map(|&v| v < t_core).collect();
        let (lc, sc) = label(&core, w, h);
        for p in 0..n {
            if lc[p] != 0 && sc[lc[p] as usize] >= min_pocket {
                zone[p] = true;
            }
        }
    }
    let zone = dilate_cross(&zone, w, h, band);
    // most foreground-like neighbour: max of (round(d*4096) << 32) + index (index fits 32 bits)
    let key: Vec<i64> = (0..n)
        .map(|p| (d[p] * 4096.0).round_ties_even() as i64 * (1i64 << 32) + p as i64)
        .collect();
    let best = max_filter(&key, w, h, win);
    let mut alpha = vec![0f32; n];
    let mut rgb = vec![0f32; n * 3];
    alpha
        .par_iter_mut()
        .zip(rgb.par_chunks_mut(3))
        .enumerate()
        .for_each(|(p, (al, px))| {
            let q = (best[p] % (1i64 << 32)) as usize;
            let fb = [
                img[q * 3] - bc[0],
                img[q * 3 + 1] - bc[1],
                img[q * 3 + 2] - bc[2],
            ];
            let pb = [
                img[p * 3] - bc[0],
                img[p * 3 + 1] - bc[1],
                img[p * 3 + 2] - bc[2],
            ];
            let num = pb[0] * fb[0] + pb[1] * fb[1] + pb[2] * fb[2];
            let den = (fb[0] * fb[0] + fb[1] * fb[1] + fb[2] * fb[2]).max(1e-6);
            let mut a = (num / den).clamp(0.0, 1.0);
            // minimum alpha that keeps the un-mixed colour inside [0,1] (colour-to-alpha):
            // a pixel that is not a mix of bg and F (black outline on navy) stays opaque
            let mut am = f32::NEG_INFINITY;
            for c in 0..3 {
                let up = (img[p * 3 + c] - bc[c]) / (1.0 - bc[c]).max(1e-6);
                let dn = (bc[c] - img[p * 3 + c]) / bc[c].max(1e-6);
                am = am.max(up.max(dn));
            }
            a = a.max(am.clamp(0.0, 1.0));
            if d[p] < t_core {
                a = 0.0;
            }
            if !zone[p] {
                a = 1.0;
            }
            if a < 0.04 {
                a = 0.0;
            }
            *al = a;
            for c in 0..3 {
                let v = (img[p * 3 + c] - (1.0 - a) * bc[c]) / a.max(1e-3);
                px[c] = v.clamp(0.0, 1.0);
            }
        });
    BgResult { rgb, alpha, bg: bc }
}
