//! Sablon (screen printing): colour separation of a prepared design into one film per
//! ink, the way garment separation software works (simulated process / index / spot),
//! with the white underbase and highlight white for dark shirts.
//!
//! The separation is computed from a print model instead of from colour channels:
//!
//! 1. `auto_inks`: the ink set. K-means in OKLab on the design colours (colours close to
//!    the shirt are left out: no ink there shows the shirt), each ink taken from the
//!    more saturated members of its cluster (an average of a cluster is duller than the
//!    colours it stands for, the main reason separations "lose colour"), a highlight
//!    white on dark shirts, print order light to dark, black next to last, highlight
//!    white last (as in Fresener / Separation Studio).
//! 2. `Model`: what a print looks like. Shirt, then the white underbase, then every ink in
//!    print order; a layer with coverage `c` and opacity `a` turns colour `C` into
//!    `(1 - c) C + c (a I + (1 - a) I C)`: the opaque part of the ink plus the light that
//!    comes through it from below (so a red without base on a black shirt is dark, on the
//!    base it is red). All in linear light.
//! 3. `solve`: for every colour of a 21^3 grid the coverages (underbase + inks) whose
//!    print is closest to that colour in OKLab (coordinate descent, a tiny ink cost
//!    keeps it from using inks that change nothing). Pixels interpolate the grid.
//!    This is what makes the base dense under saturated colours and decides where the
//!    highlight white goes: the model says where it is needed to reach the colour.
//! 4. Films: simulated process = every coverage screened as AM halftone, all films at
//!    one angle and LPI (elliptical dot by default, 55 LPI / 25 deg like common sim
//!    process setups); index = every pixel block gets exactly one ink by error
//!    diffusion; spot = the nearest ink, solid. The underbase is choked (`choke_mm`)
//!    so it never peeks out. Specks below the printable size are cleaned.
//! 5. `preview`: the print simulated from the real film bits with the same model, so the
//!    preview shows what the films will print, and `delta_e` measures how close that is
//!    to the design.
//!
//! Instead of `auto_inks` an ink set (`InkSet`: the basic colours + grey, a cheap set,
//! a dark / fire set) can be used, fitted to the shirt (`preset_inks`); inks that end
//! up printing almost nothing are dropped and the rest solved again (`drop_below`, one
//! screen less). Dot styles: one angle (dot on dot), rosette (an angle per film, dots
//! independent) and interlock (half of the colours on the inverted screen: their dots
//! sit between the others instead of on them).

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::dtf::{channel_at, minarea_px, mindot_px};
use crate::filters::{label, max_filter, min_filter};
use crate::render::{Settings, Shirt};
use crate::screen::{choose_grid, Shape};
use crate::Prepared;

/// how the inks are put on the films
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Method {
    /// simulated process: halftone films (photo-like designs)
    #[default]
    Simulasi,
    /// index: square pixels, one ink per pixel by error diffusion
    Index,
    /// spot: flat colours, every pixel the nearest ink (vector-like designs)
    Spot,
}

/// which inks when they are not given one by one
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InkSet {
    /// chosen from the design colours (`auto_inks`)
    #[default]
    Auto,
    /// yellow, red, magenta, blue, cyan, green; grey on dark shirts, black on others
    Dasar,
    /// as `Dasar`, the grey printed before the underbase
    DasarAbuDulu,
    /// yellow, red, blue (black on light / coloured shirts)
    Hemat,
    /// gold, orange, red, maroon, grey: dark, fire, vintage designs
    Gelap,
}

/// how the halftone films of simulated process are screened
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DotStyle {
    /// every film at the same angle: dot on dot
    #[default]
    SatuSudut,
    /// a different angle per film (rosette): dots fall independently
    Roset,
    /// one angle, half of the colours (alternating by hue) on the inverted screen, so
    /// neighbouring colours print side by side instead of on top of each other
    Interlock,
}

/// one ink: colour (sRGB) and opacity over what is below. Plain plastisol on a black
/// shirt without underbase looks dark and dull: about 0.6 (black 0.9, white 0.85)
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ink {
    pub rgb: [u8; 3],
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    /// simulated process: this ink's halftone made this much denser (0.2 = +20 %),
    /// like "Boost Red" by hand
    #[serde(default)]
    pub boost: f32,
}

impl Ink {
    pub const fn new(rgb: [u8; 3], opacity: f32) -> Ink {
        Ink { rgb, opacity, boost: 0.0 }
    }
}

fn default_opacity() -> f32 {
    0.6
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SepSettings {
    pub method: Method,
    /// number of inks when they are chosen automatically (highlight white included)
    pub colors: usize,
    /// the inks in print order (empty = `ink_set`)
    pub inks: Vec<Ink>,
    /// the ink set when `inks` is empty
    pub ink_set: InkSet,
    /// with `inks`: how many of them print before the underbase (e.g. a grey first)
    pub base_after: usize,
    /// white underbase: None = automatic (dark and coloured shirts)
    pub underbase: Option<bool>,
    /// the underbase is this much smaller than the inks on it (mm)
    pub choke_mm: f64,
    /// a highlight white ink (printed last) on dark shirts
    pub highlight: bool,
    pub lpi: f64,
    pub angle: f64,
    pub shape: Shape,
    pub dot_style: DotStyle,
    /// opacity of the (flashed) underbase white
    pub base_opacity: f32,
    /// the colour films (not the underbase, not the highlight white) grow this many
    /// pixels, so a slightly shifted screen leaves no gap or bare base at the edges
    pub expand_px: usize,
    /// inks printing on less than this share of the design (percent of its area) get
    /// no film: the others are solved again without them. 0 = keep all
    pub drop_below: f32,
    /// colours aimed at this much more saturated than the design (0.15 = +15 %
    /// chroma): prints often look calmer than the screen
    pub boost: f32,
    /// "hemat tinta" 0..1: every ink and the underbase get a price, so the solver
    /// leaves out ink that changes the colour only a little (less ink, softer print,
    /// cheaper), the way a hand separation keeps the base thin. 0 = colour first
    pub ink_saver: f32,
    /// "titik maksimum" 0.5..1: in a gradient no film prints more than this (a light
    /// gradient whose colour the inks cannot quite reach otherwise turns into a flat
    /// solid patch, base / ink at 100 %). Colours that are flat in the design (solid
    /// lettering, blocks) still print solid. 1 = off. Hand separations keep ~90 %
    pub max_dot: f32,
    /// on dark shirts the automatic inks are strong colours only (no pastel / cream ink):
    /// the white base and highlight make the light tones, the inks the hue, as in hand
    /// separations (a cream ink at ~100 % made flat patches in light skin and skulls)
    pub strong_inks: bool,
    /// angle of the white films (underbase and highlight white) when they should not
    /// share the colours' screen (e.g. 15 deg away, as in hand-made film separations:
    /// a shifted white screen gives no moire with the colours). None = the colour angle
    pub white_angle: Option<f64>,
}

impl Default for SepSettings {
    fn default() -> Self {
        SepSettings {
            method: Method::Simulasi,
            colors: 6,
            inks: Vec::new(),
            ink_set: InkSet::Auto,
            base_after: 0,
            underbase: None,
            choke_mm: 0.1,
            highlight: true,
            lpi: 55.0,
            angle: 25.0,
            shape: Shape::Elips,
            dot_style: DotStyle::SatuSudut,
            base_opacity: 0.85,
            expand_px: 0,
            drop_below: 0.3,
            boost: 0.0,
            ink_saver: 0.0,
            max_dot: 1.0,
            strong_inks: false,
            white_angle: None,
        }
    }
}

impl SepSettings {
    /// errors in the settings (shown to the user)
    pub fn check(&self) -> Result<(), String> {
        if !(1..=MAX_INKS).contains(&self.colors) && self.inks.is_empty() {
            return Err(format!("jumlah warna harus 1 sampai {MAX_INKS}"));
        }
        if self.inks.len() > MAX_INKS {
            return Err(format!("maksimal {MAX_INKS} tinta"));
        }
        if self.inks.iter().any(|i| !(i.opacity.is_finite() && (0.05..=1.0).contains(&i.opacity))) {
            return Err("opasitas tinta harus 5 sampai 100 %".into());
        }
        if !(self.lpi.is_finite() && (10.0..=100.0).contains(&self.lpi)) {
            return Err("LPI film harus 10 sampai 100".into());
        }
        if !self.angle.is_finite() {
            return Err("sudut film tidak valid".into());
        }
        if !(self.choke_mm.is_finite() && (0.0..=2.0).contains(&self.choke_mm)) {
            return Err("choke underbase harus 0 sampai 2 mm".into());
        }
        if !(self.base_opacity.is_finite() && (0.3..=1.0).contains(&self.base_opacity)) {
            return Err("opasitas underbase harus 30 sampai 100 %".into());
        }
        if self.inks.iter().any(|i| !(i.boost.is_finite() && (0.0..=1.0).contains(&i.boost))) {
            return Err("boost tinta harus 0 sampai 100 %".into());
        }
        if self.base_after > self.inks.len() {
            return Err("posisi underbase tidak valid".into());
        }
        if self.expand_px > 20 {
            return Err("expand warna maksimal 20 px".into());
        }
        if !(self.drop_below.is_finite() && (0.0..=10.0).contains(&self.drop_below)) {
            return Err("batas film kosong harus 0 sampai 10 %".into());
        }
        if !(self.boost.is_finite() && (0.0..=0.5).contains(&self.boost)) {
            return Err("boost warna harus 0 sampai 50 %".into());
        }
        if !(self.ink_saver.is_finite() && (0.0..=1.0).contains(&self.ink_saver)) {
            return Err("hemat tinta harus 0 sampai 100 %".into());
        }
        if !(self.max_dot.is_finite() && (0.5..=1.0).contains(&self.max_dot)) {
            return Err("titik maksimum harus 50 sampai 100 %".into());
        }
        if self.white_angle.is_some_and(|a| !a.is_finite()) {
            return Err("sudut film putih tidak valid".into());
        }
        Ok(())
    }
}

/// most inks on one design (screens on a big automatic press, base excluded)
pub const MAX_INKS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Underbase,
    Ink,
    /// highlight white, printed last
    Highlight,
}

/// a film: which ink, and where it prints (packed bits, row major)
#[derive(Clone, Debug)]
pub struct Film {
    pub role: Role,
    pub ink: Ink,
    /// index into `Separation::inks` (None = the underbase)
    pub src: Option<usize>,
    /// screen angle of a halftone film (simulated process)
    pub angle: Option<f64>,
    /// interlock: dots on the inverted screen (between the others)
    pub inverted: bool,
    pub bits: Bits,
}

#[derive(Clone, Debug)]
pub struct Bits {
    pub w: usize,
    pub h: usize,
    data: Vec<u64>,
}

impl Bits {
    pub fn new(w: usize, h: usize) -> Bits {
        Bits { w, h, data: vec![0; (w * h).div_ceil(64)] }
    }
    #[inline]
    pub fn get(&self, p: usize) -> bool {
        self.data[p >> 6] >> (p & 63) & 1 == 1
    }
    #[inline]
    fn set(&mut self, p: usize, v: bool) {
        if v {
            self.data[p >> 6] |= 1 << (p & 63);
        } else {
            self.data[p >> 6] &= !(1 << (p & 63));
        }
    }
    /// share of the pixels that print
    pub fn coverage(&self) -> f32 {
        let n: u64 = self.data.iter().map(|x| x.count_ones() as u64).sum();
        n as f32 / (self.w * self.h).max(1) as f32
    }
    fn from_bools(v: &[bool], w: usize, h: usize) -> Bits {
        let mut b = Bits::new(w, h);
        for (p, &x) in v.iter().enumerate() {
            if x {
                b.set(p, true);
            }
        }
        b
    }
}

pub struct Separation {
    pub w: usize,
    pub h: usize,
    /// the ink set in print order, dropped inks included (`Film::src` points here)
    pub inks: Vec<Ink>,
    /// how many of `inks` print before the underbase
    pub base_after: usize,
    /// in print order: the underbase (usually first), the inks, highlight white last;
    /// dropped inks have no film
    pub films: Vec<Film>,
    /// the print simulated from the films (RGBA8, alpha 255 where anything prints)
    pub preview: Vec<u8>,
    /// mean colour difference (OKLab x 100, about Delta E) between the design and the
    /// simulated print, both averaged over one halftone cell, where the design prints
    pub delta_e: f32,
}

// ---------------------------------------------------------------- colour

#[inline]
fn lin(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
fn gam(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// linear sRGB -> OKLab
#[inline]
fn oklab(c: [f32; 3]) -> [f32; 3] {
    let l = 0.412_221_46 * c[0] + 0.536_332_55 * c[1] + 0.051_445_995 * c[2];
    let m = 0.211_903_5 * c[0] + 0.680_699_5 * c[1] + 0.107_396_96 * c[2];
    let s = 0.088_302_46 * c[0] + 0.281_718_85 * c[1] + 0.629_978_7 * c[2];
    let (l, m, s) = (l.max(0.0).cbrt(), m.max(0.0).cbrt(), s.max(0.0).cbrt());
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

/// OKLab -> linear sRGB (may be out of gamut)
fn oklab_inv(c: [f32; 3]) -> [f32; 3] {
    let l = c[0] + 0.396_337_78 * c[1] + 0.215_803_76 * c[2];
    let m = c[0] - 0.105_561_346 * c[1] - 0.063_854_17 * c[2];
    let s = c[0] - 0.089_484_18 * c[1] - 1.291_485_5 * c[2];
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

#[inline]
fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

fn chroma(c: [f32; 3]) -> f32 {
    (c[1] * c[1] + c[2] * c[2]).sqrt()
}

/// sRGB 0..1 of an OKLab colour, chroma reduced until it is inside the gamut
fn to_srgb_in_gamut(c: [f32; 3]) -> [f32; 3] {
    let mut k = 1.0f32;
    for _ in 0..40 {
        let l = oklab_inv([c[0], c[1] * k, c[2] * k]);
        if l.iter().all(|&v| (-0.001..=1.001).contains(&v)) {
            return l.map(gam);
        }
        k *= 0.92;
    }
    oklab_inv([c[0], 0.0, 0.0]).map(gam)
}

fn u8c(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// shirt colour (sRGB 0..1)
pub fn shirt_rgb(s: &Shirt) -> [f32; 3] {
    match s {
        Shirt::Hitam => [0.0; 3],
        Shirt::Putih => [1.0; 3],
        Shirt::Warna(c) => *c,
    }
}

/// does this shirt need a white underbase (dark and coloured shirts)?
pub fn needs_underbase(shirt: [f32; 3]) -> bool {
    oklab(shirt.map(lin))[0] < 0.85
}

// ---------------------------------------------------------------- print model

#[derive(Clone)]
struct Model {
    shirt: [f32; 3],
    base: Option<f32>,
    /// how many inks print before the base
    base_after: usize,
    inks: Vec<([f32; 3], f32)>,
    /// every film on the same screen (see `mix_dot`)
    dot_on_dot: bool,
    /// per ink: on the inverted screen (interlock)
    inv: Vec<bool>,
    /// chroma boost of the targets
    boost: f32,
    /// price per unit of ink coverage in the solver (see `cost`)
    ink_cost: f32,
    /// the white films (underbase, highlight `hl`) on their own screen: their dots fall
    /// independently of the colours' (which stay dot on dot among themselves)
    white_free: bool,
    /// index of the highlight white ink, if any
    hl: Option<usize>,
}

impl Model {
    fn new(shirt: [f32; 3], base: Option<f32>, inks: &[Ink], base_after: usize) -> Model {
        Model {
            shirt: shirt.map(lin),
            base,
            base_after: base_after.min(inks.len()),
            inks: inks.iter().map(|i| (i.rgb.map(|v| lin(v as f32 / 255.0)), i.opacity)).collect(),
            dot_on_dot: false,
            inv: vec![false; inks.len()],
            boost: 0.0,
            ink_cost: 2e-5,
            white_free: false,
            hl: None,
        }
    }

    /// what the solver aims at for a design colour (sRGB 0..1): its OKLab, boosted
    #[inline]
    fn target(&self, c: [f32; 3]) -> [f32; 3] {
        let t = oklab(c.map(lin));
        let k = 1.0 + self.boost;
        [t[0], t[1] * k, t[2] * k]
    }

    #[inline]
    fn layer(col: &mut [f32; 3], ink: [f32; 3], cov: f32, op: f32) {
        if cov <= 0.0 {
            return;
        }
        for ch in 0..3 {
            let top = op * ink[ch] + (1.0 - op) * ink[ch] * col[ch];
            col[ch] = (1.0 - cov) * col[ch] + cov * top;
        }
    }

    /// every layer in print order (the base after `base_after` inks), `on(None)` = the
    /// base's coverage, `on(Some(k))` = ink k's
    #[inline]
    fn print(&self, on: impl Fn(Option<usize>) -> f32) -> [f32; 3] {
        let mut col = self.shirt;
        for (k, &(ink, op)) in self.inks.iter().enumerate() {
            if k == self.base_after {
                if let Some(a) = self.base {
                    Model::layer(&mut col, [1.0; 3], on(None), a);
                }
            }
            Model::layer(&mut col, ink, on(Some(k)), op);
        }
        if self.base_after >= self.inks.len() {
            if let Some(a) = self.base {
                Model::layer(&mut col, [1.0; 3], on(None), a);
            }
        }
        col
    }

    /// the average printed colour (linear) of one halftone cell when every film uses
    /// the same screen (one angle, LPI and dot shape, as in simulated process): a
    /// pixel prints every film whose coverage is above the screen's threshold there,
    /// so the dots of all films are centred on each other (dot on dot) and, over a
    /// cell, the thresholds are spread evenly over 0..1. Exact for that screen, where
    /// `mix` (dots falling independently) would be wrong as soon as inks are mixed.
    /// An inverted film (interlock) prints where the threshold is above 1 - coverage.
    fn mix_dot(&self, u: f32, c: &[f32]) -> [f32; 3] {
        self.mix_dot_with(u, None, c, None)
    }

    /// `mix_dot` where the base can be fixed on / off (`fixed`, not on the screen) and
    /// one ink (`skip`, the highlight white on its own screen) left out
    fn mix_dot_with(&self, u: f32, fixed: Option<bool>, c: &[f32], skip: Option<usize>) -> [f32; 3] {
        let mut bp = [0f32; MAX_INKS + 3];
        let mut nb = 0;
        if self.base.is_some() && fixed.is_none() {
            bp[nb] = u.clamp(0.0, 1.0);
            nb += 1;
        }
        for (k, &v) in c.iter().enumerate() {
            if Some(k) == skip {
                continue;
            }
            let v = v.clamp(0.0, 1.0);
            bp[nb] = if self.inv[k] { 1.0 - v } else { v };
            nb += 1;
        }
        bp[nb] = 1.0;
        nb += 1;
        bp[..nb].sort_by(|a, b| a.total_cmp(b));
        let mut acc = [0f32; 3];
        let mut a = 0f32;
        for &b in &bp[..nb] {
            if b <= a {
                continue;
            }
            // thresholds in (a, b]: the same layers print over the whole interval
            let t = 0.5 * (a + b);
            let col = self.print(|k| match k {
                None => fixed.map_or((u > t) as u8 as f32, |on| on as u8 as f32),
                Some(k) if Some(k) == skip => 0.0,
                Some(k) if self.inv[k] => (1.0 - c[k] < t) as u8 as f32,
                Some(k) => (c[k] > t) as u8 as f32,
            });
            for ch in 0..3 {
                acc[ch] += (b - a) * col[ch];
            }
            a = b;
        }
        acc
    }

    /// the average printed colour the solver aims at: `mix_dot` for one screen,
    /// else `mix`
    #[inline]
    fn avg(&self, u: f32, c: &[f32]) -> [f32; 3] {
        if self.dot_on_dot && self.white_free {
            // white films on their own screen: the base dots fall independently of
            // the colour stack (exact average over base on / off); the highlight is
            // the last layer and a layer is linear in what is below, so it applies to
            // the average
            let stack = |on: bool| self.mix_dot_with(u, Some(on), c, self.hl);
            let (a, b) = if self.base.is_some() { (stack(true), stack(false)) } else { (stack(false), stack(false)) };
            let uu = if self.base.is_some() { u.clamp(0.0, 1.0) } else { 0.0 };
            let mut col = [0f32; 3];
            for ch in 0..3 {
                col[ch] = uu * a[ch] + (1.0 - uu) * b[ch];
            }
            if let Some(h) = self.hl {
                let (ink, op) = self.inks[h];
                Model::layer(&mut col, ink, c[h].clamp(0.0, 1.0), op);
            }
            col
        } else if self.dot_on_dot {
            self.mix_dot(u, c)
        } else {
            self.mix(u, c)
        }
    }

    /// printed colour (linear) for underbase coverage `u` and ink coverages `c`, the
    /// dots of different films falling independently of each other
    #[inline]
    fn mix(&self, u: f32, c: &[f32]) -> [f32; 3] {
        self.print(|k| match k {
            None => u,
            Some(k) => c[k],
        })
    }
}

// ---------------------------------------------------------------- inks

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// design colours this close to the shirt colour (OKLab) need no ink: the shirt shows
const NEAR_SHIRT: f32 = 0.12;

/// design colours at least this saturated (OKLab chroma) are accent candidates
const ACCENT_CHROMA: f32 = 0.06;

/// on a dark shirt no automatic ink this close to the shirt colour (OKLab)
const NEAR_DARK_INK: f32 = 0.24;

/// the ink set for a design on `shirt` (sRGB 0..1): `n` inks in print order. Light
/// to dark, black next to last, the highlight white last (dark shirts, `highlight`).
pub fn auto_inks(p: &Prepared, shirt: [f32; 3], n: usize, highlight: bool) -> Vec<Ink> {
    auto_inks_with(p, shirt, n, highlight, false)
}

/// `auto_inks`; `strong`: on a dark shirt no pastel / cream inks (see
/// `SepSettings::strong_inks`)
pub fn auto_inks_with(p: &Prepared, shirt: [f32; 3], n: usize, highlight: bool, strong: bool) -> Vec<Ink> {
    let n = n.clamp(1, MAX_INKS);
    let sh = oklab(shirt.map(lin));
    let dark = needs_underbase(shirt);
    // design colours (OKLab), at most ~150k samples, not the shirt colour itself
    let px = p.w * p.h;
    let step = (px / 150_000).max(1);
    let mut s: Vec<[f32; 3]> = (0..px)
        .step_by(step)
        .filter(|&q| p.alpha[q] >= 0.5)
        .map(|q| oklab([p.rgb[q * 3], p.rgb[q * 3 + 1], p.rgb[q * 3 + 2]].map(lin)))
        .filter(|c| dist2(*c, sh) > NEAR_SHIRT * NEAR_SHIRT)
        .collect();
    if s.is_empty() {
        return Vec::new();
    }
    // highlight white: when the design has real whites on a dark shirt
    let whiteish = |c: &[f32; 3]| c[0] > 0.86 && chroma(*c) < 0.06;
    let use_white = dark && highlight && n >= 2 && s.iter().filter(|c| whiteish(c)).count() * 200 > s.len();
    if use_white {
        s.retain(|c| !whiteish(c));
    }
    let k = if use_white { n - 1 } else { n }.min(s.len());
    // candidates: many clusters (purified), then the k inks that reach the design
    // colours best (alone, mixed with the shirt / white, or two inks mixed)
    let mut cand = kmeans_inks(&s, (k * 4).clamp(k, 32));
    // accent colours: the clusters above follow the bulk of a design, so a small but
    // saturated part (red lips in a muted green photo, little colour squares) is
    // averaged away into a grey-brown cluster. The saturated pixels are clustered on
    // their own and added as candidates (with their real share) when the clusters
    // have no saturated colour at all (a muted design); colourful designs keep theirs
    let muted = cand.iter().all(|(c, _)| chroma(*c) < ACCENT_CHROMA);
    let sat: Vec<[f32; 3]> = if muted { s.iter().copied().filter(|c| chroma(*c) >= ACCENT_CHROMA).collect() } else { Vec::new() };
    if sat.len() * 200 >= s.len() && sat.len() >= 20 {
        let share = sat.len() as f32 / s.len() as f32;
        for (c, w) in kmeans_inks(&sat, (k * 2).clamp(2, 12)) {
            if cand.iter().all(|(o, _)| dist2(*o, c) >= 0.04 * 0.04) {
                cand.push((c, w * share));
            }
        }
    }
    // on a dark shirt the shirt itself is the black: a near-black ink only darkens
    // what the base made light, a screen and ink for almost nothing (hand separations
    // leave the base out there instead)
    if dark {
        let keep: Vec<([f32; 3], f32)> = cand
            .iter()
            .copied()
            .filter(|(c, _)| !blackish(*c) && dist2(*c, sh) > NEAR_DARK_INK * NEAR_DARK_INK)
            .filter(|(c, _)| !strong || !(c[0] > 0.82 && chroma(*c) < 0.13))
            .collect();
        if !keep.is_empty() {
            cand = keep;
        }
    }
    let mut inks: Vec<[f32; 3]> = if k == 0 { Vec::new() } else { pick_inks(&cand, k, sh, dark || sh[0] > 0.9) };
    // two inks that came out (almost) the same: one is enough; an ink (almost) the
    // shirt colour prints nothing new
    let mut i = 0;
    while i < inks.len() {
        if dist2(inks[i], sh) < NEAR_SHIRT * NEAR_SHIRT || inks[..i].iter().any(|o| dist2(*o, inks[i]) < 0.06 * 0.06) {
            inks.remove(i);
        } else {
            i += 1;
        }
    }
    let mut out: Vec<Ink> = inks
        .iter()
        .map(|c| {
            let rgb = to_srgb_in_gamut(*c);
            Ink::new(rgb.map(u8c), if blackish(*c) { 0.9 } else { default_opacity() })
        })
        .collect();
    print_order(&mut out);
    if use_white {
        out.push(Ink::new([255, 255, 255], 0.85));
    }
    out
}

/// a near-black ink (OKLab)
fn blackish(c: [f32; 3]) -> bool {
    c[0] < 0.3 && chroma(c) < 0.05
}

fn ink_lab(i: &Ink) -> [f32; 3] {
    oklab(i.rgb.map(|v| lin(v as f32 / 255.0)))
}

/// light to dark, black last
fn print_order(inks: &mut [Ink]) {
    inks.sort_by(|a, b| {
        let (a, b) = (ink_lab(a), ink_lab(b));
        (blackish(a), -a[0]).partial_cmp(&(blackish(b), -b[0])).unwrap_or(std::cmp::Ordering::Equal)
    });
}

const KUNING: [u8; 3] = [255, 212, 0];
const MERAH: [u8; 3] = [226, 30, 38];
const MAGENTA: [u8; 3] = [214, 22, 128];
const BIRU: [u8; 3] = [30, 70, 180];
const CYAN: [u8; 3] = [0, 160, 222];
const HIJAU: [u8; 3] = [20, 150, 70];
const ABU: [u8; 3] = [140, 140, 140];
const HITAM: [u8; 3] = [20, 20, 20];
const EMAS: [u8; 3] = [240, 175, 30];
const ORANYE: [u8; 3] = [235, 100, 25];
const MERAH_API: [u8; 3] = [205, 30, 30];
const MARUN: [u8; 3] = [110, 20, 25];

/// the inks of a fixed set for `shirt` (sRGB 0..1), in print order, and how many of
/// them print before the underbase. Fitted to the shirt: grey on dark shirts, black on
/// light and coloured ones, the highlight white only with an underbase, no ink of
/// (about) the shirt colour.
pub fn preset_inks(set: InkSet, shirt: [f32; 3], highlight: bool) -> (Vec<Ink>, usize) {
    let sh = oklab(shirt.map(lin));
    let base = needs_underbase(shirt);
    let dark = sh[0] < 0.45;
    let mut rgb: Vec<[u8; 3]> = match set {
        InkSet::Auto => return (Vec::new(), 0),
        InkSet::Dasar | InkSet::DasarAbuDulu => vec![KUNING, MERAH, MAGENTA, BIRU, CYAN, HIJAU],
        InkSet::Hemat => vec![KUNING, MERAH, BIRU],
        InkSet::Gelap => vec![EMAS, ORANYE, MERAH_API, MARUN],
    };
    if dark && set != InkSet::Hemat {
        rgb.push(ABU);
    }
    if !dark {
        rgb.push(HITAM);
    }
    let mut inks: Vec<Ink> = rgb
        .into_iter()
        .map(|c| Ink::new(c, if c == HITAM { 0.9 } else { default_opacity() }))
        .filter(|i| dist2(ink_lab(i), sh) > NEAR_SHIRT * NEAR_SHIRT)
        .collect();
    print_order(&mut inks);
    let mut base_after = 0;
    if set == InkSet::DasarAbuDulu && base {
        if let Some(g) = inks.iter().position(|i| i.rgb == ABU) {
            let g = inks.remove(g);
            inks.insert(0, g);
            base_after = 1;
        }
    }
    if base && highlight {
        inks.push(Ink::new([255, 255, 255], 0.85));
    }
    (inks, base_after)
}

/// distance from `p` to the segment a-b (OKLab)
fn seg_dist(p: [f32; 3], a: [f32; 3], b: [f32; 3]) -> f32 {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let l2 = ab[0] * ab[0] + ab[1] * ab[1] + ab[2] * ab[2];
    let t = if l2 > 0.0 { (((p[0] - a[0]) * ab[0] + (p[1] - a[1]) * ab[1] + (p[2] - a[2]) * ab[2]) / l2).clamp(0.0, 1.0) } else { 0.0 };
    dist2(p, [a[0] + t * ab[0], a[1] + t * ab[1], a[2] + t * ab[2]]).sqrt()
}

/// how far colour `c` is from what inks `set` can print: one ink alone, its tints
/// towards the shirt and (with a white underbase / white shirt) towards white, or two
/// inks mixed
fn reach(c: [f32; 3], set: &[[f32; 3]], shirt: [f32; 3], white: bool) -> f32 {
    let wt = [1.0, 0.0, 0.0];
    let mut d = dist2(c, shirt).sqrt();
    for (i, &a) in set.iter().enumerate() {
        d = d.min(seg_dist(c, shirt, a));
        if white {
            d = d.min(seg_dist(c, a, wt));
        }
        for &b in &set[i + 1..] {
            d = d.min(seg_dist(c, a, b));
        }
    }
    d
}

/// `k` of the weighted candidates (colour, share) that reach all of them best: greedy,
/// then swaps while that helps. Small shares count more than their size (a small blue
/// part of a design still needs its ink).
fn pick_inks(cand: &[([f32; 3], f32)], k: usize, shirt: [f32; 3], white: bool) -> Vec<[f32; 3]> {
    let total = |set: &[[f32; 3]]| -> f32 { cand.iter().map(|(c, w)| w.powf(0.6) * reach(*c, set, shirt, white)).sum() };
    let mut set: Vec<[f32; 3]> = Vec::new();
    let k = k.min(cand.len());
    while set.len() < k {
        let best = cand
            .iter()
            .filter(|(c, _)| set.iter().all(|o| dist2(*o, *c) >= 0.06 * 0.06))
            .map(|(c, _)| {
                let mut s2 = set.clone();
                s2.push(*c);
                (total(&s2), *c)
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        match best {
            Some((_, c)) => set.push(c),
            None => break,
        }
    }
    for _ in 0..3 {
        let mut better = false;
        for i in 0..set.len() {
            let now = total(&set);
            let alt = cand
                .iter()
                .filter(|(c, _)| !set.contains(c))
                .map(|(c, _)| {
                    let mut s2 = set.clone();
                    s2[i] = *c;
                    (total(&s2), *c)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((v, c)) = alt {
                if v < now - 1e-6 {
                    set[i] = c;
                    better = true;
                }
            }
        }
        if !better {
            break;
        }
    }
    set
}

/// k cluster colours with their share of the samples: k-means (deterministic
/// k-means++ start), each cluster represented by its more saturated members (the
/// ink, not the average of its tints)
fn kmeans_inks(s: &[[f32; 3]], k: usize) -> Vec<([f32; 3], f32)> {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let mut cent = vec![s[(rng.next() % s.len() as u64) as usize]];
    let mut d: Vec<f32> = s.iter().map(|c| dist2(*c, cent[0])).collect();
    while cent.len() < k {
        let tot: f32 = d.iter().sum();
        if tot <= 0.0 {
            break;
        }
        let mut r = rng.unit() * tot;
        let mut pick = s.len() - 1;
        for (i, &v) in d.iter().enumerate() {
            r -= v;
            if r <= 0.0 {
                pick = i;
                break;
            }
        }
        cent.push(s[pick]);
        for (i, c) in s.iter().enumerate() {
            d[i] = d[i].min(dist2(*c, s[pick]));
        }
    }
    let mut asg = vec![0usize; s.len()];
    for _ in 0..20 {
        asg.par_iter_mut().zip(s.par_iter()).for_each(|(a, c)| {
            *a = (0..cent.len()).min_by(|&i, &j| dist2(*c, cent[i]).total_cmp(&dist2(*c, cent[j]))).unwrap_or(0);
        });
        let mut sum = vec![[0f64; 4]; cent.len()];
        for (c, &a) in s.iter().zip(&asg) {
            for ch in 0..3 {
                sum[a][ch] += c[ch] as f64;
            }
            sum[a][3] += 1.0;
        }
        let mut moved = 0f32;
        for (i, t) in sum.iter().enumerate() {
            if t[3] > 0.0 {
                let c = [(t[0] / t[3]) as f32, (t[1] / t[3]) as f32, (t[2] / t[3]) as f32];
                moved = moved.max(dist2(c, cent[i]));
                cent[i] = c;
            }
        }
        if moved < 1e-8 {
            break;
        }
    }
    // the ink of a cluster: the mean of its most saturated fifth (neutral clusters:
    // their mean)
    (0..cent.len())
        .map(|i| {
            let mut m: Vec<[f32; 3]> = s.iter().zip(&asg).filter(|(_, &a)| a == i).map(|(c, _)| *c).collect();
            let share = m.len() as f32 / s.len() as f32;
            if m.len() < 5 || chroma(cent[i]) < 0.04 {
                return (cent[i], share);
            }
            m.sort_by(|a, b| chroma(*b).total_cmp(&chroma(*a)));
            let top = &m[..(m.len() / 5).max(1)];
            let mut c = [0f32; 3];
            for t in top {
                for ch in 0..3 {
                    c[ch] += t[ch] / top.len() as f32;
                }
            }
            // keep the cluster's lightness: only the colour gets purer
            ([cent[i][0], c[1], c[2]], share)
        })
        .filter(|(_, share)| *share > 0.0)
        .collect()
}

// ---------------------------------------------------------------- solving

const GRID: usize = 21;

/// coverages (underbase first, then the inks) for every colour of a GRID^3 sRGB grid
struct Lut {
    vars: usize,
    v: Vec<f32>,
}

impl Lut {
    fn build(m: &Model) -> Lut {
        let vars = 1 + m.inks.len();
        let g = GRID;
        let slabs: Vec<Vec<f32>> = (0..g)
            .into_par_iter()
            .map(|i| {
                let mut out = Vec::with_capacity(g * g * vars);
                let mut warm = vec![0f32; vars];
                for j in 0..g {
                    for k in 0..g {
                        let srgb = [i, j, k].map(|t| t as f32 / (g - 1) as f32);
                        let x = solve(m, m.target(srgb), &warm);
                        out.extend_from_slice(&x);
                        warm = x;
                    }
                }
                out
            })
            .collect();
        Lut { vars, v: slabs.concat() }
    }

    /// coverages for an sRGB colour (trilinear)
    fn at(&self, c: [f32; 3], out: &mut [f32]) {
        let g = (GRID - 1) as f32;
        let f = c.map(|v| (v.clamp(0.0, 1.0) * g).min(g - 1e-4));
        let i = f.map(|v| v.floor() as usize);
        let t = [f[0] - i[0] as f32, f[1] - i[1] as f32, f[2] - i[2] as f32];
        out.iter_mut().for_each(|o| *o = 0.0);
        for corner in 0..8 {
            let (a, b, d) = (corner & 1, corner >> 1 & 1, corner >> 2 & 1);
            let wgt = (if a == 1 { t[0] } else { 1.0 - t[0] }) * (if b == 1 { t[1] } else { 1.0 - t[1] }) * (if d == 1 { t[2] } else { 1.0 - t[2] });
            if wgt == 0.0 {
                continue;
            }
            let q = (((i[0] + a) * GRID + i[1] + b) * GRID + i[2] + d) * self.vars;
            for (o, v) in out.iter_mut().zip(&self.v[q..q + self.vars]) {
                *o += wgt * v;
            }
        }
    }
}

/// coverages for the colours that are really in the design, 7 bits per channel (steps
/// of 2/255, finer than the ~30 tone levels of a 55 LPI cell): the coarse grid value
/// refined for each of those colours, used as it is. Interpolating solutions is not
/// safe with one screen: the order of the coverages decides which ink shows (dot on
/// dot), so between two good solutions that order differently an interpolated one can
/// open a ring of bare underbase (a light band in a gradient).
struct Exact {
    vars: usize,
    /// per 7-bit colour: index into `v` + 1 (0 = not in the design)
    slot: Vec<u32>,
    v: Vec<f32>,
}

impl Exact {
    fn key(c: [f32; 3]) -> usize {
        let q = |v: f32| ((v.clamp(0.0, 1.0) * 127.0).round() as usize).min(127);
        (q(c[0]) << 14) | (q(c[1]) << 7) | q(c[2])
    }

    fn build(p: &Prepared, alpha: &[f32], m: &Model, lut: &Lut) -> Exact {
        let vars = lut.vars;
        let mut used = vec![false; 1 << 21];
        for q in 0..p.w * p.h {
            if alpha[q] > 0.0 {
                used[Exact::key([p.rgb[q * 3], p.rgb[q * 3 + 1], p.rgb[q * 3 + 2]])] = true;
            }
        }
        let keys: Vec<usize> = (0..1usize << 21).filter(|&k| used[k]).collect();
        let sols: Vec<Vec<f32>> = keys
            .par_iter()
            .map(|&k| {
                let c = [(k >> 14) & 127, (k >> 7) & 127, k & 127].map(|v| v as f32 / 127.0);
                let mut x = vec![0f32; vars];
                lut.at(c, &mut x);
                refine(m, m.target(c), x)
            })
            .collect();
        let mut slot = vec![0u32; 1 << 21];
        for (i, &k) in keys.iter().enumerate() {
            slot[k] = i as u32 + 1;
        }
        Exact { vars, slot, v: sols.concat() }
    }

    fn at(&self, c: [f32; 3], out: &mut [f32]) {
        let s = self.slot[Exact::key(c)] as usize;
        if s == 0 {
            out.iter_mut().for_each(|o| *o = 0.0);
        } else {
            out.copy_from_slice(&self.v[(s - 1) * self.vars..s * self.vars]);
        }
    }
}

/// a few coordinate-descent sweeps from `x` (see `solve`)
fn refine(m: &Model, t: [f32; 3], mut x: Vec<f32>) -> Vec<f32> {
    let has_base = m.base.is_some();
    if !has_base {
        x[0] = 0.0;
    }
    let mut f = cost(m, &x, t);
    for _ in 0..4 {
        let f0 = f;
        for v in 0..x.len() {
            if v == 0 && !has_base {
                continue;
            }
            let old = x[v];
            let mut eval = |val: f32| {
                x[v] = val;
                let c = cost(m, &x, t);
                x[v] = old;
                c
            };
            let (mut a, mut b) = (0f32, 1f32);
            const R: f32 = 0.618_034;
            let (mut c1, mut c2) = (b - R * (b - a), a + R * (b - a));
            let (mut f1, mut f2) = (eval(c1), eval(c2));
            for _ in 0..12 {
                if f1 < f2 {
                    b = c2;
                    c2 = c1;
                    f2 = f1;
                    c1 = b - R * (b - a);
                    f1 = eval(c1);
                } else {
                    a = c1;
                    c1 = c2;
                    f1 = f2;
                    c2 = a + R * (b - a);
                    f2 = eval(c2);
                }
            }
            let mid = 0.5 * (a + b);
            let mut best = (f, old);
            for cand in [mid, 0.0, 1.0] {
                let c = eval(cand);
                if c < best.0 {
                    best = (c, cand);
                }
            }
            x[v] = best.1;
            f = best.0;
        }
        if f0 - f < 1e-7 {
            break;
        }
    }
    x
}

/// cost of coverages `x` (underbase, inks) for target `t` (OKLab)
#[inline]
fn cost(m: &Model, x: &[f32], t: [f32; 3]) -> f32 {
    let p = oklab(m.avg(x[0], &x[1..]));
    // a tiny price for ink keeps unneeded inks (and base) at zero
    let ink: f32 = x[1..].iter().sum::<f32>() + 0.5 * x[0];
    dist2(p, t) + m.ink_cost * ink
}

/// coverages whose print is closest to `t`: coordinate descent with golden section
/// line searches, from several starts (nothing, the previous grid colour, the best
/// single ink on full base)
fn solve(m: &Model, t: [f32; 3], warm: &[f32]) -> Vec<f32> {
    let vars = 1 + m.inks.len();
    let has_base = m.base.is_some();
    let mut starts = vec![vec![0f32; vars], warm.to_vec()];
    for k in 0..m.inks.len() {
        let mut x = vec![0f32; vars];
        x[0] = if has_base { 1.0 } else { 0.0 };
        x[1 + k] = 1.0;
        starts.push(x);
    }
    // keep the three most promising starts
    starts.sort_by(|a, b| cost(m, a, t).total_cmp(&cost(m, b, t)));
    starts.truncate(3);
    let mut best: Option<(f32, Vec<f32>)> = None;
    for mut x in starts {
        if !has_base {
            x[0] = 0.0;
        }
        let mut f = cost(m, &x, t);
        for _ in 0..10 {
            let f0 = f;
            for v in 0..vars {
                if v == 0 && !has_base {
                    continue;
                }
                let line = |val: f32, x: &mut Vec<f32>| {
                    let old = x[v];
                    x[v] = val;
                    let c = cost(m, x, t);
                    x[v] = old;
                    c
                };
                // golden section on [0, 1], then the ends
                let (mut a, mut b) = (0f32, 1f32);
                const R: f32 = 0.618_034;
                let mut c1 = b - R * (b - a);
                let mut c2 = a + R * (b - a);
                let mut f1 = line(c1, &mut x);
                let mut f2 = line(c2, &mut x);
                for _ in 0..16 {
                    if f1 < f2 {
                        b = c2;
                        c2 = c1;
                        f2 = f1;
                        c1 = b - R * (b - a);
                        f1 = line(c1, &mut x);
                    } else {
                        a = c1;
                        c1 = c2;
                        f1 = f2;
                        c2 = a + R * (b - a);
                        f2 = line(c2, &mut x);
                    }
                }
                let mid = 0.5 * (a + b);
                let mut cand = [(line(mid, &mut x), mid), (line(0.0, &mut x), 0.0), (line(1.0, &mut x), 1.0), (f, x[v])];
                cand.sort_by(|p, q| p.0.total_cmp(&q.0));
                x[v] = cand[0].1;
                f = cand[0].0;
            }
            if f0 - f < 1e-7 {
                break;
            }
        }
        if best.as_ref().is_none_or(|(bf, _)| f < *bf) {
            best = Some((f, x));
        }
    }
    best.map(|b| b.1).unwrap_or_else(|| vec![0.0; vars])
}

// ---------------------------------------------------------------- separation

/// rosette: screen angle of ink film k relative to the underbase (none on the base's
/// angle; 30 / 60 / 15 / 45 / 75 first, as in four-colour rosettes)
const ROSET: [f64; 12] = [30.0, 60.0, 15.0, 45.0, 75.0, 22.5, 52.5, 7.5, 37.5, 67.5, 82.5, 0.0];

/// separate the prepared design (at print size) into films. `s` gives the shirt, DPI,
/// textures (pudar / lubang layers act on every film) and "Lindungi teks" (the
/// lettering prints in the nearest ink, solid).
pub fn separate(p: &Prepared, s: &Settings, sep: &SepSettings) -> Separation {
    let (w, h) = (p.w, p.h);
    let shirt = shirt_rgb(&s.shirt);
    let base = sep.underbase.unwrap_or_else(|| needs_underbase(shirt)).then_some(sep.base_opacity);
    let (inks, base_after) = if !sep.inks.is_empty() {
        (sep.inks.clone(), sep.base_after.min(sep.inks.len()))
    } else if sep.ink_set == InkSet::Auto {
        (auto_inks_with(p, shirt, sep.colors, sep.highlight, sep.strong_inks), 0)
    } else {
        preset_inks(sep.ink_set, shirt, sep.highlight)
    };
    let dpi = s.dpi;
    // design opacity, with the texture fade
    let alpha: Vec<f32> = match crate::distress::fade(s, w, h, 0, 0) {
        Some(f) => p.alpha.iter().zip(&f).map(|(a, b)| a * b).collect(),
        None => p.alpha.clone(),
    };
    let solid = p.solid.as_ref().filter(|_| s.protect_text);
    let k = mindot_px(dpi).max(1);
    // coverage per film (underbase first, then the inks used), 0..1 as u8; inks that
    // print (almost) nothing are dropped and the others solved again without them
    let mut keep: Vec<usize> = (0..inks.len()).collect();
    let (m, mut cov) = loop {
        let used: Vec<Ink> = keep.iter().map(|&i| inks[i]).collect();
        let m = model_of(shirt, base, &used, keep.iter().filter(|&&i| i < base_after).count(), sep);
        let cov = coverages(p, &alpha, solid, &m, &used, sep, k);
        if sep.drop_below > 0.0 && keep.len() == inks.len() && keep.len() > 1 {
            // dots smaller than the printable minimum are cleaned from the film anyway
            let min_cov = if sep.method == Method::Simulasi { (minarea_px(dpi) as f64 / (dpi / sep.lpi).powi(2) * 255.0).min(128.0) as u8 } else { 0 };
            let area = alpha.par_iter().map(|&a| a as f64).sum::<f64>().max(1.0);
            let share: Vec<f64> = cov[1..].par_iter().map(|c| c.iter().filter(|&&v| v >= min_cov).map(|&v| v as u64).sum::<u64>() as f64 / 255.0 / area).collect();
            let kept: Vec<usize> = keep.iter().zip(&share).filter(|(_, &u)| u * 100.0 >= sep.drop_below as f64).map(|(&i, _)| i).collect();
            if !kept.is_empty() && kept.len() < keep.len() {
                keep = kept;
                continue;
            }
        }
        break (m, cov);
    };
    let used: Vec<Ink> = keep.iter().map(|&i| inks[i]).collect();
    let ni = used.len();
    // lettering that prints solid: in the text mask and close enough to one ink (as in
    // `coverages`), the rest of the mask keeps its dots
    let flat: Option<Vec<bool>> = solid.map(|sm| {
        (0..w * h)
            .into_par_iter()
            .map_init(
                || vec![0f32; 1 + ni],
                |x, q| sm[q] >= 0.5 && nearest(&m, [p.rgb[q * 3], p.rgb[q * 3 + 1], p.rgb[q * 3 + 2]], x) <= SOLID_TEXT_MAX,
            )
            .collect()
    });
    let is_hl = |i: usize| base.is_some() && i + 1 == ni && used[i].rgb == [255, 255, 255];
    if sep.method == Method::Simulasi && sep.max_dot < 1.0 {
        // the white films only (underbase and highlight): a hole in the base under an
        // ink just darkens that spot a little, while a hole in an ink over the base shows
        // a white speck (a red logo with texture would get white dots)
        cap_gradients(&mut cov, p, &alpha, flat.as_deref(), w, h, dpi, sep, |f| (f == 0 || is_hl(f - 1)).then_some(sep.max_dot));
    }
    // expand: the colour films a little wider (trapping); never the highlight white
    if sep.expand_px > 0 {
        let d = 2 * sep.expand_px + 1;
        cov[1..].par_iter_mut().enumerate().filter(|(i, _)| !is_hl(*i)).for_each(|(_, c)| *c = max_filter(c, w, h, d));
    }
    // underbase choke: never wider than the inks on it
    let r = ((sep.choke_mm / 25.4 * dpi).round() as usize).min(64);
    if base.is_some() && r > 0 {
        cov[0] = min_filter(&cov[0], w, h, 2 * r + 1);
    }
    // films in print order: the inks before the base, the base, the rest
    let holes = crate::distress::holes(s, w, h, 0, 0);
    let ba = m.base_after;
    let order: Vec<usize> = (1..=ba).chain(base.map(|_| 0)).chain(ba + 1..=ni).collect();
    let films: Vec<Film> = order
        .into_iter()
        .map(|f| {
            let c = &cov[f];
            let inverted = f > 0 && m.inv[f - 1];
            let white = f == 0 || is_hl(f - 1);
            let angle = if f > 0 && sep.dot_style == DotStyle::Roset {
                (sep.angle + ROSET[(f - 1) % ROSET.len()]).rem_euclid(180.0)
            } else if white && m.white_free {
                sep.white_angle.unwrap_or(sep.angle).rem_euclid(180.0)
            } else {
                sep.angle
            };
            let mut bits: Vec<bool> = match sep.method {
                Method::Simulasi => {
                    // an inverted film prints where the threshold is above 1 - coverage
                    let t: Vec<f32> = c.iter().map(|&v| if inverted { 1.0 - v as f32 / 255.0 } else { v as f32 / 255.0 }).collect();
                    let mut b = channel_at(&t, w, h, 0, 0, dpi, sep.lpi, angle, sep.shape, choose_grid(dpi, sep.lpi, angle), false);
                    if inverted {
                        b.par_iter_mut().for_each(|o| *o = !*o);
                    }
                    // solid text: the coverage itself (0 or 1), not dots
                    if let Some(fm) = &flat {
                        b.par_iter_mut().enumerate().for_each(|(q, o)| {
                            if fm[q] {
                                *o = c[q] >= 128;
                            }
                        });
                    }
                    b
                }
                _ => c.iter().map(|&v| v >= 128).collect(),
            };
            if let Some(hm) = &holes {
                bits.par_iter_mut().zip(hm.par_iter()).for_each(|(b, &hole)| *b &= !hole);
            }
            clean_bits(&mut bits, w, h, dpi);
            let (role, ink, src) = if f == 0 {
                (Role::Underbase, Ink::new([255, 255, 255], sep.base_opacity), None)
            } else {
                (if is_hl(f - 1) { Role::Highlight } else { Role::Ink }, used[f - 1], Some(keep[f - 1]))
            };
            let angle = (sep.method == Method::Simulasi).then_some(angle);
            Film { role, ink, src, angle, inverted, bits: Bits::from_bools(&bits, w, h) }
        })
        .collect();
    let preview = simulate(&m, &films, w, h);
    let delta_e = delta_e(p, &alpha, shirt, &preview, w, h, ((2.0 * dpi / sep.lpi).round() as usize).max(2));
    Separation { w, h, inks, base_after, films, preview, delta_e }
}

/// the print model for the inks `used` (print order, `base_after` of them before the
/// base) and the separation's method and dot style
fn model_of(shirt: [f32; 3], base: Option<f32>, used: &[Ink], base_after: usize, sep: &SepSettings) -> Model {
    let mut m = Model::new(shirt, base, used, base_after);
    m.boost = sep.boost;
    // hemat tinta: up to ~0.8 dE (OKLab x 100) of colour traded for a full layer of ink
    m.ink_cost = 2e-5 + sep.ink_saver * 2.4e-4;
    m.hl = (base.is_some() && used.last().is_some_and(|i| i.rgb == [255, 255, 255])).then(|| used.len() - 1);
    m.white_free = sep.method == Method::Simulasi
        && sep.dot_style != DotStyle::Roset
        && sep.white_angle.is_some_and(|a| (a - sep.angle).rem_euclid(180.0).min((sep.angle - a).rem_euclid(180.0)) > 0.5);
    if sep.method == Method::Simulasi {
        match sep.dot_style {
            DotStyle::SatuSudut => m.dot_on_dot = true,
            DotStyle::Interlock => {
                m.dot_on_dot = true;
                m.inv = interlock(used);
            }
            // other angles: the dots of different films fall independently
            DotStyle::Roset => {}
        }
    }
    m
}

/// interlock: the colourful inks in hue order, every second one on the inverted screen
/// (hue neighbours mix side by side, like the colours between them in the design);
/// neutrals (white, grey, black) stay on the normal screen, dot on dot
fn interlock(inks: &[Ink]) -> Vec<bool> {
    let mut hue: Vec<(f32, usize)> = inks
        .iter()
        .enumerate()
        .filter_map(|(i, ink)| {
            let c = ink_lab(ink);
            (chroma(c) >= 0.05).then(|| (c[2].atan2(c[1]), i))
        })
        .collect();
    hue.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut inv = vec![false; inks.len()];
    for (j, &(_, i)) in hue.iter().enumerate() {
        inv[i] = j % 2 == 1;
    }
    inv
}

/// coverage per film (underbase first, then the inks of `m`), 0..255
fn coverages(p: &Prepared, alpha: &[f32], solid: Option<&Vec<f32>>, m: &Model, used: &[Ink], sep: &SepSettings, k: usize) -> Vec<Vec<u8>> {
    let (w, h) = (p.w, p.h);
    let ni = used.len();
    let mut cov: Vec<Vec<u8>> = vec![vec![0u8; w * h]; 1 + ni];
    let boost: Vec<f32> = used.iter().map(|i| 1.0 + i.boost).collect();
    let rows: Vec<Vec<u8>> = match sep.method {
        Method::Index => {
            index_cov(p, alpha, m, k, &mut cov);
            return cov;
        }
        Method::Simulasi => {
            let lut = Lut::build(m);
            let exact = Exact::build(p, alpha, m, &lut);
            (0..h)
                .into_par_iter()
                .map(|y| {
                    let mut out = vec![0u8; w * (1 + ni)];
                    let mut x_ = vec![0f32; 1 + ni];
                    for x in 0..w {
                        let q = y * w + x;
                        let a = alpha[q];
                        if a <= 0.0 {
                            continue;
                        }
                        let c = [p.rgb[q * 3], p.rgb[q * 3 + 1], p.rgb[q * 3 + 2]];
                        let flat = solid.is_some_and(|sm| sm[q] >= 0.5) && nearest(m, c, &mut x_) <= SOLID_TEXT_MAX;
                        if flat {
                            if a < 0.5 {
                                x_.iter_mut().for_each(|v| *v = 0.0);
                            }
                        } else {
                            exact.at(c, &mut x_);
                            x_.iter_mut().for_each(|v| *v *= a);
                            // "boost" by hand: this ink denser
                            for (v, b) in x_[1..].iter_mut().zip(&boost) {
                                *v = (*v * b).min(1.0);
                            }
                        }
                        for (f, v) in x_.iter().enumerate() {
                            out[f * w + x] = u8c(*v);
                        }
                    }
                    out
                })
                .collect()
        }
        Method::Spot => (0..h)
            .into_par_iter()
            .map(|y| {
                let mut out = vec![0u8; w * (1 + ni)];
                let mut x_ = vec![0f32; 1 + ni];
                for x in 0..w {
                    let q = y * w + x;
                    if alpha[q] < 0.5 {
                        continue;
                    }
                    nearest(m, [p.rgb[q * 3], p.rgb[q * 3 + 1], p.rgb[q * 3 + 2]], &mut x_);
                    for (f, v) in x_.iter().enumerate() {
                        out[f * w + x] = u8c(*v);
                    }
                }
                out
            })
            .collect(),
    };
    for (y, r) in rows.iter().enumerate() {
        for (f, c) in cov.iter_mut().enumerate() {
            c[y * w..(y + 1) * w].copy_from_slice(&r[f * w..(f + 1) * w]);
        }
    }
    cov
}

/// lettering prints solid in one ink only when one ink (or nothing) is this close to
/// its colour (OKLab); farther colours (a soft cloud the text finder took for text,
/// a gradient letter) keep their dots instead of turning into a flat wrong colour
const SOLID_TEXT_MAX: f32 = 0.09;

/// the single ink (on full underbase) or nothing whose print is nearest to `c` (sRGB);
/// returns that distance (OKLab)
fn nearest(m: &Model, c: [f32; 3], out: &mut [f32]) -> f32 {
    let t = m.target(c);
    out.iter_mut().for_each(|v| *v = 0.0);
    let mut best = dist2(oklab(m.shirt), t);
    let mut pick = None;
    let mut x = vec![0f32; out.len()];
    for k in 0..m.inks.len() {
        x.iter_mut().for_each(|v| *v = 0.0);
        x[0] = if m.base.is_some() { 1.0 } else { 0.0 };
        x[1 + k] = 1.0;
        let d = dist2(oklab(m.mix(x[0], &x[1..])), t);
        if d < best {
            best = d;
            pick = Some(k);
        }
    }
    if let Some(k) = pick {
        out[0] = if m.base.is_some() { 1.0 } else { 0.0 };
        out[1 + k] = 1.0;
    }
    best.sqrt()
}

/// index separation: k x k blocks, each gets one ink (on base) or nothing, by error
/// diffusion (Floyd-Steinberg, serpentine) in OKLab
fn index_cov(p: &Prepared, alpha: &[f32], m: &Model, k: usize, cov: &mut [Vec<u8>]) {
    let (w, h) = (p.w, p.h);
    let (bw, bh) = (w.div_ceil(k), h.div_ceil(k));
    // the candidates: nothing (the shirt) and every ink on full base
    let mut cand: Vec<([f32; 3], Option<usize>)> = vec![(oklab(m.shirt), None)];
    for i in 0..m.inks.len() {
        let mut x = vec![0f32; m.inks.len()];
        x[i] = 1.0;
        cand.push((oklab(m.mix(if m.base.is_some() { 1.0 } else { 0.0 }, &x)), Some(i)));
    }
    // block targets: the design over the shirt (linear mix), as OKLab
    let mut tgt = vec![[0f32; 3]; bw * bh];
    tgt.par_iter_mut().enumerate().for_each(|(b, t)| {
        let (bx, by) = (b % bw, b / bw);
        let mut acc = [0f32; 3];
        let mut nn = 0f32;
        for y in by * k..((by + 1) * k).min(h) {
            for x in bx * k..((bx + 1) * k).min(w) {
                let q = y * w + x;
                let a = alpha[q];
                for ch in 0..3 {
                    acc[ch] += a * lin(p.rgb[q * 3 + ch]) + (1.0 - a) * m.shirt[ch];
                }
                nn += 1.0;
            }
        }
        let o = oklab(acc.map(|v| v / nn.max(1.0)));
        *t = [o[0], o[1] * (1.0 + m.boost), o[2] * (1.0 + m.boost)];
    });
    let mut err = vec![[0f32; 3]; bw * 2];
    let mut pick = vec![None; bw * bh];
    for by in 0..bh {
        let (cur, nxt) = err.split_at_mut(bw);
        cur.copy_from_slice(nxt);
        nxt.iter_mut().for_each(|e| *e = [0.0; 3]);
        let ltr = by % 2 == 0;
        for i in 0..bw {
            let bx = if ltr { i } else { bw - 1 - i };
            let b = by * bw + bx;
            let want = [0, 1, 2].map(|ch| tgt[b][ch] + cur[bx][ch].clamp(-0.2, 0.2));
            let (c, ink) = cand.iter().min_by(|a, b2| dist2(a.0, want).total_cmp(&dist2(b2.0, want))).copied().unwrap();
            pick[b] = ink;
            let e = [want[0] - c[0], want[1] - c[1], want[2] - c[2]];
            let fwd = |x: usize| if ltr { x.checked_add(1).filter(|&v| v < bw) } else { x.checked_sub(1) };
            let back = |x: usize| if ltr { x.checked_sub(1) } else { x.checked_add(1).filter(|&v| v < bw) };
            for ch in 0..3 {
                if let Some(f) = fwd(bx) {
                    cur[f][ch] += e[ch] * 7.0 / 16.0;
                    nxt[f][ch] += e[ch] * 1.0 / 16.0;
                }
                if let Some(bk) = back(bx) {
                    nxt[bk][ch] += e[ch] * 3.0 / 16.0;
                }
                nxt[bx][ch] += e[ch] * 5.0 / 16.0;
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let q = y * w + x;
            if alpha[q] < 0.25 {
                continue;
            }
            if let Some(i) = pick[(y / k) * bw + x / k] {
                if m.base.is_some() {
                    cov[0][q] = 255;
                }
                cov[1 + i][q] = 255;
            }
        }
    }
}

/// "titik maksimum": a white film above `max_dot` where the design colour varies (a
/// light gradient the inks cannot quite reach, clipped into a flat solid patch; above
/// ~90 % the holes are too small to print and get filled anyway) prints `max_dot`
/// instead, so the gradient keeps its dots. A solid area whose design colour is flat
/// (lettering, a white outline) stays solid: the variation is measured over the full
/// pixels of that film within two halftone cells, so soft edges do not count. Solid
/// lettering (`flat`) is left alone; `cap_of(film)` = the cap, None = not capped.
#[allow(clippy::too_many_arguments)]
fn cap_gradients(cov: &mut [Vec<u8>], p: &Prepared, alpha: &[f32], flat: Option<&[bool]>, w: usize, h: usize, dpi: f64, sep: &SepSettings, cap_of: impl Fn(usize) -> Option<f32> + Sync) {
    // statistics per block of about one halftone cell; a pixel is judged over the 3 x 3
    // blocks around it (about two cells). Memory: a few floats per block, not per pixel
    // (a print at 600 dpi has tens of millions of pixels)
    let bs = ((dpi / sep.lpi).round() as usize).max(2);
    let (bw, bh) = (w.div_ceil(bs), h.div_ceil(bs));
    let lum = |q: usize| oklab([p.rgb[q * 3], p.rgb[q * 3 + 1], p.rgb[q * 3 + 2]].map(lin))[0];
    for (f, c) in cov.iter_mut().enumerate() {
        let Some(cap) = cap_of(f) else { continue };
        let cap = (cap * 255.0).round() as u8;
        if !c.par_iter().any(|&v| v > cap) {
            continue;
        }
        // per block: number, sum and sum of squares of the design lightness over the
        // (nearly) full pixels of this film; the soft edge of a flat letter (between the
        // cap and full) is not a gradient
        let stats: Vec<[f32; 3]> = (0..bw * bh)
            .into_par_iter()
            .map(|b| {
                let (bx, by) = (b % bw, b / bw);
                let mut acc = [0f32; 3];
                for y in by * bs..((by + 1) * bs).min(h) {
                    for x in bx * bs..((bx + 1) * bs).min(w) {
                        let q = y * w + x;
                        if c[q] >= 250 && alpha[q] > 0.0 {
                            let l = lum(q);
                            acc[0] += 1.0;
                            acc[1] += l;
                            acc[2] += l * l;
                        }
                    }
                }
                acc
            })
            .collect();
        c.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            let by = y / bs;
            for (x, v) in row.iter_mut().enumerate() {
                let q = y * w + x;
                if *v <= cap || flat.is_some_and(|fm| fm[q]) {
                    continue;
                }
                let bx = x / bs;
                let mut acc = [0f32; 3];
                for yy in by.saturating_sub(1)..(by + 2).min(bh) {
                    for xx in bx.saturating_sub(1)..(bx + 2).min(bw) {
                        let s = stats[yy * bw + xx];
                        acc[0] += s[0];
                        acc[1] += s[1];
                        acc[2] += s[2];
                    }
                }
                if acc[0] < 0.5 {
                    // no full pixel near: a lone near-full spot inside a gradient
                    *v = cap;
                    continue;
                }
                let (e1, e2) = (acc[1] / acc[0], acc[2] / acc[0]);
                // standard deviation of the design lightness: about 1 dE = a gradient,
                // below = a flat colour
                if (e2 - e1 * e1).max(0.0).sqrt() > 0.01 {
                    *v = cap;
                }
            }
        });
    }
}

/// ink islands and holes smaller than the printable minimum (0.15 mm) removed / filled
fn clean_bits(b: &mut [bool], w: usize, h: usize, dpi: f64) {
    let amin = minarea_px(dpi);
    let (lab, sizes) = label(b, w, h);
    b.par_iter_mut().zip(lab.par_iter()).for_each(|(v, &l)| {
        if *v && l != 0 && sizes[l as usize] < amin {
            *v = false;
        }
    });
    let holes: Vec<bool> = b.iter().map(|v| !v).collect();
    let (hl, hs) = label(&holes, w, h);
    b.par_iter_mut().zip(hl.par_iter()).for_each(|(v, &l)| {
        if !*v && l != 0 && hs[l as usize] < amin {
            *v = true;
        }
    });
}

/// the print simulated from the films (RGBA8; alpha 0 where nothing prints)
fn simulate(m: &Model, films: &[Film], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h * 4];
    // the ink films in the model's order (print order without the base)
    let base = films.iter().find(|f| f.role == Role::Underbase);
    let tops: Vec<&Film> = films.iter().filter(|f| f.role != Role::Underbase).collect();
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, row)| {
        let mut c = vec![0f32; tops.len()];
        for x in 0..w {
            let q = y * w + x;
            let u = if base.is_some_and(|b| b.bits.get(q)) { 1.0 } else { 0.0 };
            let mut any = u > 0.0;
            for (i, f) in tops.iter().enumerate() {
                c[i] = if f.bits.get(q) { 1.0 } else { 0.0 };
                any |= c[i] > 0.0;
            }
            if !any {
                continue;
            }
            let col = m.mix(u, &c);
            let o = &mut row[x * 4..x * 4 + 4];
            for ch in 0..3 {
                o[ch] = u8c(gam(col[ch]));
            }
            o[3] = 255;
        }
    });
    out
}

/// mean OKLab distance x 100 between the design and the simulated print over the shirt,
/// both averaged over `cell` x `cell` blocks (two halftone cells: what the eye sees
/// from a little distance), over
/// the blocks where the design prints
fn delta_e(p: &Prepared, alpha: &[f32], shirt: [f32; 3], sim: &[u8], w: usize, h: usize, cell: usize) -> f32 {
    let sh = shirt.map(lin);
    let (bw, bh) = (w.div_ceil(cell), h.div_ceil(cell));
    let r: Vec<(f64, f64)> = (0..bh)
        .into_par_iter()
        .map(|by| {
            let (mut sum, mut cnt) = (0f64, 0f64);
            for bx in 0..bw {
                let (mut a, mut b, mut cov, mut nn) = ([0f32; 3], [0f32; 3], 0f32, 0f32);
                for y in by * cell..((by + 1) * cell).min(h) {
                    for x in bx * cell..((bx + 1) * cell).min(w) {
                        let q = y * w + x;
                        let al = alpha[q];
                        cov += al;
                        nn += 1.0;
                        for ch in 0..3 {
                            a[ch] += al * lin(p.rgb[q * 3 + ch]) + (1.0 - al) * sh[ch];
                            b[ch] += if sim[q * 4 + 3] == 255 { lin(sim[q * 4 + ch] as f32 / 255.0) } else { sh[ch] };
                        }
                    }
                }
                if cov / nn < 0.5 {
                    continue;
                }
                let d = dist2(oklab(a.map(|v| v / nn)), oklab(b.map(|v| v / nn))).sqrt();
                sum += d as f64;
                cnt += 1.0;
            }
            (sum, cnt)
        })
        .collect();
    let (s, c) = r.iter().fold((0.0, 0.0), |acc, v| (acc.0 + v.0, acc.1 + v.1));
    if c == 0.0 {
        0.0
    } else {
        (s / c * 100.0) as f32
    }
}

// ---------------------------------------------------------------- film output

/// one film as a grayscale PNG: black where the ink prints, white elsewhere, with a
/// `margin_mm` border holding registration marks (top, bottom, left, right) and the
/// label (film number and ink) at the top left
pub fn save_film(film: &Film, dpi: f64, margin_mm: f64, label_text: &str, path: &std::path::Path) -> anyhow::Result<()> {
    let (w, h) = (film.bits.w, film.bits.h);
    let pad = ((margin_mm / 25.4 * dpi).round() as usize).max(0);
    let (fw, fh) = (w + 2 * pad, h + 2 * pad);
    // marks: a cross in a circle, 8 mm across, line 0.25 mm, centred in the margin
    let rr = ((4.0 / 25.4 * dpi) as usize).max(4);
    let lw = ((0.25 / 25.4 * dpi).round() as usize).max(1);
    let marks: Vec<(usize, usize)> = if pad >= rr + 2 {
        vec![(fw / 2, pad / 2), (fw / 2, fh - pad / 2 - 1), (pad / 2, fh / 2), (fw - pad / 2 - 1, fh / 2)]
    } else {
        Vec::new()
    };
    let scale = ((dpi / 100.0).round() as usize).max(2);
    let label: Vec<[u8; 7]> = label_text.chars().map(crate::testsheet::glyph).collect();
    let (lx, ly) = (pad.max(4) / 4 + 2, pad.saturating_sub(7 * scale) / 2);
    let is_mark = |x: usize, y: usize| {
        marks.iter().any(|&(cx, cy)| {
            let (dx, dy) = (x as i64 - cx as i64, y as i64 - cy as i64);
            let d = ((dx * dx + dy * dy) as f64).sqrt();
            (d - rr as f64).abs() <= lw as f64 / 2.0 + 0.5 || (dx.unsigned_abs() as usize <= lw / 2 && dy.unsigned_abs() as usize <= rr + rr / 3) || (dy.unsigned_abs() as usize <= lw / 2 && dx.unsigned_abs() as usize <= rr + rr / 3)
        })
    };
    let is_label = |x: usize, y: usize| {
        if pad < 7 * scale + 2 || y < ly || y >= ly + 7 * scale || x < lx {
            return false;
        }
        let (gx, gy) = ((x - lx) / scale, (y - ly) / scale);
        let (ci, cx) = (gx / 6, gx % 6);
        ci < label.len() && cx < 5 && label[ci][gy] & (1 << (4 - cx)) != 0
    };
    crate::io::write_png_gray_rows(fw, fh, dpi, path, |y0, y1| {
        let mut out = vec![255u8; (y1 - y0) * fw];
        for y in y0..y1 {
            for x in 0..fw {
                let ink = if x >= pad && y >= pad && x < pad + w && y < pad + h {
                    film.bits.get((y - pad) * w + (x - pad))
                } else {
                    is_mark(x, y) || is_label(x, y)
                };
                if ink {
                    out[(y - y0) * fw + x] = 0;
                }
            }
        }
        Ok(out)
    })
}
