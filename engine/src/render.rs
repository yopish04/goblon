//! Full render: tone -> noise clip -> calibration -> per-channel screen -> 8 pure
//! colours + binary alpha -> DTF clean-up. Port of `render` in studiozea_ref.py.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::distress;
use crate::dtf::{bayer_bits_at, calibrate, channel_at, clean, ed_bits_at, fm_bits_at, interlock_snap, safe_zone, sharp_edges};
use crate::noise::{green_noise, Grain};
use crate::screen::{choose_grid, Grid, Shape};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "type", content = "rgb")]
pub enum Shirt {
    /// black shirt: R, G, B screened, no ink = black = transparent
    Hitam,
    /// white shirt: C, M, Y screened, no ink = white = transparent
    Putih,
    /// coloured shirt (0..1 RGB): opacity screen (FM) + CMY on white underbase
    Warna([f32; 3]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Pattern {
    Am,
    Fm,
    Grain(Grain),
    /// ordered dither (Bayer 8 x 8) on blocks of the minimum dot
    Bayer,
    /// Floyd-Steinberg error diffusion on blocks of the minimum dot
    Difusi,
    /// neat AM: Euclidean dots, dot-on-dot at one angle (default 45), integer grid, no
    /// FM at the tone ends
    Interlock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KMode {
    Off,
    Garis,
    Gcr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GridChoice {
    Auto,
    Rapi,
    Presisi,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub shirt: Shirt,
    pub lpi: f64,
    pub shape: Shape,
    pub dpi: f64,
    pub grid: GridChoice,
    pub pattern: Pattern,
    /// per-channel angles; None = from `angle` and `rosette`
    pub angles: Option<[f64; 3]>,
    /// base screen angle in degrees (None = 22.5 like the reference)
    pub angle: Option<f64>,
    /// rosette (C/Y angle, M angle + 45) or dot-on-dot (all the same angle);
    /// None = rosette except on black shirts and from 55 LPI
    pub rosette: Option<bool>,
    /// "halftone only on fades": Some(area) prints the design in its true colours and
    /// screens only where it fades into the shirt colour (normalised distance from the
    /// shirt colour below `area`, 0..1). Some(0) = no halftone at all: every pixel at
    /// least half opaque prints in its own full colour, the rest not at all. None = everything screened into the
    /// 8 pure colours (the reference look)
    pub halftone_area: Option<f32>,
    /// no-halftone mode: colours closer to the shirt colour than this (normalised
    /// distance, 0..1) are not printed (near-black shadows / JPEG noise on a black shirt)
    pub shirt_cut: f32,
    /// filter for scaling the image to the print size
    pub resample: crate::resize::Resample,
    pub k: KMode,
    pub gcr: f32,
    /// coloured shirt only: knock out design colours close to the shirt colour
    pub knockout: f32,
    pub edges: bool,
    pub hybrid: bool,
    pub calibrate: bool,
    pub clip: f32,
    /// "Lindungi teks": lettering (the `solid` mask of the prepared image, see text.rs)
    /// prints solid in its own colours instead of screened
    #[serde(default)]
    pub protect_text: bool,
    /// how protected lettering prints: solid, or fine dots (the same screen at a higher
    /// LPI, see `text_lpi`; AM and interlock only, the other patterns are already fine)
    #[serde(default)]
    pub text_style: TextStyle,
    /// vintage texture layers (empty = no texture, render unchanged)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub distress: Vec<distress::Layer>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shirt: Shirt::Hitam,
            lpi: 35.0,
            shape: Shape::Bulat,
            dpi: 300.0,
            grid: GridChoice::Auto,
            pattern: Pattern::Am,
            angles: None,
            angle: None,
            rosette: None,
            halftone_area: None,
            shirt_cut: 0.12,
            resample: crate::resize::Resample::Lanczos,
            k: KMode::Off,
            gcr: 1.0,
            knockout: 0.0,
            edges: false,
            hybrid: true,
            calibrate: true,
            clip: 0.02,
            protect_text: false,
            text_style: TextStyle::Padat,
            distress: Vec::new(),
        }
    }
}

impl Settings {
    pub fn grid(&self) -> Grid {
        if self.pattern == Pattern::Interlock {
            return Grid::Rapi;
        }
        match self.grid {
            GridChoice::Rapi => Grid::Rapi,
            GridChoice::Presisi => Grid::Presisi,
            GridChoice::Auto => choose_grid(self.dpi, self.lpi, self.angles()[0]),
        }
    }

    /// output limited to the 8 pure colours (false: true colours, see `halftone_area`)
    pub fn pure_colours(&self) -> bool {
        self.fades_area().is_none() && !(self.protect_text && self.text_fine_lpi().is_none())
    }

    /// "Lindungi teks" with fine dots: the LPI the lettering is screened at (twice the
    /// screen, at least 45 and at most 75 LPI or dpi / 4, never coarser than the screen).
    /// None = lettering prints solid
    pub fn text_fine_lpi(&self) -> Option<f64> {
        let screened = matches!(self.pattern, Pattern::Am | Pattern::Interlock);
        (self.protect_text && self.text_style == TextStyle::Halus && screened).then(|| (self.lpi * 2.0).max(45.0).min(75.0f64.min(self.dpi / 4.0)).max(self.lpi))
    }

    /// the valid `halftone_area` (0 <= area <= 1; 0 = no halftone), if that mode is on
    pub fn fades_area(&self) -> Option<f32> {
        self.halftone_area.filter(|a| a.is_finite() && *a >= 0.0).map(|a| a.min(1.0))
    }

    pub fn angles(&self) -> [f64; 3] {
        if let Some(a) = self.angles {
            return a;
        }
        if self.pattern == Pattern::Interlock {
            return [self.angle.filter(|a| a.is_finite()).map_or(45.0, |a| a.rem_euclid(360.0)); 3];
        }
        let a = self.angle.filter(|a| a.is_finite()).map_or(22.5, |a| a.rem_euclid(360.0));
        let rosette = self
            .rosette
            .unwrap_or(!(self.shirt == Shirt::Hitam || self.lpi >= 55.0));
        if rosette {
            [a, (a + 45.0).rem_euclid(360.0), a]
        } else {
            [a; 3]
        }
    }
}

/// one channel of the dither / interlock patterns (port of `_screen1`)
#[allow(clippy::too_many_arguments)]
fn screen1(t: &[f32], w: usize, h: usize, ox: usize, oy: usize, s: &Settings, ang: f64, angs: [f64; 3]) -> Vec<bool> {
    match s.pattern {
        Pattern::Bayer => bayer_bits_at(t, w, h, s.dpi, ox, oy),
        Pattern::Difusi => ed_bits_at(t, w, h, s.dpi, ox, oy),
        _ => {
            let grid = s.grid();
            let t = if s.calibrate {
                calibrate(t, s.lpi, s.shape, s.dpi, angs, grid)
            } else {
                t.to_vec()
            };
            let (lo, hi) = safe_zone(s.dpi, s.lpi, ang, s.shape, grid);
            channel_at(&interlock_snap(&t, lo, hi), w, h, ox, oy, s.dpi, s.lpi, ang, s.shape, grid, false)
        }
    }
}

fn plane(rgb: &[f32], c: usize) -> Vec<f32> {
    rgb.iter().skip(c).step_by(3).copied().collect()
}

/// screen 3 interleaved channels -> interleaved bits ((ox, oy) = window origin)
fn screen3(
    tone: &[f32],
    w: usize,
    h: usize,
    ox: usize,
    oy: usize,
    s: &Settings,
    ang: [f64; 3],
) -> Vec<bool> {
    let grid = s.grid();
    let planes: Vec<Vec<bool>> = (0..3)
        .map(|c| {
            let t = plane(tone, c);
            match s.pattern {
                Pattern::Am => {
                    let t = if s.calibrate {
                        calibrate(&t, s.lpi, s.shape, s.dpi, ang, grid)
                    } else {
                        t
                    };
                    channel_at(&t, w, h, ox, oy, s.dpi, s.lpi, ang[c], s.shape, grid, s.hybrid)
                }
                Pattern::Fm => fm_bits_at(&t, w, h, s.dpi, ox, oy),
                Pattern::Grain(g) => {
                    let tile = green_noise(g);
                    (0..w * h)
                        .map(|p| t[p] > tile.at(p % w + ox, p / w + oy))
                        .collect()
                }
                Pattern::Bayer | Pattern::Difusi | Pattern::Interlock => screen1(&t, w, h, ox, oy, s, ang[c], ang),
            }
        })
        .collect();
    let mut out = vec![false; w * h * 3];
    for p in 0..w * h {
        for c in 0..3 {
            out[p * 3 + c] = planes[c][p];
        }
    }
    out
}

/// "Lindungi teks": how the lettering prints
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextStyle {
    /// solid in its own colours (like a spot colour)
    #[default]
    Padat,
    /// the same screen with finer dots (`Settings::text_fine_lpi`)
    Halus,
}

/// c: interleaved RGB 0..1 (background already un-mixed), a: alpha 0..1.
/// Returns RGBA8 with alpha only 0/255 and colours only 0/255.
pub fn render(c: &[f32], a: &[f32], w: usize, h: usize, s: &Settings) -> Vec<u8> {
    render_masked(c, a, None, w, h, s)
}

/// `render` with the "Lindungi teks" mask (`solid`, 0..1 per pixel, used when
/// `s.protect_text`)
pub fn render_masked(c: &[f32], a: &[f32], solid: Option<&[f32]>, w: usize, h: usize, s: &Settings) -> Vec<u8> {
    if distress::needs_print_size(s) {
        return render_at_masked(c, a, solid, w, h, 0, 0, &distress::with_print_size(s, w, h));
    }
    render_at_masked(c, a, solid, w, h, 0, 0, s)
}

/// `render` for a window whose top-left pixel is (ox, oy) in the full print. Only the
/// position-dependent parts (threshold tiles, noise, FM blocks) use the origin, so a
/// window with enough halo reproduces the full render exactly (see crate::band).
/// ox and oy must be multiples of `dtf::mindot_px(s.dpi)`.
///
/// Texture layers (`s.distress`): pudar layers scale the opacity before screening
/// (= tone on a black shirt, ink on a white shirt, opacity on a coloured shirt), lubang
/// layers knock holes into the final alpha (dot-aware) before `dtf::clean`, which
/// stays the last step. A gambar layer without a size uses the texture's native
/// pixel size here; `render` and `band::Plan` resolve it to the print width first.
pub fn render_at(
    c: &[f32],
    a: &[f32],
    w: usize,
    h: usize,
    ox: usize,
    oy: usize,
    s: &Settings,
) -> Vec<u8> {
    render_at_masked(c, a, None, w, h, ox, oy, s)
}

/// `render_at` with the "Lindungi teks" mask: pixels with `solid` >= 0.5 print solid
/// in their own colour (like `render_solid`) after screening, before the texture holes
/// and the clean-up (so vintage textures still wear the lettering)
pub fn render_at_masked(
    c: &[f32],
    a: &[f32],
    solid: Option<&[f32]>,
    w: usize,
    h: usize,
    ox: usize,
    oy: usize,
    s: &Settings,
) -> Vec<u8> {
    let interlock;
    let s = if s.pattern == Pattern::Interlock && s.shape != Shape::Bulat {
        interlock = Settings { shape: Shape::Bulat, ..s.clone() };
        &interlock
    } else {
        s
    };
    let n = w * h;
    let mut o = vec![0u8; n * 4];
    // pudar layers: faded opacity (tone / ink / opacity are all proportional to it)
    let faded: Vec<f32>;
    let a: &[f32] = match distress::fade(s, w, h, ox, oy) {
        Some(f) => {
            faded = a.iter().zip(&f).map(|(x, y)| x * y).collect();
            &faded
        }
        None => a,
    };
    let screen = |s: &Settings, o: &mut [u8]| {
        if let Some(area) = s.fades_area() {
            render_fades(c, a, w, h, ox, oy, s, area, o);
        } else {
            render_pure(c, a, w, h, ox, oy, s, s.angles(), o);
        }
    };
    screen(s, &mut o);
    if let Some(m) = solid.filter(|_| s.protect_text) {
        match s.text_fine_lpi() {
            // the lettering from a second, finer screen of the same window
            Some(lpi) if m.iter().any(|&v| v >= 0.5) => {
                let fine = Settings { lpi, ..s.clone() };
                let mut f = vec![0u8; n * 4];
                screen(&fine, &mut f);
                o.par_chunks_mut(4).zip(f.par_chunks(4)).zip(m.par_iter()).for_each(|((px, fp), &v)| {
                    if v >= 0.5 {
                        px.copy_from_slice(fp);
                    }
                });
            }
            Some(_) => {}
            None => keep_solid(c, a, m, s, &mut o),
        }
    }
    // lubang layers: holes in the printed ink (removes the white underbase too)
    if let Some(m) = distress::holes(s, w, h, ox, oy) {
        distress::knock_out(&mut o, w, h, &m, s);
    }
    clean(&mut o, w, h, s.dpi);
    o
}

/// "halftone only on fades" (port of `render_fades` in studiozea_ref.py): true colours,
/// screened only where the design fades into the shirt colour; the dots get the colour
/// that keeps the average, so the border between solid and dots has no seam
#[allow(clippy::too_many_arguments)]
fn render_fades(c: &[f32], a: &[f32], w: usize, h: usize, ox: usize, oy: usize, s: &Settings, area: f32, o: &mut [u8]) {
    let n = w * h;
    let sh: [f32; 3] = match s.shirt {
        Shirt::Hitam => [0.0; 3],
        Shirt::Putih => [1.0; 3],
        Shirt::Warna(rgb) => rgb,
    };
    let coloured = matches!(s.shirt, Shirt::Warna(_));
    if area == 0.0 {
        render_solid(c, a, w, h, s, sh, coloured, o);
        return;
    }
    let mut seen = vec![0f32; n * 3];
    let mut cov = vec![0f32; n];
    seen.par_chunks_mut(3)
        .zip(cov.par_iter_mut())
        .enumerate()
        .for_each(|(p, (sp, cv))| {
            let mut al = a[p];
            if coloured && s.knockout > 0.0 {
                let dr = c[p * 3] - sh[0];
                let dg = c[p * 3 + 1] - sh[1];
                let db = c[p * 3 + 2] - sh[2];
                let d = (dr * dr + dg * dg + db * db).sqrt();
                al *= ((d - s.knockout + 0.06) / 0.06).clamp(0.0, 1.0);
            }
            let mut dist = 0f32;
            for ch in 0..3 {
                let v = al * c[p * 3 + ch] + (1.0 - al) * sh[ch];
                sp[ch] = v;
                dist = dist.max((v - sh[ch]).abs() / sh[ch].max(1.0 - sh[ch]));
            }
            if dist < s.clip {
                dist = 0.0;
            }
            *cv = (dist / area).min(1.0);
        });
    let ang = s.angles()[0];
    let grid = s.grid();
    let mut b: Vec<bool> = match s.pattern {
        Pattern::Am => {
            let t = if s.calibrate {
                calibrate(&cov, s.lpi, s.shape, s.dpi, [ang; 3], grid)
            } else {
                cov.clone()
            };
            channel_at(&t, w, h, ox, oy, s.dpi, s.lpi, ang, s.shape, grid, s.hybrid)
        }
        Pattern::Fm => fm_bits_at(&cov, w, h, s.dpi, ox, oy),
        Pattern::Grain(g) => {
            let tile = green_noise(g);
            (0..n).map(|p| cov[p] > tile.at(p % w + ox, p / w + oy)).collect()
        }
        Pattern::Bayer | Pattern::Difusi | Pattern::Interlock => screen1(&cov, w, h, ox, oy, s, ang, [ang; 3]),
    };
    if s.edges {
        let tone3: Vec<f32> = cov.iter().flat_map(|&v| [v; 3]).collect();
        let mut b3: Vec<bool> = b.iter().flat_map(|&v| [v; 3]).collect();
        sharp_edges(&tone3, &mut b3, w, h, s.dpi);
        b = b3.chunks_exact(3).map(|x| x[0] || x[1] || x[2]).collect();
    }
    o.par_chunks_mut(4).enumerate().for_each(|(p, px)| {
        if b[p] {
            let k = cov[p].max(1e-6);
            for ch in 0..3 {
                let v = (sh[ch] + (seen[p * 3 + ch] - sh[ch]) / k).clamp(0.0, 1.0);
                px[ch] = (v * 255.0).round_ties_even() as u8;
            }
            px[3] = 255;
        }
    });
}

/// "Lindungi teks": masked pixels print like `render_solid` (own colour when at least
/// half opaque and not the shirt colour, else nothing), whatever the screen did there
fn keep_solid(c: &[f32], a: &[f32], m: &[f32], s: &Settings, o: &mut [u8]) {
    let sh: [f32; 3] = match s.shirt {
        Shirt::Hitam => [0.0; 3],
        Shirt::Putih => [1.0; 3],
        Shirt::Warna(rgb) => rgb,
    };
    let coloured = matches!(s.shirt, Shirt::Warna(_));
    o.par_chunks_mut(4).enumerate().for_each(|(p, px)| {
        if m[p] < 0.5 {
            return;
        }
        let mut al = a[p];
        if coloured && s.knockout > 0.0 {
            let dr = c[p * 3] - sh[0];
            let dg = c[p * 3 + 1] - sh[1];
            let db = c[p * 3 + 2] - sh[2];
            let d = (dr * dr + dg * dg + db * db).sqrt();
            al *= ((d - s.knockout + 0.06) / 0.06).clamp(0.0, 1.0);
        }
        let mut own = 0f32;
        for ch in 0..3 {
            own = own.max((c[p * 3 + ch] - sh[ch]).abs() / sh[ch].max(1.0 - sh[ch]));
        }
        if al >= 0.5 && own >= s.clip.max(s.shirt_cut) {
            for ch in 0..3 {
                px[ch] = (c[p * 3 + ch].clamp(0.0, 1.0) * 255.0).round_ties_even() as u8;
            }
            px[3] = 255;
        } else {
            px.fill(0);
        }
    });
}

/// no halftone and nothing half-way: a pixel at least half opaque (and not the shirt
/// colour) prints in its own full colour, every other pixel not at all
#[allow(clippy::too_many_arguments)]
fn render_solid(c: &[f32], a: &[f32], w: usize, h: usize, s: &Settings, sh: [f32; 3], coloured: bool, o: &mut [u8]) {
    let _ = (w, h);
    o.par_chunks_mut(4).enumerate().for_each(|(p, px)| {
        let mut al = a[p];
        if coloured && s.knockout > 0.0 {
            let dr = c[p * 3] - sh[0];
            let dg = c[p * 3 + 1] - sh[1];
            let db = c[p * 3 + 2] - sh[2];
            let d = (dr * dr + dg * dg + db * db).sqrt();
            al *= ((d - s.knockout + 0.06) / 0.06).clamp(0.0, 1.0);
        }
        let mut own = 0f32;
        for ch in 0..3 {
            own = own.max((c[p * 3 + ch] - sh[ch]).abs() / sh[ch].max(1.0 - sh[ch]));
        }
        if al >= 0.5 && own >= s.clip.max(s.shirt_cut) {
            for ch in 0..3 {
                px[ch] = (c[p * 3 + ch].clamp(0.0, 1.0) * 255.0).round_ties_even() as u8;
            }
            px[3] = 255;
        }
    });
}

/// the reference look: every channel screened, output limited to the 8 pure colours
#[allow(clippy::too_many_arguments)]
fn render_pure(c: &[f32], a: &[f32], w: usize, h: usize, ox: usize, oy: usize, s: &Settings, ang: [f64; 3], o: &mut [u8]) {
    let n = w * h;
    match s.shirt {
        Shirt::Hitam => {
            let mut tone: Vec<f32> = (0..n * 3).map(|i| c[i] * a[i / 3]).collect();
            for p in 0..n {
                let m = tone[p * 3].max(tone[p * 3 + 1]).max(tone[p * 3 + 2]);
                if m < s.clip {
                    tone[p * 3] = 0.0;
                    tone[p * 3 + 1] = 0.0;
                    tone[p * 3 + 2] = 0.0;
                }
            }
            let mut b = screen3(&tone, w, h, ox, oy, s, ang);
            if s.edges {
                sharp_edges(&tone, &mut b, w, h, s.dpi);
            }
            o.par_chunks_mut(4).enumerate().for_each(|(p, px)| {
                let al = b[p * 3] || b[p * 3 + 1] || b[p * 3 + 2];
                for ch in 0..3 {
                    px[ch] = if b[p * 3 + ch] && al { 255 } else { 0 };
                }
                px[3] = if al { 255 } else { 0 };
            });
        }
        Shirt::Putih | Shirt::Warna(_) => {
            let coloured = matches!(s.shirt, Shirt::Warna(_));
            let mut ink: Vec<f32> = (0..n * 3)
                .map(|i| {
                    if coloured {
                        (1.0 - c[i]) * 1.0
                    } else {
                        (1.0 - c[i]) * a[i / 3]
                    }
                })
                .collect();
            for p in 0..n {
                let m = ink[p * 3].max(ink[p * 3 + 1]).max(ink[p * 3 + 2]);
                if m < s.clip {
                    ink[p * 3] = 0.0;
                    ink[p * 3 + 1] = 0.0;
                    ink[p * 3 + 2] = 0.0;
                }
            }
            let mn: Vec<f32> = (0..n)
                .map(|p| ink[p * 3].min(ink[p * 3 + 1]).min(ink[p * 3 + 2]))
                .collect();
            let kk: Vec<f32> = match s.k {
                KMode::Off => vec![0.0; n],
                KMode::Gcr => mn.iter().map(|&m| s.gcr * m).collect(),
                KMode::Garis => mn.iter().map(|&m| if m > 0.9 { m } else { 0.0 }).collect(),
            };
            let inkp: Vec<f32> = (0..n * 3)
                .map(|i| {
                    let k = kk[i / 3];
                    let v = (ink[i] - k) / (1.0 - k).max(1e-3);
                    v.clamp(0.0, 1.0)
                })
                .collect();
            let mut b = screen3(&inkp, w, h, ox, oy, s, ang);
            if s.edges {
                sharp_edges(&inkp, &mut b, w, h, s.dpi);
            }
            let kb: Vec<bool> = if s.k != KMode::Off {
                let grid = s.grid();
                let kt = if s.calibrate {
                    calibrate(&kk, s.lpi, s.shape, s.dpi, [45.0; 3], grid)
                } else {
                    kk.clone()
                };
                channel_at(&kt, w, h, ox, oy, s.dpi, s.lpi, 45.0, s.shape, grid, s.hybrid)
            } else {
                vec![false; n]
            };
            let alpha: Vec<bool> = match s.shirt {
                Shirt::Warna(sh) => {
                    let op: Vec<f32> = if s.knockout > 0.0 {
                        (0..n)
                            .map(|p| {
                                let dr = c[p * 3] - sh[0];
                                let dg = c[p * 3 + 1] - sh[1];
                                let db = c[p * 3 + 2] - sh[2];
                                let d = (dr * dr + dg * dg + db * db).sqrt();
                                a[p] * ((d - s.knockout + 0.06) / 0.06).clamp(0.0, 1.0)
                            })
                            .collect()
                    } else {
                        a.to_vec()
                    };
                    fm_bits_at(&op, w, h, s.dpi, ox, oy)
                }
                _ => (0..n)
                    .map(|p| b[p * 3] || b[p * 3 + 1] || b[p * 3 + 2] || kb[p])
                    .collect(),
            };
            o.par_chunks_mut(4).enumerate().for_each(|(p, px)| {
                for ch in 0..3 {
                    let on = !b[p * 3 + ch] && !kb[p];
                    px[ch] = if on && alpha[p] { 255 } else { 0 };
                }
                px[3] = if alpha[p] { 255 } else { 0 };
            });
        }
    }
}
