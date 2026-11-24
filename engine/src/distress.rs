//! Vintage texture layers (`Settings.distress`): a texture image (black = worn ink,
//! white = ink stays) knocks holes into the printed ink (*lubang*), lowers the tone
//! before screening (*pudar*, worn areas become smaller printable dots), or both
//! (*campur*: holes where the texture is dark, a fade in its grey parts).
//!
//! Textures come from the built-in library (`lib:<file name>`: every image in the
//! repository folder `textures/`, embedded at build time, see `crate::library`) or
//! from a file of the user (it stays on the user's computer).
//!
//! Placement: by default the texture COVERS the whole print (aspect ratio kept,
//! centred), then zoom, rotation, mirror and an offset move it; outside the texture
//! it repeats mirrored (no seams). Every value is a pure function of the ABSOLUTE
//! print pixel (x + ox, y + oy), the print size, the dpi and the layer, with basic
//! IEEE operations only, so band / window renders equal the full render and the
//! pixels are the same on every platform. The neighbourhood operations (mask close /
//! open, block snap and the dot-aware vote) are covered by [`halo_extra`].
//!
//! Print rules kept: the hole mask is closed then opened with a `mindot_px` square
//! (mask only, never the ink), halftone dots are kept or dropped whole by a majority
//! vote under the mask, solid ink is cut exactly, and `dtf::clean` runs last.

use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::dtf::mindot_px;
use crate::render::{Pattern, Settings};

// ------------------------------------------------------------------ schema

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// texture image: built-in library (`lib:<name>`) or a file of the user
    Gambar,
    /// procedural looks of an earlier version (retak, bintik, gores, pudar): ignored
    #[serde(other)]
    Lama,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Gambar => "gambar",
            Kind::Lama => "lama",
        }
    }

    pub fn parse(s: &str) -> Option<Kind> {
        match s.trim().to_ascii_lowercase().as_str() {
            "gambar" | "tekstur" | "texture" => Some(Kind::Gambar),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// knock holes into the ink where the texture is dark
    Lubang,
    /// reduce the tone before screening by the texture darkness
    Pudar,
    /// both: holes where dark, fade in the grey parts
    Campur,
}

impl Mode {
    pub fn holes(self) -> bool {
        matches!(self, Mode::Lubang | Mode::Campur)
    }
    pub fn fades(self) -> bool {
        matches!(self, Mode::Pudar | Mode::Campur)
    }
    pub fn name(self) -> &'static str {
        match self {
            Mode::Lubang => "lubang",
            Mode::Pudar => "pudar",
            Mode::Campur => "campur",
        }
    }
}

/// One texture layer as the UI / job JSON sends it. Missing (or unreadable) fields
/// take the defaults of [`Layer::params`]; numbers are clamped there.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient::mode")]
    pub mode: Option<Mode>,
    /// 0..1 strength of the holes: pixels darker than 1 - threshold are removed
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient::num32")]
    pub threshold: Option<f32>,
    /// 0..1 fade strength (pudar / campur)
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient::num32")]
    pub amount: Option<f32>,
    /// `lib:<file name>` (built-in) or an image file (PNG, JPG, TIFF incl. 1-bit, WebP, BMP)
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient::text")]
    pub path: Option<String>,
    /// texture width on the print in mm; None = cover the whole print
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient::num")]
    pub scale_mm: Option<f64>,
    /// size factor on top of the cover / width size (1 = exactly covering)
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient::num")]
    pub zoom: Option<f64>,
    /// rotation in degrees
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient::num")]
    pub angle: Option<f64>,
    /// mirror the texture horizontally
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient::flag")]
    pub mirror: Option<bool>,
    /// swap which pixels are worn (white = worn)
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient::flag")]
    pub invert: Option<bool>,
    /// repeat (mirrored) outside the texture; false = nothing happens outside it
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient::flag")]
    pub tile: Option<bool>,
    /// shift of the texture centre from the print centre, mm
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "lenient::pair")]
    pub offset_mm: Option<[f64; 2]>,
    /// kept for older jobs (not used)
    #[serde(default, skip_serializing, deserialize_with = "lenient::seed")]
    pub seed: u32,
    /// print size in mm, filled in by `with_print_size` (not part of the job JSON)
    #[serde(skip)]
    pub print_mm: Option<[f64; 2]>,
}

/// A layer with every default filled in and every number clamped.
#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub kind: Kind,
    pub mode: Mode,
    pub threshold: f64,
    pub amount: f64,
    pub path: Option<String>,
    pub scale_mm: Option<f64>,
    pub zoom: f64,
    pub angle: f64,
    pub mirror: bool,
    pub invert: bool,
    pub tile: bool,
    pub offset_mm: [f64; 2],
    pub print_mm: Option<[f64; 2]>,
}

/// f32 from the JSON as the decimal the user typed (0.15f32 -> 0.15, not 0.1500000059)
fn f32_dec(v: f32) -> f64 {
    format!("{v}").parse().unwrap_or(v as f64)
}

fn fin(v: Option<f64>, def: f64, lo: f64, hi: f64) -> f64 {
    match v {
        Some(x) if x.is_finite() => x.clamp(lo, hi),
        _ => def,
    }
}

impl Layer {
    /// a texture layer with all defaults
    pub fn new(path: &str) -> Layer {
        Layer {
            kind: Kind::Gambar,
            mode: None,
            threshold: None,
            amount: None,
            path: Some(path.to_string()),
            scale_mm: None,
            zoom: None,
            angle: None,
            mirror: None,
            invert: None,
            tile: None,
            offset_mm: None,
            seed: 0,
            print_mm: None,
        }
    }

    /// resolved parameters: defaults + clamping (never panics)
    pub fn params(&self) -> Params {
        let angle = match self.angle {
            Some(a) if a.is_finite() && a.abs() <= 1e6 => {
                let r = a.rem_euclid(360.0);
                if r > 180.0 {
                    r - 360.0
                } else {
                    r
                }
            }
            _ => 0.0,
        };
        let off = self.offset_mm.unwrap_or([0.0; 2]);
        Params {
            kind: self.kind,
            mode: self.mode.unwrap_or(Mode::Lubang),
            threshold: fin(self.threshold.map(f32_dec), 0.5, 0.0, 1.0),
            amount: fin(self.amount.map(f32_dec), 0.5, 0.0, 1.0),
            path: self.path.clone().filter(|p| !p.trim().is_empty()),
            scale_mm: self
                .scale_mm
                .filter(|x| x.is_finite() && *x > 0.0)
                .map(|x| x.clamp(1.0, 100_000.0)),
            zoom: fin(self.zoom.filter(|z| *z > 0.0), 1.0, 0.1, 10.0),
            angle,
            mirror: self.mirror.unwrap_or(false),
            invert: self.invert.unwrap_or(false),
            tile: self.tile.unwrap_or(true),
            offset_mm: off.map(|v| fin(Some(v), 0.0, -1e5, 1e5)),
            print_mm: self.print_mm,
        }
    }
}

impl Params {
    /// does this layer change anything at all?
    pub fn active(&self) -> bool {
        self.kind == Kind::Gambar
            && self.path.is_some()
            && ((self.mode.holes() && self.threshold > 0.0) || (self.mode.fades() && self.amount > 0.0))
    }

    pub fn describe(&self) -> String {
        let mut s = format!(
            "{} {} kekuatan={} pudar={} path={} {} zoom={} angle={} mirror={} invert={} tile={} offset_mm={},{}",
            self.kind.name(),
            self.mode.name(),
            self.threshold,
            self.amount,
            self.path.as_deref().unwrap_or("-"),
            self.scale_mm
                .map(|v| format!("lebar={v}mm"))
                .unwrap_or_else(|| "menutupi-cetakan".into()),
            self.zoom,
            self.angle,
            self.mirror,
            self.invert,
            self.tile,
            self.offset_mm[0],
            self.offset_mm[1]
        );
        if !self.active() {
            s += " (tidak aktif)";
        }
        s
    }
}

/// tolerant field parsers: numbers may arrive as strings or null (UI number inputs),
/// bad values fall back to the defaults instead of failing the whole job
mod lenient {
    use super::Mode;
    use serde::{Deserialize, Deserializer};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Any {
        B(bool),
        N(f64),
        S(String),
        A(Vec<Any>),
        #[allow(dead_code)]
        O(serde::de::IgnoredAny),
    }

    fn num_of(a: &Any) -> Option<f64> {
        let v = match a {
            Any::N(x) => Some(*x),
            Any::S(s) => s.trim().replace(',', ".").parse::<f64>().ok(),
            Any::B(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        };
        v.filter(|x| x.is_finite())
    }

    pub fn num<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f64>, D::Error> {
        Ok(Option::<Any>::deserialize(d)?.as_ref().and_then(num_of))
    }

    pub fn num32<'de, D: Deserializer<'de>>(d: D) -> Result<Option<f32>, D::Error> {
        Ok(num(d)?.map(|v| v.clamp(-1e30, 1e30) as f32))
    }

    pub fn seed<'de, D: Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
        Ok(num(d)?
            .map(|v| v.round().clamp(0.0, u32::MAX as f64) as u32)
            .unwrap_or(0))
    }

    pub fn flag<'de, D: Deserializer<'de>>(d: D) -> Result<Option<bool>, D::Error> {
        Ok(match Option::<Any>::deserialize(d)? {
            Some(Any::B(b)) => Some(b),
            Some(Any::N(x)) => Some(x != 0.0),
            Some(Any::S(s)) => match s.trim().to_ascii_lowercase().as_str() {
                "true" | "1" | "ya" | "yes" | "on" => Some(true),
                "false" | "0" | "tidak" | "no" | "off" => Some(false),
                _ => None,
            },
            _ => None,
        })
    }

    pub fn text<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
        Ok(match Option::<Any>::deserialize(d)? {
            Some(Any::S(s)) if !s.trim().is_empty() => Some(s),
            _ => None,
        })
    }

    pub fn mode<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Mode>, D::Error> {
        Ok(match Option::<Any>::deserialize(d)? {
            Some(Any::S(s)) => match s.trim().to_ascii_lowercase().as_str() {
                "lubang" | "holes" | "knockout" => Some(Mode::Lubang),
                "pudar" | "fade" => Some(Mode::Pudar),
                "campur" | "mix" | "both" => Some(Mode::Campur),
                _ => None,
            },
            _ => None,
        })
    }

    pub fn pair<'de, D: Deserializer<'de>>(d: D) -> Result<Option<[f64; 2]>, D::Error> {
        Ok(match Option::<Any>::deserialize(d)? {
            Some(Any::A(v)) => {
                let g = |i: usize| v.get(i).and_then(num_of).unwrap_or(0.0);
                Some([g(0), g(1)])
            }
            Some(Any::S(s)) => {
                let p: Vec<f64> = s
                    .split([';', '/', 'x', ' '])
                    .filter(|t| !t.is_empty())
                    .map(|t| t.trim().replace(',', ".").parse::<f64>().unwrap_or(0.0))
                    .collect();
                Some([
                    p.first().copied().unwrap_or(0.0),
                    p.get(1).copied().unwrap_or(0.0),
                ])
            }
            Some(a) => num_of(&a).map(|x| [x, x]),
            None => None,
        })
    }
}

// ------------------------------------------------------------------ settings helpers

/// resolved, active layers of the settings (empty when nothing changes the render)
pub fn layers(s: &Settings) -> Vec<Params> {
    if !(s.dpi.is_finite() && s.dpi > 0.0) {
        return Vec::new();
    }
    s.distress
        .iter()
        .map(Layer::params)
        .filter(Params::active)
        .collect()
}

/// any active layer that knocks holes (needs the extra halo and the dot-aware pass)
pub fn has_holes(s: &Settings) -> bool {
    layers(s).iter().any(|l| l.mode.holes())
}

/// Settings with the print size (pixels at `s.dpi`) stored in every texture layer:
/// the texture covers and centres on the whole print. The full render and
/// `band::Plan` call this; `render_at` alone uses its window as the print.
pub fn with_print_size(s: &Settings, print_w: usize, print_h: usize) -> Settings {
    let mut s2 = s.clone();
    if s.dpi.is_finite() && s.dpi > 0.0 {
        let mm = [print_w as f64 * 25.4 / s.dpi, print_h as f64 * 25.4 / s.dpi];
        for l in s2.distress.iter_mut() {
            if l.kind == Kind::Gambar {
                l.print_mm = Some(mm);
            }
        }
    }
    s2
}

/// does any texture layer still miss the print size?
pub fn needs_print_size(s: &Settings) -> bool {
    s.distress
        .iter()
        .any(|l| l.kind == Kind::Gambar && l.print_mm.is_none())
}

/// Check the layers before rendering (the app and the CLI call this): every texture
/// layer needs a readable image. Rendering itself never fails on a bad texture; the
/// layer is then left out.
pub fn validate(s: &Settings) -> Result<(), String> {
    let mut errs = Vec::new();
    for (i, l) in s.distress.iter().enumerate() {
        let p = l.params();
        if p.kind != Kind::Gambar {
            continue;
        }
        match &p.path {
            None => errs.push(format!("lapisan {}: belum memilih tekstur", i + 1)),
            Some(path) => {
                if let Err(e) = Src::of(path).and_then(|src| probe(&src)) {
                    errs.push(format!("lapisan {}: tekstur {path}: {e}", i + 1));
                }
            }
        }
    }
    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs.join("; "))
    }
}

/// one line per layer (CLI / logs)
pub fn describe(s: &Settings) -> Vec<String> {
    s.distress.iter().map(|l| l.params().describe()).collect()
}

/// grey preview (worn = black, as the engine sees it) at most `max` px wide/high:
/// (w, h, RGBA8)
pub fn thumbnail(path: &str, max: usize) -> Option<(usize, usize, Vec<u8>)> {
    let src = Src::of(path).ok()?;
    let (w, h, plane) = decode_plane(&src).ok()?;
    let f = (w.max(h) as f64 / max.max(1) as f64).max(1.0);
    let (tw, th) = (((w as f64 / f).round() as usize).max(1), ((h as f64 / f).round() as usize).max(1));
    let small = if (tw, th) == (w, h) { plane } else { area_resample(&plane, w, h, tw, th) };
    let mut out = Vec::with_capacity(tw * th * 4);
    for v in small {
        let g = 255 - v;
        out.extend_from_slice(&[g, g, g, 255]);
    }
    Some((tw, th, out))
}

/// size (w, h) of a texture source without decoding it
pub fn texture_size(path: &str) -> Result<(u32, u32), String> {
    probe(&Src::of(path)?)
}

// ------------------------------------------------------------------ deterministic math

/// splitmix64 finaliser (test data)
#[cfg(test)]
#[inline(always)]
fn mix(mut z: u64) -> u64 {
    // splitmix64 finaliser
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// sin and cos of an angle in degrees with basic operations only (bit-identical on
/// every platform; exact for multiples of 90 degrees)
pub fn sincos_deg(deg: f64) -> (f64, f64) {
    if !deg.is_finite() {
        return (0.0, 1.0);
    }
    let d = deg - 360.0 * (deg / 360.0).floor();
    let q = ((d + 45.0) / 90.0).floor();
    let r = (d - 90.0 * q) * (std::f64::consts::PI / 180.0);
    let r2 = r * r;
    // Taylor series, |r| <= pi/4
    let mut s = 0.0;
    let mut c = 0.0;
    let mut fs = 1.0; // 1/(2n+1)!
    let mut fc = 1.0; // 1/(2n)!
    let mut terms_s = [0.0f64; 10];
    let mut terms_c = [0.0f64; 10];
    for n in 0..10 {
        let sign = if n % 2 == 0 { 1.0 } else { -1.0 };
        terms_s[n] = sign * fs;
        terms_c[n] = sign * fc;
        let k = (2 * n + 2) as f64;
        fc /= (k - 1.0) * k;
        fs /= k * (k + 1.0);
    }
    for n in (0..10).rev() {
        s = s * r2 + terms_s[n];
        c = c * r2 + terms_c[n];
    }
    let s = s * r;
    match (q as i64).rem_euclid(4) {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

// ------------------------------------------------------------------ texture

/// a decoded texture: removal strength per pixel (255 - luma, or alpha when the file
/// has transparency), resampled for the print
pub struct Texture {
    pub w: usize,
    pub h: usize,
    /// source size (aspect ratio and native pixel size)
    pub src_w: usize,
    pub src_h: usize,
    pub data: Vec<u8>,
}

/// where a texture comes from: a file of the user, or the built-in library
/// (`lib:<file name>`, files of the repository folder `textures/`)
pub enum Src {
    File(std::path::PathBuf),
    Lib(&'static [u8]),
}

impl Src {
    pub fn of(path: &str) -> Result<Src, String> {
        match path.strip_prefix(LIB_PREFIX) {
            Some(name) => crate::library::get(name)
                .map(Src::Lib)
                .ok_or_else(|| format!("tekstur pustaka '{name}' tidak ada di versi ini")),
            None => Ok(Src::File(Path::new(path).to_path_buf())),
        }
    }

    /// (length, mtime) for the cache key
    fn stamp(&self) -> Option<(u64, u128)> {
        match self {
            Src::Lib(b) => Some((b.len() as u64, 0)),
            Src::File(p) => {
                let meta = std::fs::metadata(p).ok()?;
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                Some((meta.len(), mtime))
            }
        }
    }

    fn read(&self) -> Result<Box<dyn std::io::Read + '_>, String> {
        Ok(match self {
            Src::Lib(b) => Box::new(std::io::Cursor::new(*b)),
            Src::File(p) => Box::new(std::io::BufReader::new(
                std::fs::File::open(p).map_err(|e| e.to_string())?,
            )),
        })
    }
}

/// path prefix of built-in library textures
pub const LIB_PREFIX: &str = "lib:";

fn image_reader<R: std::io::BufRead + std::io::Seek>(
    r: image::ImageReader<R>,
) -> Result<image::ImageReader<R>, String> {
    r.with_guessed_format()
        .map_err(|e| format!("tidak bisa dibaca ({e})"))
}

/// read only the header (validate)
fn probe(src: &Src) -> Result<(u32, u32), String> {
    match src {
        Src::Lib(b) => probe_reader(image_reader(image::ImageReader::new(std::io::Cursor::new(*b)))?),
        Src::File(p) => probe_reader(image_reader(
            image::ImageReader::open(p).map_err(|e| format!("tidak bisa dibuka ({e})"))?,
        )?),
    }
}

fn probe_reader<R: std::io::BufRead + std::io::Seek>(r: image::ImageReader<R>) -> Result<(u32, u32), String> {
    if r.format().is_none() {
        return Err("format tidak dikenal (pakai PNG, JPG, TIFF, WebP atau BMP)".into());
    }
    let (w, h) = r
        .into_dimensions()
        .map_err(|e| format!("tidak bisa dibaca ({e})"))?;
    if w == 0 || h == 0 {
        return Err("ukuran gambar 0".into());
    }
    Ok((w, h))
}

/// removal plane at source resolution (1 byte per pixel; the f32 io::decode path is
/// never used so 12000 x 12000 bitmap TIFFs open)
fn decode_plane(src: &Src) -> Result<(usize, usize, Vec<u8>), String> {
    let is_png = src
        .read()
        .ok()
        .and_then(|mut f| {
            let mut b = [0u8; 8];
            f.read_exact(&mut b).ok().map(|_| b)
        })
        .is_some_and(|b| b == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    if is_png {
        if let Some(r) = decode_png_stream(src)? {
            return Ok(r);
        }
    }
    let img = match src {
        Src::Lib(b) => decode_image(image_reader(image::ImageReader::new(std::io::Cursor::new(*b)))?)?,
        Src::File(p) => decode_image(image_reader(image::ImageReader::open(p).map_err(|e| e.to_string())?)?)?,
    };
    let (w, h) = (img.width() as usize, img.height() as usize);
    let plane = if img.color().has_alpha() {
        let la = img.into_luma_alpha8().into_raw();
        if la.chunks_exact(2).any(|p| p[1] < 255) {
            la.chunks_exact(2).map(|p| p[1]).collect()
        } else {
            la.chunks_exact(2).map(|p| 255 - p[0]).collect()
        }
    } else {
        let mut l = img.into_luma8().into_raw();
        l.iter_mut().for_each(|v| *v = 255 - *v);
        l
    };
    Ok((w, h, plane))
}

fn decode_image<R: std::io::BufRead + std::io::Seek>(
    mut rd: image::ImageReader<R>,
) -> Result<image::DynamicImage, String> {
    let mut lim = image::Limits::default();
    lim.max_image_width = Some(65_535);
    lim.max_image_height = Some(65_535);
    lim.max_alloc = Some(3 << 30);
    rd.limits(lim);
    rd.decode().map_err(|e| e.to_string())
}

/// row-streamed PNG decode (no full RGBA buffer); None = interlaced, use `image`
fn decode_png_stream(src: &Src) -> Result<Option<(usize, usize, Vec<u8>)>, String> {
    let f = src.read()?;
    let mut dec = png::Decoder::new_with_limits(f, png::Limits { bytes: 1 << 31 });
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut rd = dec.read_info().map_err(|e| e.to_string())?;
    if rd.info().interlaced {
        return Ok(None);
    }
    let (w, h) = (rd.info().width as usize, rd.info().height as usize);
    if w == 0 || h == 0 || w > 65_535 || h > 65_535 {
        return Err("ukuran gambar tidak didukung".into());
    }
    let (ct, _) = rd.output_color_type();
    let ch = match ct {
        png::ColorType::Grayscale => 1,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        png::ColorType::Indexed => return Ok(None),
    };
    let has_a = ch == 2 || ch == 4;
    let mut lum = vec![0u8; w * h];
    let mut alp = if has_a { vec![0u8; w * h] } else { Vec::new() };
    let mut transparent = false;
    let mut y = 0;
    while let Some(row) = rd.next_row().map_err(|e| e.to_string())? {
        if y >= h {
            break;
        }
        let d = row.data();
        for x in 0..w.min(d.len() / ch) {
            let px = &d[x * ch..x * ch + ch];
            let l = if ch >= 3 {
                // Rec. 709 luma like image::to_luma8
                ((px[0] as u32 * 2126 + px[1] as u32 * 7152 + px[2] as u32 * 722 + 5000) / 10000)
                    as u8
            } else {
                px[0]
            };
            lum[y * w + x] = l;
            if has_a {
                let a = px[ch - 1];
                alp[y * w + x] = a;
                transparent |= a < 255;
            }
        }
        y += 1;
    }
    if y < h {
        return Err("file PNG terpotong".into());
    }
    if transparent {
        Ok(Some((w, h, alp)))
    } else {
        lum.iter_mut().for_each(|v| *v = 255 - *v);
        Ok(Some((w, h, lum)))
    }
}

/// area-average resample (exact interval overlaps) of a u8 plane
fn area_resample(src: &[u8], sw: usize, sh: usize, tw: usize, th: usize) -> Vec<u8> {
    fn weights(s: usize, t: usize) -> Vec<(usize, Vec<f64>)> {
        let f = s as f64 / t as f64;
        (0..t)
            .map(|i| {
                let a = i as f64 * f;
                let b = ((i + 1) as f64 * f).min(s as f64);
                let i0 = a.floor() as usize;
                let i1 = (b.ceil() as usize).min(s).max(i0 + 1);
                let w = (i0..i1)
                    .map(|k| (b.min(k as f64 + 1.0) - a.max(k as f64)).max(0.0))
                    .collect();
                (i0, w)
            })
            .collect()
    }
    let wx = weights(sw, tw);
    let wy = weights(sh, th);
    let mut out = vec![0u8; tw * th];
    out.par_chunks_mut(tw).enumerate().for_each(|(oy, orow)| {
        let (j0, ref wys) = wy[oy];
        let mut acc = vec![0.0f64; tw];
        let mut tot = 0.0;
        for (dj, &wyv) in wys.iter().enumerate() {
            if wyv <= 0.0 {
                continue;
            }
            let line = &src[(j0 + dj) * sw..(j0 + dj + 1) * sw];
            for (ox, (i0, wxs)) in wx.iter().enumerate() {
                let mut s = 0.0;
                for (di, &wxv) in wxs.iter().enumerate() {
                    s += wxv * line[i0 + di] as f64;
                }
                acc[ox] += wyv * s;
            }
            tot += wyv;
        }
        let fx = sw as f64 / tw as f64;
        for (o, a) in orow.iter_mut().zip(&acc) {
            *o = (a / (tot * fx)).round().clamp(0.0, 255.0) as u8;
        }
    });
    out
}

type TexKey = (String, u64, u128, usize, u64);

struct TexCache {
    items: Vec<(TexKey, Arc<Texture>, u64)>,
    tick: u64,
}

const TEX_CACHE_ITEMS: usize = 4;
const TEX_CACHE_BYTES: usize = 768 << 20;

fn tex_cache() -> &'static Mutex<TexCache> {
    static C: OnceLock<Mutex<TexCache>> = OnceLock::new();
    C.get_or_init(|| {
        Mutex::new(TexCache {
            items: Vec::new(),
            tick: 0,
        })
    })
}

/// Texture for `path` covering `width_px` print pixels (None = native size), decoded
/// and area-resampled once, cached by (path, file length, mtime, target width, dpi)
/// in a small LRU. Errors (missing file, unknown format) return None.
pub fn texture(path: &str, width_px: Option<f64>, dpi: f64) -> Option<Arc<Texture>> {
    let src = Src::of(path).ok()?;
    let (len, mtime) = src.stamp()?;
    let sw = probe(&src).ok()?.0 as usize;
    // stored width: shrink to the print resolution, never enlarge (bilinear sampling)
    let tw = match width_px {
        Some(wp) if wp.is_finite() && wp > 0.0 => (wp.round() as usize).clamp(1, sw),
        _ => sw,
    };
    let k: TexKey = (path.to_string(), len, mtime, tw, dpi.to_bits());
    {
        let mut c = tex_cache().lock().unwrap_or_else(|e| e.into_inner());
        c.tick += 1;
        let t = c.tick;
        if let Some(it) = c.items.iter_mut().find(|it| it.0 == k) {
            it.2 = t;
            return Some(it.1.clone());
        }
    }
    let (w0, h0, plane) = decode_plane(&src).ok()?;
    let tex = if tw < w0 {
        let th = ((tw as f64 * h0 as f64 / w0 as f64).round() as usize).max(1);
        Texture {
            w: tw,
            h: th,
            src_w: w0,
            src_h: h0,
            data: area_resample(&plane, w0, h0, tw, th),
        }
    } else {
        Texture {
            w: w0,
            h: h0,
            src_w: w0,
            src_h: h0,
            data: plane,
        }
    };
    let tex = Arc::new(tex);
    let mut c = tex_cache().lock().unwrap_or_else(|e| e.into_inner());
    c.tick += 1;
    let t = c.tick;
    if let Some(it) = c.items.iter().find(|it| it.0 == k) {
        return Some(it.1.clone());
    }
    c.items.push((k, tex.clone(), t));
    loop {
        let bytes: usize = c.items.iter().map(|it| it.1.data.len()).sum();
        if c.items.len() <= TEX_CACHE_ITEMS && (bytes <= TEX_CACHE_BYTES || c.items.len() == 1) {
            break;
        }
        let (i, _) = c
            .items
            .iter()
            .enumerate()
            .min_by_key(|(_, it)| it.2)
            .expect("non-empty");
        c.items.remove(i);
    }
    Some(tex)
}

/// texture placement on the print (all in print pixels)
struct Placed {
    tex: Arc<Texture>,
    /// texture size on the print
    wp: f64,
    hp: f64,
    cx: f64,
    cy: f64,
    cs: f64,
    sn: f64,
    tile: bool,
    invert: bool,
    mirror: bool,
}

impl Placed {
    /// `win_w`/`win_h`: the window, used as the print when the print size is unknown
    fn new(l: &Params, dpi: f64, win_w: usize, win_h: usize) -> Option<Placed> {
        let path = l.path.as_deref()?;
        let ppm = dpi / 25.4;
        let (tw, th) = texture_size(path).ok()?;
        let (tw, th) = (tw as f64, th as f64);
        let (pw, ph) = match l.print_mm {
            Some([w, h]) if w > 0.0 && h > 0.0 => (w * ppm, h * ppm),
            _ => (win_w as f64, win_h as f64),
        };
        let (sn, cs) = sincos_deg(l.angle);
        // cover: the rotated texture rectangle contains the whole print
        let base = match l.scale_mm {
            Some(mm) => mm * ppm,
            None => {
                let (ac, asn) = (cs.abs(), sn.abs());
                let s = ((pw * ac + ph * asn) / tw).max((ph * ac + pw * asn) / th);
                s * tw
            }
        };
        let wp = (base * l.zoom).max(1.0);
        let hp = wp * th / tw;
        let tex = texture(path, Some(wp), dpi)?;
        Some(Placed {
            cx: 0.5 * pw + l.offset_mm[0] * ppm,
            cy: 0.5 * ph + l.offset_mm[1] * ppm,
            wp,
            hp,
            cs,
            sn,
            tile: l.tile,
            invert: l.invert,
            mirror: l.mirror,
            tex,
        })
    }

    /// texture darkness 0..1 (1 = fully worn) at absolute print pixel (x, y); outside
    /// an untiled texture nothing is worn
    #[inline]
    fn at(&self, x: usize, y: usize) -> f64 {
        let px = x as f64 + 0.5 - self.cx;
        let py = y as f64 + 0.5 - self.cy;
        let mut u = self.cs * px + self.sn * py + 0.5 * self.wp;
        let v = -self.sn * px + self.cs * py + 0.5 * self.hp;
        if self.mirror {
            u = self.wp - u;
        }
        let t = &self.tex;
        let tu = u * (t.w as f64 / self.wp);
        let tv = v * (t.h as f64 / self.hp);
        if !self.tile && !(tu >= 0.0 && tu < t.w as f64 && tv >= 0.0 && tv < t.h as f64) {
            return 0.0;
        }
        let fx = tu - 0.5;
        let fy = tv - 0.5;
        let x0 = fx.floor();
        let y0 = fy.floor();
        let ax = fx - x0;
        let ay = fy - y0;
        let (w, h) = (t.w as i64, t.h as i64);
        // mirrored repeat: no seams where the texture continues
        let idx = |i: i64, n: i64| -> usize {
            if self.tile {
                let m = i.rem_euclid(2 * n);
                (if m < n { m } else { 2 * n - 1 - m }) as usize
            } else {
                i.clamp(0, n - 1) as usize
            }
        };
        let (i0, i1) = (idx(x0 as i64, w), idx(x0 as i64 + 1, w));
        let (j0, j1) = (idx(y0 as i64, h), idx(y0 as i64 + 1, h));
        let g = |i: usize, j: usize| t.data[j * t.w + i] as f64;
        let top = g(i0, j0) + (g(i1, j0) - g(i0, j0)) * ax;
        let bot = g(i0, j1) + (g(i1, j1) - g(i0, j1)) * ax;
        let r = (top + (bot - top) * ay) / 255.0;
        if self.invert {
            1.0 - r
        } else {
            r
        }
    }
}

/// texture darkness of a window, None if the texture is unusable
fn field(win: &Win, l: &Params) -> Option<Vec<f64>> {
    let pl = Placed::new(l, win.dpi, win.w, win.h)?;
    let mut out = vec![0.0; win.w * win.h];
    out.par_chunks_mut(win.w).enumerate().for_each(|(row, o)| {
        for (x, v) in o.iter_mut().enumerate() {
            *v = pl.at(win.ox + x, win.oy + row);
        }
    });
    Some(out)
}

/// the pixel window being rendered: size, absolute origin, print dpi
#[derive(Clone, Copy)]
pub struct Win {
    pub w: usize,
    pub h: usize,
    pub ox: usize,
    pub oy: usize,
    pub dpi: f64,
}

// ------------------------------------------------------------------ per window

/// Fade multiplier (product of 1 - amount * darkness over the fading layers) for the
/// window, or None when no layer fades. Multiplies the opacity before screening (=
/// tone on a black shirt, ink on a white shirt, opacity on a coloured shirt).
pub fn fade(s: &Settings, w: usize, h: usize, ox: usize, oy: usize) -> Option<Vec<f32>> {
    let ls: Vec<Params> = layers(s).into_iter().filter(|l| l.mode.fades()).collect();
    if ls.is_empty() || w == 0 || h == 0 {
        return None;
    }
    let win = Win { w, h, ox, oy, dpi: s.dpi };
    let mut f = vec![1.0f64; w * h];
    let mut any = false;
    for l in &ls {
        if let Some(fl) = field(&win, l) {
            any = true;
            f.par_iter_mut().zip(fl.par_iter()).for_each(|(v, x)| {
                *v *= 1.0 - l.amount * x.clamp(0.0, 1.0);
            });
        }
    }
    any.then(|| f.into_iter().map(|v| v as f32).collect())
}

/// cleaned hole mask (union of the hole layers) of the window, or None
pub fn holes(s: &Settings, w: usize, h: usize, ox: usize, oy: usize) -> Option<Vec<bool>> {
    let ls: Vec<Params> = layers(s).into_iter().filter(|l| l.mode.holes()).collect();
    if ls.is_empty() || w == 0 || h == 0 {
        return None;
    }
    let win = Win { w, h, ox, oy, dpi: s.dpi };
    let mut m: Option<Vec<bool>> = None;
    for l in &ls {
        if let Some(fl) = field(&win, l) {
            let cut = 1.0 - l.threshold;
            let lm: Vec<bool> = fl.par_iter().map(|&r| r > cut).collect();
            m = Some(match m {
                None => lm,
                Some(mut a) => {
                    a.par_iter_mut().zip(lm.par_iter()).for_each(|(x, y)| *x |= *y);
                    a
                }
            });
        }
    }
    let m = m?;
    let m = clean_mask(&m, w, h, mindot_px(s.dpi));
    m.iter().any(|&v| v).then_some(m)
}

// ------------------------------------------------------------------ mask clean-up

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    /// out(p) = any m(p - d), d in 0..k (both axes)
    DilateBack,
    /// out(p) = any m(p + d)
    DilateFwd,
    /// out(p) = all m(p + d)
    ErodeFwd,
}

/// square k x k dilation / erosion in O(1) per pixel (run lengths); samples outside
/// the window are ignored
fn morph_op(m: &[bool], w: usize, h: usize, k: usize, op: Op) -> Vec<bool> {
    if k <= 1 || w == 0 || h == 0 {
        return m.to_vec();
    }
    let mut a = vec![false; w * h];
    a.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let src = &m[y * w..(y + 1) * w];
        match op {
            Op::DilateBack => {
                let mut last = usize::MAX;
                for x in 0..w {
                    if src[x] {
                        last = x;
                    }
                    row[x] = last != usize::MAX && x - last < k;
                }
            }
            Op::DilateFwd => {
                let mut next = usize::MAX;
                for x in (0..w).rev() {
                    if src[x] {
                        next = x;
                    }
                    row[x] = next != usize::MAX && next - x < k;
                }
            }
            Op::ErodeFwd => {
                let mut nf = usize::MAX;
                for x in (0..w).rev() {
                    if !src[x] {
                        nf = x;
                    }
                    row[x] = nf == usize::MAX || nf - x >= k;
                }
            }
        }
    });
    // vertical: distance counters per column, columns split over threads
    let cw = w.div_ceil(rayon::current_num_threads().max(1) * 2).max(64);
    let starts: Vec<usize> = (0..w).step_by(cw).collect();
    let cols: Vec<(usize, Vec<bool>)> = starts
        .into_par_iter()
        .map(|x0| {
            let n = (x0 + cw).min(w) - x0;
            let mut out = vec![false; n * h];
            let mut d = vec![usize::MAX / 2; n];
            let mut step = |y: usize| {
                let src = &a[y * w + x0..y * w + x0 + n];
                let dst = &mut out[y * n..(y + 1) * n];
                for i in 0..n {
                    if op == Op::ErodeFwd {
                        d[i] = if src[i] { d[i] + 1 } else { 0 };
                        dst[i] = d[i] >= k;
                    } else {
                        d[i] = if src[i] { 0 } else { d[i] + 1 };
                        dst[i] = d[i] < k;
                    }
                }
            };
            if op == Op::DilateBack {
                (0..h).for_each(&mut step);
            } else {
                (0..h).rev().for_each(&mut step);
            }
            (x0, out)
        })
        .collect();
    let mut b = vec![false; w * h];
    b.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x0, out) in &cols {
            let n = out.len() / h;
            row[*x0..x0 + n].copy_from_slice(&out[y * n..(y + 1) * n]);
        }
    });
    b
}

/// k x k dilation (offsets 0..k back) or erosion (offsets 0..k forward)
fn morph(m: &[bool], w: usize, h: usize, k: usize, dilate: bool) -> Vec<bool> {
    morph_op(
        m,
        w,
        h,
        k,
        if dilate { Op::DilateBack } else { Op::ErodeFwd },
    )
}

/// close (merge features closer than 0.15 mm: no ink slivers) then open (drop mask
/// features thinner than 0.15 mm: no hairline holes), then snap to the k x k block
/// grid of the FM clusters (block majority, tie = its top-left pixel) so cuts through
/// solid ink leave whole blocks. Mask only, never the ink. The window origin must be
/// block aligned (as for `render_at`).
pub fn clean_mask(m: &[bool], w: usize, h: usize, k: usize) -> Vec<bool> {
    let c = morph(&morph(m, w, h, k, true), w, h, k, false);
    let o = morph(&morph(&c, w, h, k, false), w, h, k, true);
    if k <= 1 {
        return o;
    }
    let bw = w.div_ceil(k);
    let mut out = vec![false; w * h];
    out.par_chunks_mut(w * k)
        .enumerate()
        .for_each(|(by, rows)| {
            let y0 = by * k;
            let nr = rows.len() / w;
            for bx in 0..bw {
                let x0 = bx * k;
                let x1 = (x0 + k).min(w);
                let mut cnt = 0;
                for y in y0..y0 + nr {
                    cnt += o[y * w + x0..y * w + x1].iter().filter(|&&v| v).count();
                }
                let n = nr * (x1 - x0);
                let on = 2 * cnt > n || (2 * cnt == n && o[y0 * w + x0]);
                if on {
                    for r in 0..nr {
                        rows[r * w + x0..r * w + x1]
                            .iter_mut()
                            .for_each(|v| *v = true);
                    }
                }
            }
        });
    out
}

// ------------------------------------------------------------------ dot-aware knockout

/// (max dot area in px, max dot bounding-box side in px): ink components at most this
/// big are halftone dots, kept or dropped whole
pub fn dot_limits(s: &Settings) -> (f64, usize) {
    let cell = match s.pattern {
        Pattern::Am | Pattern::Interlock if s.lpi.is_finite() && s.lpi > 0.0 => s.dpi / s.lpi,
        _ => 0.5 * s.dpi / 25.4,
    };
    let cell = if cell.is_finite() {
        cell.clamp(1.0, 4096.0)
    } else {
        8.0
    };
    (0.8 * cell * cell, (1.5 * cell).ceil() as usize)
}

/// extra halo for the hole layers (see the sum below)
pub fn halo_extra(s: &Settings) -> usize {
    if !has_holes(s) {
        return 0;
    }
    let k = mindot_px(s.dpi);
    // mask close + open (4 k) + block snap (k), one dot, sliver trim (2 k + k)
    dot_limits(s).1 + 8 * k + 8
}

/// Remove ink under the cleaned mask: halftone dots (small components that do not
/// touch the window edge) are dropped whole when most of their pixels are under the
/// mask and kept whole otherwise; larger (solid) ink is cut exactly, and ink slivers
/// thinner than 0.15 mm left next to a cut are removed with it. RGB = 0 where the
/// ink is removed. `dtf::clean` must run afterwards.
pub fn knock_out(o: &mut [u8], w: usize, h: usize, mask: &[bool], s: &Settings) {
    let n = w * h;
    let ink: Vec<bool> = (0..n).map(|p| o[p * 4 + 3] == 255).collect();
    let (amax, span) = dot_limits(s);
    // per pixel: 0 = not looked at, 1 = solid, 2 = dot kept, 3 = dot dropped. Only
    // components under the mask are explored, and a flood stops as soon as it is
    // known to be solid (too big, too wide, at the window edge or touching solid)
    let mut st = vec![0u8; n];
    let mut stamp = vec![0u32; n];
    let mut cur = 0u32;
    let mut stack: Vec<usize> = Vec::new();
    let mut seen: Vec<usize> = Vec::new();
    for p0 in 0..n {
        if !ink[p0] || !mask[p0] || st[p0] != 0 {
            continue;
        }
        cur += 1;
        seen.clear();
        stack.clear();
        stack.push(p0);
        stamp[p0] = cur;
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
        let mut hits = 0usize;
        let mut solid = false;
        'flood: while let Some(q) = stack.pop() {
            seen.push(q);
            let (x, y) = (q % w, q / w);
            hits += mask[q] as usize;
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            if x == 0
                || y == 0
                || x + 1 == w
                || y + 1 == h
                || seen.len() as f64 >= amax
                || x1 - x0 >= span
                || y1 - y0 >= span
            {
                solid = true;
                break;
            }
            for nb in [q - 1, q + 1, q - w, q + w] {
                if ink[nb] {
                    if st[nb] == 1 {
                        solid = true;
                        break 'flood;
                    }
                    if stamp[nb] != cur {
                        stamp[nb] = cur;
                        stack.push(nb);
                    }
                }
            }
        }
        let v = if solid {
            1
        } else if 2 * hits > seen.len() {
            3
        } else {
            2
        };
        for &q in &seen {
            st[q] = v;
        }
    }
    // pixels cut out of solid ink (exact cut) and the ink left
    let cut: Vec<bool> = (0..n)
        .into_par_iter()
        .map(|p| ink[p] && mask[p] && st[p] == 1)
        .collect();
    let mut left: Vec<bool> = (0..n)
        .into_par_iter()
        .map(|p| ink[p] && st[p] != 3 && !cut[p])
        .collect();
    // ink slivers thinner than 0.15 mm between a cut and the design edge (or a
    // halftone gap) are removed too: ink pixels near a cut that no k x k square of
    // ink covers
    let k = mindot_px(s.dpi);
    if k > 1 && cut.iter().any(|&c| c) {
        let opened = morph(&morph(&left, w, h, k, false), w, h, k, true);
        let near = dilate_sym(&cut, w, h, k);
        left.par_iter_mut()
            .zip(opened.par_iter().zip(near.par_iter()))
            .for_each(|(l, (op, nr))| {
                if *l && !*op && *nr {
                    *l = false;
                }
            });
    }
    o.par_chunks_mut(4)
        .zip(left.par_iter())
        .for_each(|(px, &keep)| {
            if px[3] == 255 && !keep {
                px.copy_from_slice(&[0, 0, 0, 0]);
            }
        });
}

/// dilation with a (2r + 1) square (samples outside the window ignored)
fn dilate_sym(m: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let b = morph_op(m, w, h, r + 1, Op::DilateBack);
    morph_op(&b, w, h, r + 1, Op::DilateFwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sincos_matches_libm() {
        for d in [
            -725.0, -90.0, -30.0, 0.0, 12.5, 30.0, 45.0, 89.0, 90.0, 135.0, 180.0, 271.0, 359.9,
        ] {
            let (s, c) = sincos_deg(d);
            let r = (d as f64).to_radians();
            assert!((s - r.sin()).abs() < 1e-15, "{d}");
            assert!((c - r.cos()).abs() < 1e-15, "{d}");
        }
        assert_eq!(sincos_deg(90.0), (1.0, 0.0));
        assert_eq!(sincos_deg(0.0), (0.0, 1.0));
    }

    #[test]
    fn fast_morphology_equals_naive() {
        let (w, h) = (53, 37);
        let mut st = 99u64;
        let m: Vec<bool> = (0..w * h)
            .map(|_| {
                st = mix(st);
                st % 3 == 0
            })
            .collect();
        for k in 1..6 {
            for (op, dil, fwd) in [
                (Op::DilateBack, true, false),
                (Op::DilateFwd, true, true),
                (Op::ErodeFwd, false, true),
            ] {
                let got = morph_op(&m, w, h, k, op);
                for y in 0..h {
                    for x in 0..w {
                        let mut acc = !dil;
                        for dy in 0..k {
                            for dx in 0..k {
                                let (xx, yy) = if fwd {
                                    (x as i64 + dx as i64, y as i64 + dy as i64)
                                } else {
                                    (x as i64 - dx as i64, y as i64 - dy as i64)
                                };
                                if xx < 0 || yy < 0 || xx >= w as i64 || yy >= h as i64 {
                                    continue;
                                }
                                let v = m[yy as usize * w + xx as usize];
                                acc = if dil { acc || v } else { acc && v };
                            }
                        }
                        assert_eq!(got[y * w + x], acc, "k {k} at {x},{y}");
                    }
                }
            }
            let r = k - 1;
            let ds = dilate_sym(&m, w, h, r);
            for y in 0..h {
                for x in 0..w {
                    let want = (y.saturating_sub(r)..(y + r + 1).min(h)).any(|yy| {
                        (x.saturating_sub(r)..(x + r + 1).min(w)).any(|xx| m[yy * w + xx])
                    });
                    assert_eq!(ds[y * w + x], want);
                }
            }
        }
    }

    /// the bounded flood fill classifies exactly like full component labelling
    #[test]
    fn dot_classes_equal_full_labelling() {
        use crate::filters::label;
        let s = Settings::default(); // 300 dpi, 35 lpi: dots < 58.8 px, span 13
        let (amax, span) = dot_limits(&s);
        let (w, h) = (120, 90);
        let mut st = 7u64;
        let mut rnd = |n: u64| {
            st = mix(st);
            st % n
        };
        for round in 0..20 {
            // ink: random discs of several sizes + a solid band; mask: random blobs
            let mut ink = vec![false; w * h];
            let mut mask = vec![false; w * h];
            for _ in 0..40 {
                let (cx, cy, r) = (
                    rnd(w as u64) as i64,
                    rnd(h as u64) as i64,
                    1 + rnd(7) as i64,
                );
                for y in 0..h as i64 {
                    for x in 0..w as i64 {
                        if (x - cx).pow(2) + (y - cy).pow(2) <= r * r {
                            ink[y as usize * w + x as usize] = true;
                        }
                    }
                }
            }
            for x in 0..w {
                for y in 40 + round..50 + round {
                    ink[y * w + x] = true;
                }
            }
            for _ in 0..25 {
                let (cx, cy, r) = (
                    rnd(w as u64) as i64,
                    rnd(h as u64) as i64,
                    2 + rnd(9) as i64,
                );
                for y in 0..h as i64 {
                    for x in 0..w as i64 {
                        if (x - cx).pow(2) + (y - cy).pow(2) <= r * r {
                            mask[y as usize * w + x as usize] = true;
                        }
                    }
                }
            }
            let mut o: Vec<u8> = ink
                .iter()
                .flat_map(|&v| if v { [255u8; 4] } else { [0u8; 4] })
                .collect();
            knock_out(&mut o, w, h, &mask, &s);
            // reference: label everything
            let (lab, sizes) = label(&ink, w, h);
            let nl = sizes.len();
            let mut hits = vec![0u32; nl];
            let mut bb = vec![[usize::MAX, usize::MAX, 0, 0]; nl];
            let mut edge = vec![false; nl];
            for p in 0..w * h {
                let l = lab[p] as usize;
                if l == 0 {
                    continue;
                }
                let (x, y) = (p % w, p / w);
                hits[l] += mask[p] as u32;
                bb[l] = [
                    bb[l][0].min(x),
                    bb[l][1].min(y),
                    bb[l][2].max(x),
                    bb[l][3].max(y),
                ];
                edge[l] |= x == 0 || y == 0 || x + 1 == w || y + 1 == h;
            }
            let mut cut = vec![false; w * h];
            let mut left = vec![false; w * h];
            for p in 0..w * h {
                let l = lab[p] as usize;
                if l == 0 {
                    continue;
                }
                let b = bb[l];
                let dot = (sizes[l] as f64) < amax
                    && b[2] - b[0] < span
                    && b[3] - b[1] < span
                    && !edge[l];
                if dot {
                    left[p] = 2 * hits[l] <= sizes[l];
                } else {
                    cut[p] = mask[p];
                    left[p] = !mask[p];
                }
            }
            let k = mindot_px(s.dpi);
            if cut.iter().any(|&c| c) {
                let opened = morph(&morph(&left, w, h, k, false), w, h, k, true);
                let near = dilate_sym(&cut, w, h, k);
                for p in 0..w * h {
                    if left[p] && !opened[p] && near[p] {
                        left[p] = false;
                    }
                }
            }
            for p in 0..w * h {
                assert_eq!(o[p * 4 + 3] == 255, left[p], "round {round} pixel {p}");
            }
        }
    }

    #[test]
    fn lenient_layer_json() {
        let l: Layer = serde_json::from_str(
            r#"{"kind":"gambar","path":"lib:x.jpg","threshold":"0,7","zoom":null,"mirror":"ya","offset_mm":[null,"2"],"mode":"???"}"#,
        )
        .unwrap();
        let p = l.params();
        assert_eq!(p.mode, Mode::Lubang);
        assert_eq!(p.threshold, 0.7);
        assert_eq!(p.zoom, 1.0);
        assert!(p.mirror);
        assert_eq!(p.offset_mm, [0.0, 2.0]);
        assert!(p.active());
        // layers of the removed procedural looks still parse and are ignored
        let old: Layer = serde_json::from_str(r#"{"kind":"retak","amount":0.3,"seed":4}"#).unwrap();
        assert_eq!(old.kind, Kind::Lama);
        assert!(!old.params().active());
        let g: Layer = serde_json::from_str(r#"{"kind":"gambar","scale_mm":-1,"threshold":7}"#).unwrap();
        let p = g.params();
        assert_eq!(p.scale_mm, None);
        assert_eq!(p.threshold, 1.0);
        assert!(!p.active()); // no file
    }
}
