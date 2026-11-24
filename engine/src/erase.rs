//! Eraser / restore brush. Port of `apply_strokes` in studiozea_ref.py.
//!
//! A stroke is a polyline in source pixels with a radius; it covers every pixel whose
//! centre lies within `r` of the polyline (round caps, hard edge: DTF prints no
//! semi-transparency anyway). Strokes apply in order after background removal and
//! before crop / trim, so they stay on the same spot of the design whatever the crop,
//! trim or print size.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BrushMode {
    /// no ink under the brush
    Hapus,
    /// the original pixels (before background removal) come back
    Kembalikan,
    /// "Lindungi teks": mark as lettering (printed solid); the image is not changed
    Teks,
    /// "Lindungi teks": not lettering (screened as usual)
    BukanTeks,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub mode: BrushMode,
    /// radius in source pixels
    pub r: f64,
    /// polyline in source pixels (x, y); one point = a dot
    pub pts: Vec<[f64; 2]>,
}

/// largest brush radius (source pixels) and total number of points accepted
pub const MAX_R: f64 = 5000.0;
pub const MAX_POINTS: usize = 200_000;

pub fn validate(strokes: &[Stroke]) -> Result<(), String> {
    let mut n = 0;
    for s in strokes {
        if !(s.r.is_finite() && (0.5..=MAX_R).contains(&s.r)) {
            return Err(format!("ukuran kuas harus antara 1 dan {} px", 2.0 * MAX_R));
        }
        if s.pts.is_empty() || s.pts.iter().any(|p| !(p[0].is_finite() && p[1].is_finite())) {
            return Err("coretan tidak valid".into());
        }
        n += s.pts.len();
    }
    if n > MAX_POINTS {
        return Err(format!("terlalu banyak coretan ({n} titik, maksimal {MAX_POINTS})"));
    }
    Ok(())
}

/// call `f` for every pixel (index) under one stroke
fn cover(s: &Stroke, w: usize, h: usize, mut f: impl FnMut(usize)) {
    let segs: Vec<([f64; 2], [f64; 2])> = if s.pts.len() == 1 {
        vec![(s.pts[0], s.pts[0])]
    } else {
        s.pts.windows(2).map(|p| (p[0], p[1])).collect()
    };
    let r2 = s.r * s.r;
    for (a, b) in segs {
        let (ax, ay, bx, by) = (a[0], a[1], b[0], b[1]);
        let x0 = (ax.min(bx) - s.r).floor().max(0.0);
        let y0 = (ay.min(by) - s.r).floor().max(0.0);
        let x1 = ((ax.max(bx) + s.r).ceil() + 1.0).min(w as f64);
        let y1 = ((ay.max(by) + s.r).ceil() + 1.0).min(h as f64);
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let (dx, dy) = (bx - ax, by - ay);
        let l2 = dx * dx + dy * dy;
        for y in y0 as usize..y1 as usize {
            let py = y as f64 + 0.5;
            for x in x0 as usize..x1 as usize {
                let px = x as f64 + 0.5;
                let t = if l2 > 0.0 {
                    (((px - ax) * dx + (py - ay) * dy) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let qx = ax + t * dx - px;
                let qy = ay + t * dy - py;
                if qx * qx + qy * qy <= r2 {
                    f(y * w + x);
                }
            }
        }
    }
}

/// apply the strokes to `rgb`/`alpha` (w x h); `orig_*` = the image before background
/// removal (used by Kembalikan). Text strokes are skipped here (see `apply_text`).
pub fn apply(
    rgb: &mut [f32],
    alpha: &mut [f32],
    orig_rgb: &[f32],
    orig_alpha: &[f32],
    w: usize,
    h: usize,
    strokes: &[Stroke],
) {
    for s in strokes {
        match s.mode {
            BrushMode::Hapus => cover(s, w, h, |q| alpha[q] = 0.0),
            BrushMode::Kembalikan => cover(s, w, h, |q| {
                alpha[q] = orig_alpha[q];
                rgb[q * 3..q * 3 + 3].copy_from_slice(&orig_rgb[q * 3..q * 3 + 3]);
            }),
            BrushMode::Teks | BrushMode::BukanTeks => {}
        }
    }
}

/// apply the text strokes (Teks = 1, BukanTeks = 0) to the "Lindungi teks" mask
pub fn apply_text(mask: &mut [f32], w: usize, h: usize, strokes: &[Stroke]) {
    for s in strokes {
        match s.mode {
            BrushMode::Teks => cover(s, w, h, |q| mask[q] = 1.0),
            BrushMode::BukanTeks => cover(s, w, h, |q| mask[q] = 0.0),
            BrushMode::Hapus | BrushMode::Kembalikan => {}
        }
    }
}
