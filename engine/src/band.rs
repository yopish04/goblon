//! Light-weight rendering: the print is rendered in windows (horizontal bands or any
//! rectangle) with a halo, from a prepared image kept at SOURCE resolution and resized
//! lazily. Every step of `render` is either per pixel (with absolute coordinates for
//! the threshold tiles / noise / FM blocks, see `render_at`) or looks at a bounded
//! neighbourhood, so a window with a halo larger than that neighbourhood gives exactly
//! the pixels of the full render (checked by tests/band.rs on real designs).
//!
//! Memory at 600 dpi, 30 x 40 cm (67 Mpx): packed result 34 MB + one band (~300 MB)
//! instead of ~4.4 GB for the full-image render.

use std::sync::Arc;

use rayon::prelude::*;

use crate::distress;
use crate::dtf::{mindot_px, minarea_px};
use crate::filters::label;
use crate::io::Checks;
use crate::render::{render_at_masked, Pattern, Settings};
use crate::resize::{precompute, Coeffs};
use crate::Prepared;

/// Exact lazy version of `resize::premul_resize`: any window of the resized image,
/// computed with the same per-pixel arithmetic (Pillow float Lanczos, premultiplied).
pub struct Resizer {
    pub w: usize,
    pub h: usize,
    pub ow: usize,
    pub oh: usize,
    /// source image; the premultiplied planes r*a, g*a, b*a, a are computed on the fly
    /// (same f32 product as premul_resize, so no extra 16 bytes per source pixel)
    src: Arc<Prepared>,
    cx: Option<Coeffs>,
    cy: Option<Coeffs>,
}

impl Resizer {
    pub fn new(rgb: &[f32], alpha: &[f32], w: usize, h: usize, ow: usize, oh: usize) -> Resizer {
        let src = Prepared {
            w,
            h,
            pre_w: w,
            src_box: [0, 0, w, h],
            rgb: rgb.to_vec(),
            alpha: alpha.to_vec(),
            bg: None,
            bg_used: None,
            solid: None,
        };
        Resizer::from_prepared(Arc::new(src), ow, oh, crate::resize::Resample::Lanczos)
    }

    pub fn from_prepared(src: Arc<Prepared>, ow: usize, oh: usize, f: crate::resize::Resample) -> Resizer {
        let (w, h) = (src.w, src.h);
        Resizer {
            w,
            h,
            ow,
            oh,
            src,
            cx: (ow != w).then(|| precompute(w, ow, f)),
            cy: (oh != h).then(|| precompute(h, oh, f)),
        }
    }

    /// premultiplied plane value (c = 0..2 colour * alpha, 3 = alpha, 4 = the solid mask,
    /// not premultiplied) of source pixel p
    #[inline]
    fn pm(&self, c: usize, p: usize) -> f32 {
        match c {
            3 => self.src.alpha[p],
            4 => self.src.solid.as_ref().map_or(0.0, |s| s[p]),
            _ => self.src.rgb[p * 3 + c] * self.src.alpha[p],
        }
    }

    /// the resized "Lindungi teks" mask of a window (None when the image has none)
    pub fn solid(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> Option<Vec<f32>> {
        self.src.solid.as_ref()?;
        Some(self.plane(4, x0, y0, x1, y1))
    }

    /// one resized plane (see `pm`) of the window
    fn plane(&self, c: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> Vec<f32> {
        let (ww, wh) = (x1 - x0, y1 - y0);
        // source rows needed for the vertical pass
        let (sy0, sy1) = match &self.cy {
            Some(cy) => {
                let lo = (y0..y1).map(|yy| cy.bounds[yy].0).min().unwrap_or(0);
                let hi = (y0..y1)
                    .map(|yy| cy.bounds[yy].0 + cy.bounds[yy].1)
                    .max()
                    .unwrap_or(0);
                (lo, hi.max(lo))
            }
            None => (y0, y1),
        };
        let rows = sy1 - sy0;
        // horizontal pass on the needed source rows, only the window columns
        let mut hp = vec![0f32; rows * ww];
        hp.par_chunks_mut(ww.max(1)).enumerate().for_each(|(r, out)| {
            let row0 = (sy0 + r) * self.w;
            match &self.cx {
                Some(cx) => {
                    for (i, o) in out.iter_mut().enumerate() {
                        let xx = x0 + i;
                        let (xmin, xmax) = cx.bounds[xx];
                        let k = &cx.k[xx * cx.ksize..];
                        let mut ss = 0.0f64;
                        for x in 0..xmax {
                            ss += self.pm(c, row0 + x + xmin) as f64 * k[x];
                        }
                        *o = ss as f32;
                    }
                }
                None => {
                    for (i, o) in out.iter_mut().enumerate() {
                        *o = self.pm(c, row0 + x0 + i);
                    }
                }
            }
        });
        match &self.cy {
            Some(cy) => {
                let mut out = vec![0f32; ww * wh];
                out.par_chunks_mut(ww.max(1)).enumerate().for_each(|(r, row)| {
                    let yy = y0 + r;
                    let (ymin, ymax) = cy.bounds[yy];
                    let k = &cy.k[yy * cy.ksize..];
                    for (i, o) in row.iter_mut().enumerate() {
                        let mut ss = 0.0f64;
                        for y in 0..ymax {
                            ss += hp[(y + ymin - sy0) * ww + i] as f64 * k[y];
                        }
                        *o = ss as f32;
                    }
                });
                out
            }
            None => hp,
        }
    }

    /// resized window [x0, x1) x [y0, y1): interleaved rgb + alpha (like premul_resize)
    pub fn window(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> (Vec<f32>, Vec<f32>) {
        let (ww, wh) = (x1 - x0, y1 - y0);
        let planes: Vec<Vec<f32>> = (0..4usize).into_par_iter().map(|c| self.plane(c, x0, y0, x1, y1)).collect();
        let n = ww * wh;
        let a: Vec<f32> = planes[3].iter().map(|&v| v.clamp(0.0, 1.0)).collect();
        let mut c = vec![0f32; n * 3];
        c.par_chunks_mut(3).enumerate().for_each(|(p, px)| {
            let d = a[p].max(1e-4);
            for ch in 0..3 {
                px[ch] = (planes[ch][p] / d).clamp(0.0, 1.0);
            }
        });
        (c, a)
    }
}

/// Everything needed to render any window of one print.
pub struct Plan {
    pub w: usize,
    pub h: usize,
    pub settings: Settings,
    /// halo in print pixels (multiple of the FM block size)
    pub halo: usize,
    k: usize,
    src: Arc<Prepared>,
    resizer: Option<Resizer>,
}

/// halo that covers every neighbourhood `render` looks at (see module docs):
/// hybrid / K tone filter (dpi/lpi), FM blocks, hard-edge ring + EDT, the texture
/// hole mask close/open and dot-aware vote (`distress::halo_extra`), then the
/// island / hole clean-up (min area) on top of the screen.
pub fn halo_px(s: &Settings) -> usize {
    let k = mindot_px(s.dpi);
    let fsize = ((s.dpi / s.lpi) as usize).max(3);
    let edge = 2 + (2.2f64.max(0.15 * s.dpi / 25.4 + 0.4)).ceil() as usize + 4;
    let screen = fsize.max(edge).max(k) + k;
    let amin = minarea_px(s.dpi) as usize;
    // error diffusion: the dots of a tile depend on the whole tile + warm-up
    let ed = if s.pattern == Pattern::Difusi { crate::dtf::ed_halo(s.dpi) } else { 0 };
    let h = 2 * screen + 2 * amin + 16 + distress::halo_extra(s) + ed;
    h.div_ceil(k) * k
}

impl Plan {
    /// `src` = prepared image at source resolution (after background removal, crop,
    /// trim), `ow` x `oh` = print size in pixels.
    pub fn new(src: Prepared, ow: usize, oh: usize, settings: Settings) -> Plan {
        Plan::from_arc(Arc::new(src), ow, oh, settings)
    }

    /// like `new`, sharing the prepared image (the app keeps it cached)
    pub fn from_arc(src: Arc<Prepared>, ow: usize, oh: usize, settings: Settings) -> Plan {
        // textures cover and centre on the whole print
        let settings = if distress::needs_print_size(&settings) {
            distress::with_print_size(&settings, ow, oh)
        } else {
            settings
        };
        let resizer = (ow != src.w || oh != src.h)
            .then(|| Resizer::from_prepared(src.clone(), ow, oh, settings.resample));
        let k = mindot_px(settings.dpi);
        let halo = halo_px(&settings);
        Plan {
            w: ow,
            h: oh,
            halo,
            k,
            settings,
            src,
            resizer,
        }
    }

    pub fn source(&self) -> &Prepared {
        &self.src
    }

    /// prepared (resized, continuous tone) window at print resolution
    pub fn prepared(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> (Vec<f32>, Vec<f32>) {
        match &self.resizer {
            Some(r) => r.window(x0, y0, x1, y1),
            None => {
                let ww = x1 - x0;
                let mut c = Vec::with_capacity(ww * (y1 - y0) * 3);
                let mut a = Vec::with_capacity(ww * (y1 - y0));
                for y in y0..y1 {
                    let q = y * self.src.w;
                    c.extend_from_slice(&self.src.rgb[(q + x0) * 3..(q + x1) * 3]);
                    a.extend_from_slice(&self.src.alpha[q + x0..q + x1]);
                }
                (c, a)
            }
        }
    }

    /// "Lindungi teks" mask of a window at print resolution (None = no text kept solid)
    pub fn solid(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> Option<Vec<f32>> {
        let s = self.src.solid.as_ref().filter(|_| self.settings.protect_text)?;
        Some(match &self.resizer {
            Some(r) => r.solid(x0, y0, x1, y1)?,
            None => (y0..y1).flat_map(|y| s[y * self.src.w + x0..y * self.src.w + x1].iter().copied()).collect(),
        })
    }

    /// exact RGBA8 pixels of the print window [x0, x1) x [y0, y1)
    pub fn render(&self, x0: usize, y0: usize, x1: usize, y1: usize) -> Vec<u8> {
        let (x1, y1) = (x1.min(self.w), y1.min(self.h));
        if x1 <= x0 || y1 <= y0 {
            return Vec::new();
        }
        // halo window, origin aligned to the FM block grid
        let wx0 = (x0.saturating_sub(self.halo) / self.k) * self.k;
        let wy0 = (y0.saturating_sub(self.halo) / self.k) * self.k;
        let wx1 = (x1 + self.halo).min(self.w);
        let wy1 = (y1 + self.halo).min(self.h);
        let (c, a) = self.prepared(wx0, wy0, wx1, wy1);
        let solid = self.solid(wx0, wy0, wx1, wy1);
        let ww = wx1 - wx0;
        let full = render_at_masked(&c, &a, solid.as_deref(), ww, wy1 - wy0, wx0, wy0, &self.settings);
        let rw = x1 - x0;
        let mut out = Vec::with_capacity(rw * (y1 - y0) * 4);
        for y in y0..y1 {
            let q = ((y - wy0) * ww + (x0 - wx0)) * 4;
            out.extend_from_slice(&full[q..q + rw * 4]);
        }
        out
    }

    /// band height (core rows) that keeps one band around `target_px` pixels
    pub fn band_rows(&self, target_px: usize) -> usize {
        let r = (target_px / self.w.max(1)).max(64);
        r.div_ceil(self.k) * self.k
    }

    /// description of the screen for the UI (pattern name)
    pub fn is_am(&self) -> bool {
        matches!(self.settings.pattern, Pattern::Am | Pattern::Interlock)
    }
}

/// per-pixel checks (semi-transparent, impure colour, hidden colour) of RGBA rows;
/// `tiny` is counted on the whole print by `tiny_islands`
pub fn pixel_checks(o: &[u8]) -> Checks {
    pixel_checks_for(o, true)
}

/// `pure` = false for true-colour output (Settings::pure_colours): colours not counted
pub fn pixel_checks_for(o: &[u8], pure: bool) -> Checks {
    let mut c = Checks {
        semi: 0,
        impure: 0,
        tiny: 0,
        hidden: 0,
    };
    for px in o.chunks_exact(4) {
        let a = px[3];
        if a != 0 && a != 255 {
            c.semi += 1;
        }
        if pure && a == 255 && px[..3].iter().any(|&v| v != 0 && v != 255) {
            c.impure += 1;
        }
        if a != 255 {
            c.hidden += px[..3].iter().filter(|&&v| v != 0).count();
        }
    }
    c
}

/// The print as 4 bits per pixel (R, G, B, A each 0 or 255): 1/8 of RGBA8. True-colour
/// output (halftone only on fades) is kept as RGBA8 instead (`full`).
pub struct Packed {
    pub w: usize,
    pub h: usize,
    data: Vec<u8>,
    full: bool,
}

impl Packed {
    pub fn new(w: usize, h: usize) -> Packed {
        Packed {
            w,
            h,
            data: vec![0u8; (w * h).div_ceil(2)],
            full: false,
        }
    }

    /// RGBA8 storage (any colours; alpha still 0 or 255)
    pub fn new_full(w: usize, h: usize) -> Packed {
        Packed {
            w,
            h,
            data: vec![0u8; w * h * 4],
            full: true,
        }
    }

    /// the storage the output of these settings needs
    pub fn for_settings(w: usize, h: usize, s: &crate::Settings) -> Packed {
        if s.pure_colours() {
            Packed::new(w, h)
        } else {
            Packed::new_full(w, h)
        }
    }

    /// 4-bit value (bit 0 R, 1 G, 2 B, 3 A) of pixel p (4-bit storage only)
    #[inline]
    pub fn get(&self, p: usize) -> u8 {
        let b = self.data[p >> 1];
        if p & 1 == 0 {
            b & 15
        } else {
            b >> 4
        }
    }

    #[inline]
    fn opaque(&self, p: usize) -> bool {
        if self.full {
            self.data[p * 4 + 3] != 0
        } else {
            self.get(p) & 8 != 0
        }
    }

    /// RGB of pixel p (0..255)
    #[inline]
    fn rgb(&self, p: usize) -> [u32; 3] {
        if self.full {
            let d = &self.data[p * 4..p * 4 + 3];
            [d[0] as u32, d[1] as u32, d[2] as u32]
        } else {
            let v = self.get(p) as u32;
            [(v & 1) * 255, ((v >> 1) & 1) * 255, ((v >> 2) & 1) * 255]
        }
    }

    /// store RGBA8 rows starting at row y0 (values must be 0 or 255 per channel, unless full)
    pub fn put_rows(&mut self, y0: usize, rgba: &[u8]) {
        if self.full {
            let p0 = y0 * self.w * 4;
            self.data[p0..p0 + rgba.len()].copy_from_slice(rgba);
            return;
        }
        let p0 = y0 * self.w;
        for (i, px) in rgba.chunks_exact(4).enumerate() {
            let p = p0 + i;
            let v = (px[0] >> 7) | ((px[1] >> 7) << 1) | ((px[2] >> 7) << 2) | ((px[3] >> 7) << 3);
            let b = &mut self.data[p >> 1];
            *b = if p & 1 == 0 { (*b & 0xF0) | v } else { (*b & 0x0F) | (v << 4) };
        }
    }

    /// RGBA8 of rows [y0, y1)
    pub fn rows(&self, y0: usize, y1: usize) -> Vec<u8> {
        if self.full {
            return self.data[y0 * self.w * 4..y1 * self.w * 4].to_vec();
        }
        let mut out = vec![0u8; (y1 - y0) * self.w * 4];
        out.par_chunks_mut(self.w * 4).enumerate().for_each(|(r, row)| {
            let p0 = (y0 + r) * self.w;
            for x in 0..self.w {
                let v = self.get(p0 + x);
                for c in 0..4 {
                    row[x * 4 + c] = if v >> c & 1 == 1 { 255 } else { 0 };
                }
            }
        });
        out
    }

    /// box average of 2^level x 2^level print pixels -> straight-alpha RGBA8 image of
    /// the print area [x0, x0 + tw * 2^level) x [y0, ...), clipped to the print
    pub fn box_rgba(&self, x0: usize, y0: usize, tw: usize, th: usize, level: u32) -> Vec<u8> {
        let f = 1usize << level;
        let mut out = vec![0u8; tw * th * 4];
        out.par_chunks_mut(tw * 4).enumerate().for_each(|(ty, row)| {
            for tx in 0..tw {
                let (mut n, mut sa, mut sr, mut sg, mut sb) = (0u32, 0u32, 0u32, 0u32, 0u32);
                for dy in 0..f {
                    let y = y0 + ty * f + dy;
                    if y >= self.h {
                        break;
                    }
                    for dx in 0..f {
                        let x = x0 + tx * f + dx;
                        if x >= self.w {
                            break;
                        }
                        let p = y * self.w + x;
                        n += 1;
                        if self.opaque(p) {
                            let [r, g, b] = self.rgb(p);
                            sa += 1;
                            sr += r;
                            sg += g;
                            sb += b;
                        }
                    }
                }
                let o = &mut row[tx * 4..tx * 4 + 4];
                if n > 0 && sa > 0 {
                    o[0] = ((sr + sa / 2) / sa) as u8;
                    o[1] = ((sg + sa / 2) / sa) as u8;
                    o[2] = ((sb + sa / 2) / sa) as u8;
                    o[3] = ((sa * 255 + n / 2) / n) as u8;
                }
            }
        });
        out
    }

    /// number of opaque 4-connected islands smaller than the minimum area, counted on
    /// the whole print band by band (exact: an island is counted in the band holding
    /// its first pixel; islands reaching beyond the margin are larger than min area)
    pub fn tiny_islands(&self, dpi: f64) -> usize {
        let amin = minarea_px(dpi);
        let m = amin as usize + 1;
        let band = 1024usize;
        let starts: Vec<usize> = (0..self.h).step_by(band).collect();
        starts
            .par_iter()
            .map(|&y0| {
                let y1 = (y0 + band).min(self.h);
                let e0 = y0.saturating_sub(m);
                let e1 = (y1 + m).min(self.h);
                let w = self.w;
                let mask: Vec<bool> = (e0 * w..e1 * w).map(|p| self.opaque(p)).collect();
                let (lab, sizes) = label(&mask, w, e1 - e0);
                let nl = sizes.len();
                let mut first = vec![usize::MAX; nl];
                let mut touches = vec![false; nl];
                for (i, &l) in lab.iter().enumerate() {
                    if l == 0 {
                        continue;
                    }
                    let l = l as usize;
                    if first[l] == usize::MAX {
                        first[l] = i;
                    }
                    let y = e0 + i / w;
                    if (y == e0 && e0 > 0) || (y + 1 == e1 && e1 < self.h) {
                        touches[l] = true;
                    }
                }
                (1..nl)
                    .filter(|&l| {
                        let fy = e0 + first[l] / w;
                        sizes[l] < amin && !touches[l] && fy >= y0 && fy < y1
                    })
                    .count()
            })
            .sum()
    }

    pub fn bytes(&self) -> usize {
        self.data.len()
    }
}

/// running box-average overview (straight alpha) filled band by band
pub struct Overview {
    pub w: usize,
    pub h: usize,
    pw: usize,
    ph: usize,
    acc: Vec<[u32; 5]>, // n, opaque, sum r, sum g, sum b (0..255 over opaque pixels)
}

impl Overview {
    /// overview of a pw x ph print, longest side at most `max`
    pub fn new(pw: usize, ph: usize, max: usize) -> Overview {
        let s = (pw.max(ph) as f64 / max as f64).max(1.0);
        let w = ((pw as f64 / s).round() as usize).max(1);
        let h = ((ph as f64 / s).round() as usize).max(1);
        Overview {
            w,
            h,
            pw,
            ph,
            acc: vec![[0; 5]; w * h],
        }
    }

    pub fn add_rows(&mut self, y0: usize, rgba: &[u8]) {
        let pw = self.pw;
        for (r, row) in rgba.chunks_exact(pw * 4).enumerate() {
            let y = y0 + r;
            let oy = (y * self.h / self.ph).min(self.h - 1);
            for x in 0..pw {
                let ox = (x * self.w / pw).min(self.w - 1);
                let px = &row[x * 4..x * 4 + 4];
                let a = &mut self.acc[oy * self.w + ox];
                a[0] += 1;
                if px[3] != 0 {
                    a[1] += 1;
                    // real values: true-colour output (halftone only on fades) is not 0/255
                    a[2] += px[0] as u32;
                    a[3] += px[1] as u32;
                    a[4] += px[2] as u32;
                }
            }
        }
    }

    pub fn rgba(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.w * self.h * 4];
        for (i, a) in self.acc.iter().enumerate() {
            if a[0] > 0 && a[1] > 0 {
                out[i * 4] = ((a[2] + a[1] / 2) / a[1]) as u8;
                out[i * 4 + 1] = ((a[3] + a[1] / 2) / a[1]) as u8;
                out[i * 4 + 2] = ((a[4] + a[1] / 2) / a[1]) as u8;
                out[i * 4 + 3] = ((a[1] * 255 + a[0] / 2) / a[0]) as u8;
            }
        }
        out
    }
}

/// continuous-tone RGBA8 (straight alpha) of prepared pixels, for viewing
pub fn tone_rgba(c: &[f32], a: &[f32]) -> Vec<u8> {
    let mut out = vec![0u8; a.len() * 4];
    out.par_chunks_mut(4).enumerate().for_each(|(p, o)| {
        for ch in 0..3 {
            o[ch] = (c[p * 3 + ch] * 255.0).round().clamp(0.0, 255.0) as u8;
        }
        o[3] = (a[p] * 255.0).round().clamp(0.0, 255.0) as u8;
    });
    out
}

/// render the whole print band by band into a Packed buffer (+ checks).
/// `on_band(y0, y1, rgba)` sees each finished band (for overviews / progress) and can
/// stop the render by returning false.
pub fn render_packed(
    plan: &Plan,
    band_px: usize,
    mut on_band: impl FnMut(usize, usize, &[u8]) -> bool,
) -> Option<(Packed, Checks)> {
    let mut packed = Packed::for_settings(plan.w, plan.h, &plan.settings);
    let pure = plan.settings.pure_colours();
    let rows = plan.band_rows(band_px);
    let mut checks = Checks {
        semi: 0,
        impure: 0,
        tiny: 0,
        hidden: 0,
    };
    let mut y0 = 0;
    while y0 < plan.h {
        let y1 = (y0 + rows).min(plan.h);
        let o = plan.render(0, y0, plan.w, y1);
        let c = pixel_checks_for(&o, pure);
        checks.semi += c.semi;
        checks.impure += c.impure;
        checks.hidden += c.hidden;
        packed.put_rows(y0, &o);
        if !on_band(y0, y1, &o) {
            return None;
        }
        y0 = y1;
    }
    checks.tiny = packed.tiny_islands(plan.settings.dpi);
    Some((packed, checks))
}
