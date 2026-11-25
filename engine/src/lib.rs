#![allow(clippy::too_many_arguments, clippy::needless_range_loop)]
//! StudioZea DTF engine. Rust port of prototype/studiozea_ref.py (the tested spec):
//! same thresholds, same order of floating point operations, same pixels.

pub mod band;
pub mod bg;
pub mod distress;
pub mod dtf;
pub mod erase;
pub mod filters;
pub mod gang;
pub mod io;
pub mod library;
pub mod noise;
pub mod render;
pub mod resize;
pub mod sablon;
pub mod screen;
pub mod testsheet;
pub mod text;
pub mod upscale;
#[cfg(feature = "gpu")]
mod upscale_gpu;

pub use render::{render, GridChoice, KMode, Pattern, Settings, Shirt, TextStyle, TrueDots};
pub use screen::{choose_grid, screen_info, Grid, ScreenInfo, Shape};

use serde::{Deserialize, Serialize};

/// What the app / CLI asks for: background, crop, print size, screen settings.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Job {
    /// "otomatis" (the shirt colour removes black/white), "warna" (flat coloured bg), "tidak"
    pub remove_bg: BgMode,
    /// eraser / restore strokes in source pixels, applied after background removal
    pub erase: Vec<erase::Stroke>,
    /// crop in source pixels (x0, y0, x1, y1), applied after background removal
    pub crop: Option<[usize; 4]>,
    pub auto_trim: bool,
    /// turn the design clockwise by 0, 90, 180 or 270 degrees (after crop and trim,
    /// before the halftone, so the screen angles stay as set)
    pub rotate: u16,
    /// mirror left-right (after turning, as the print is seen)
    pub mirror: bool,
    /// background flood tolerance (RGB distance). None = automatic (see BgParams::auto_for)
    pub bg_tolerance: Option<f32>,
    /// also remove enclosed areas of exactly the background colour. None = automatic
    pub bg_pockets: Option<bool>,
    /// print width in cm; height follows the aspect ratio. None = keep pixels
    pub width_cm: Option<f64>,
    pub settings: Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BgMode {
    Otomatis,
    Warna,
    Tidak,
}

impl Default for Job {
    fn default() -> Self {
        Job {
            remove_bg: BgMode::Otomatis,
            erase: Vec::new(),
            crop: None,
            auto_trim: false,
            rotate: 0,
            mirror: false,
            bg_tolerance: None,
            bg_pockets: None,
            width_cm: None,
            settings: Settings::default(),
        }
    }
}

pub struct Prepared {
    pub w: usize,
    pub h: usize,
    /// width in source pixels after background removal / crop / trim, before resize
    pub pre_w: usize,
    /// the part of the source image this covers (x0, y0, x1, y1) after crop and trim
    pub src_box: [usize; 4],
    pub rgb: Vec<f32>,
    pub alpha: Vec<f32>,
    pub bg: Option<[f32; 3]>,
    /// background parameters actually used (tolerance, pockets)
    pub bg_used: Option<(f32, bool)>,
    /// "Lindungi teks": 1 = print solid (lettering), per pixel; None = off / no text
    pub solid: Option<Vec<f32>>,
}

pub struct Output {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
    pub checks: io::Checks,
    pub bg: Option<[f32; 3]>,
    pub screen: ScreenInfo,
}

fn crop(p: &Prepared, b: [usize; 4]) -> Prepared {
    let [x0, y0, x1, y1] = b;
    let (x0, y0) = (x0.min(p.w - 1), y0.min(p.h - 1));
    let (x1, y1) = (x1.min(p.w), y1.min(p.h));
    let (w, h) = (x1.saturating_sub(x0).max(1), y1.saturating_sub(y0).max(1));
    let mut rgb = Vec::with_capacity(w * h * 3);
    let mut alpha = Vec::with_capacity(w * h);
    let mut solid = p.solid.as_ref().map(|_| Vec::with_capacity(w * h));
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let q = y * p.w + x;
            rgb.extend_from_slice(&p.rgb[q * 3..q * 3 + 3]);
            alpha.push(p.alpha[q]);
            if let (Some(d), Some(s)) = (solid.as_mut(), p.solid.as_ref()) {
                d.push(s[q]);
            }
        }
    }
    Prepared {
        w,
        h,
        pre_w: w,
        src_box: [p.src_box[0] + x0, p.src_box[1] + y0, p.src_box[0] + x0 + w, p.src_box[1] + y0 + h],
        rgb,
        alpha,
        bg: p.bg,
        bg_used: p.bg_used,
        solid,
    }
}

/// print size in pixels for a prepared (source resolution) image
pub fn print_size(p: &Prepared, job: &Job) -> (usize, usize) {
    match job.width_cm {
        Some(cm) => {
            let ow = (cm / 2.54 * job.settings.dpi).round_ties_even().max(1.0) as usize;
            let oh = ((ow as f64) * p.h as f64 / p.w as f64)
                .round_ties_even()
                .max(1.0) as usize;
            (ow, oh)
        }
        None => (p.w, p.h),
    }
}

/// 'otomatis': does this opaque image have a flat background the shirt colour does not
/// remove by itself? Black shirt: black = no ink, so only a non-black flat background
/// (white paper, navy, maroon...) is removed; white shirt: likewise for non-white;
/// coloured shirt: any flat background. Transparent images are used as they are.
pub fn auto_removes_bg(src: &io::Loaded, shirt: Shirt) -> bool {
    if !src.alpha.iter().all(|&a| a >= 1.0) || !bg::has_flat_border(&src.rgb, src.w, src.h) {
        return false;
    }
    let b = bg::border_colour(&src.rgb, src.w, src.h);
    match shirt {
        Shirt::Hitam => !bg::is_black(b),
        Shirt::Putih => !bg::is_white(b),
        Shirt::Warna(_) => true,
    }
}

/// background removal + crop + trim at source resolution (no resize)
pub fn prepare_source(src: &io::Loaded, job: &Job) -> Prepared {
    prepare_source_text(src, job, None)
}

/// `prepare_source` with the detected text of the source (`text::detect`), used when
/// `job.settings.protect_text` (manual text strokes work without it)
pub fn prepare_source_text(src: &io::Loaded, job: &Job, text: Option<&text::TextMap>) -> Prepared {
    let mut p = Prepared {
        w: src.w,
        h: src.h,
        pre_w: src.w,
        src_box: [0, 0, src.w, src.h],
        rgb: src.rgb.clone(),
        alpha: src.alpha.clone(),
        bg: None,
        bg_used: None,
        solid: None,
    };
    let auto_bg = job.remove_bg == BgMode::Otomatis && auto_removes_bg(src, job.settings.shirt);
    if job.remove_bg == BgMode::Warna || auto_bg {
        let auto = bg::BgParams::auto_for(bg::border_colour(&p.rgb, p.w, p.h));
        let prm = bg::BgParams {
            tolerance: job.bg_tolerance.unwrap_or(auto.tolerance),
            pockets: job.bg_pockets.unwrap_or(auto.pockets),
        };
        let r = bg::remove_bg_with(&p.rgb, p.w, p.h, prm);
        p = Prepared {
            w: p.w,
            h: p.h,
            pre_w: p.w,
            src_box: p.src_box,
            rgb: r.rgb,
            alpha: r.alpha,
            bg: Some(r.bg),
            bg_used: Some((prm.tolerance, prm.pockets)),
            solid: None,
        };
    }
    if !job.erase.is_empty() {
        erase::apply(&mut p.rgb, &mut p.alpha, &src.rgb, &src.alpha, p.w, p.h, &job.erase);
    }
    if job.settings.protect_text {
        let mut m = match text {
            Some(t) => text::solid_mask(t, &p.rgb, &p.alpha, p.w, p.h),
            None => vec![0.0; p.w * p.h],
        };
        erase::apply_text(&mut m, p.w, p.h, &job.erase);
        if m.iter().any(|&v| v > 0.0) {
            p.solid = Some(m);
        }
    }
    if let Some(b) = job.crop {
        p = crop(&p, b);
    }
    if job.auto_trim {
        if let Some((x0, y0, x1, y1)) =
            resize::autotrim(&p.rgb, &p.alpha, p.w, p.h, job.settings.shirt)
        {
            p = crop(&p, [x0, y0, x1, y1]);
        }
    }
    if job.mirror || job.rotate % 360 != 0 {
        let (rgb, alpha, w, h) = resize::orient(&p.rgb, &p.alpha, p.w, p.h, job.rotate, job.mirror);
        let solid = p.solid.as_ref().map(|s| resize::orient_plane(s, p.w, p.h, job.rotate, job.mirror));
        p = Prepared { w, h, pre_w: w, rgb, alpha, solid, ..p };
    }
    p
}

/// background removal + crop + trim + resize to print pixels
pub fn prepare(src: &io::Loaded, job: &Job) -> Prepared {
    prepare_text(src, job, None)
}

/// `prepare` with the detected text (see `prepare_source_text`)
pub fn prepare_text(src: &io::Loaded, job: &Job, text: Option<&text::TextMap>) -> Prepared {
    let p = prepare_source_text(src, job, text);
    let (ow, oh) = print_size(&p, job);
    if ow != p.w || oh != p.h {
        let (rgb, alpha) = resize::premul_resize_with(&p.rgb, &p.alpha, p.w, p.h, ow, oh, job.settings.resample);
        let solid = p.solid.as_ref().map(|s| resize::resize_plane(s, p.w, p.h, ow, oh, job.settings.resample));
        return Prepared {
            w: ow,
            h: oh,
            pre_w: p.w,
            src_box: p.src_box,
            rgb,
            alpha,
            bg: p.bg,
            bg_used: p.bg_used,
            solid,
        };
    }
    p
}

/// light-weight plan: prepared image at source resolution, rendered lazily per window
pub fn plan(src: &io::Loaded, job: &Job) -> band::Plan {
    let p = prepare_source(src, job);
    let (ow, oh) = print_size(&p, job);
    band::Plan::new(p, ow, oh, job.settings.clone())
}

pub fn run(src: &io::Loaded, job: &Job) -> Output {
    run_text(src, job, None)
}

/// `run` with the detected text (see `prepare_source_text`)
pub fn run_text(src: &io::Loaded, job: &Job, text: Option<&text::TextMap>) -> Output {
    let p = prepare_text(src, job, text);
    let rgba = render::render_masked(&p.rgb, &p.alpha, p.solid.as_deref(), p.w, p.h, &job.settings);
    let checks = io::checks_for(&rgba, p.w, p.h, job.settings.dpi, job.settings.pure_colours());
    let s = &job.settings;
    Output {
        w: p.w,
        h: p.h,
        rgba,
        checks,
        bg: p.bg,
        screen: screen_info(s.dpi, s.lpi, s.angles()[0], s.grid()),
    }
}
