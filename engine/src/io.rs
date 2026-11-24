//! Load images as float colour + alpha, write the print PNG (RGBA8 + pHYs DPI) after
//! the mandatory checks.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::dtf::minarea_px;
use crate::filters::label;

pub struct Loaded {
    pub w: usize,
    pub h: usize,
    pub rgb: Vec<f32>,
    pub alpha: Vec<f32>,
    pub has_alpha: bool,
}

/// 8-bit RGBA image as decoded (4 bytes per pixel; the app keeps this, not floats)
pub struct Rgba8 {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
    pub has_alpha: bool,
}

impl Rgba8 {
    /// the float image `decode` would give (value / 255 as f32, alpha 1 without alpha)
    pub fn to_loaded(&self) -> Loaded {
        let n = self.w * self.h;
        let mut rgb = Vec::with_capacity(n * 3);
        let mut alpha = Vec::with_capacity(n);
        for px in self.px.chunks_exact(4) {
            rgb.push(px[0] as f32 / 255.0);
            rgb.push(px[1] as f32 / 255.0);
            rgb.push(px[2] as f32 / 255.0);
            alpha.push(if self.has_alpha { px[3] as f32 / 255.0 } else { 1.0 });
        }
        Loaded {
            w: self.w,
            h: self.h,
            rgb,
            alpha,
            has_alpha: self.has_alpha,
        }
    }
}

impl Loaded {
    /// back to 8-bit RGBA (rounded)
    pub fn to_rgba8(&self) -> Rgba8 {
        let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let px = (0..self.w * self.h)
            .flat_map(|p| [to8(self.rgb[p * 3]), to8(self.rgb[p * 3 + 1]), to8(self.rgb[p * 3 + 2]), to8(self.alpha[p])])
            .collect();
        Rgba8 { w: self.w, h: self.h, px, has_alpha: self.has_alpha }
    }
}

fn open_image(bytes: &[u8]) -> Result<image::DynamicImage> {
    // the default 512 MiB decoder limit refuses 16-bit or very large print files
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .context("cannot read image")?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(6 << 30);
    reader.limits(limits);
    reader.decode().context("cannot decode image")
}

/// decode to 8-bit RGBA (4 bytes per pixel)
pub fn load_rgba8(path: &Path) -> Result<Rgba8> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    let img = open_image(&bytes)?;
    drop(bytes);
    let has_alpha = img.color().has_alpha();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let px = img.into_rgba8().into_raw();
    Ok(Rgba8 { w, h, px, has_alpha })
}

/// like `load` in studiozea_ref.py: 8-bit decode, value / 255 as f32
pub fn decode(bytes: &[u8]) -> Result<Loaded> {
    let img = open_image(bytes)?;
    let (w, h) = (img.width() as usize, img.height() as usize);
    let has_alpha = img.color().has_alpha();
    let mut rgb = Vec::with_capacity(w * h * 3);
    let mut alpha = Vec::with_capacity(w * h);
    if has_alpha {
        for px in img.to_rgba8().pixels() {
            rgb.push(px[0] as f32 / 255.0);
            rgb.push(px[1] as f32 / 255.0);
            rgb.push(px[2] as f32 / 255.0);
            alpha.push(px[3] as f32 / 255.0);
        }
    } else {
        for px in img.to_rgb8().pixels() {
            rgb.push(px[0] as f32 / 255.0);
            rgb.push(px[1] as f32 / 255.0);
            rgb.push(px[2] as f32 / 255.0);
        }
        alpha.resize(w * h, 1.0);
    }
    Ok(Loaded {
        w,
        h,
        rgb,
        alpha,
        has_alpha,
    })
}

pub fn load(path: &Path) -> Result<Loaded> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    decode(&bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Checks {
    pub semi: usize,
    pub impure: usize,
    pub tiny: usize,
    pub hidden: usize,
}

impl Checks {
    pub fn ok(&self) -> bool {
        self.semi == 0 && self.impure == 0 && self.tiny == 0 && self.hidden == 0
    }
}

pub fn checks(o: &[u8], w: usize, h: usize, dpi: f64) -> Checks {
    checks_for(o, w, h, dpi, true)
}

/// `pure` = false for true-colour output (Settings::pure_colours): colours not counted
pub fn checks_for(o: &[u8], w: usize, h: usize, dpi: f64, pure: bool) -> Checks {
    let n = w * h;
    let op: Vec<bool> = (0..n).map(|p| o[p * 4 + 3] == 255).collect();
    let (_, sizes) = label(&op, w, h);
    let amin = minarea_px(dpi);
    let mut c = Checks {
        semi: 0,
        impure: 0,
        tiny: sizes.iter().skip(1).filter(|&&s| s < amin).count(),
        hidden: 0,
    };
    for p in 0..n {
        let a = o[p * 4 + 3];
        if a != 0 && a != 255 {
            c.semi += 1;
        }
        let rgb = &o[p * 4..p * 4 + 3];
        if pure && a == 255 && rgb.iter().any(|&v| v != 0 && v != 255) {
            c.impure += 1;
        }
        if a != 255 {
            c.hidden += rgb.iter().filter(|&&v| v != 0).count();
        }
    }
    c
}

/// write RGBA8 PNG with the DPI in pHYs; refuses files that fail the checks
pub fn save_png(o: &[u8], w: usize, h: usize, dpi: f64, path: &Path) -> Result<()> {
    save_png_for(o, w, h, dpi, path, true)
}

/// `save_png` for output that may be true colour (`pure` = Settings::pure_colours)
pub fn save_png_for(o: &[u8], w: usize, h: usize, dpi: f64, path: &Path, pure: bool) -> Result<()> {
    let ch = checks_for(o, w, h, dpi, pure);
    if ch.semi > 0 || ch.impure > 0 || ch.hidden > 0 {
        bail!("refusing to write {}: {:?}", path.display(), ch);
    }
    write_png_rows(w, h, dpi, path, |y0, y1| Ok(o[y0 * w * 4..y1 * w * 4].to_vec()))
}

/// write the print PNG from a packed result (streamed, no full RGBA buffer);
/// `checks` must be the checks of exactly these pixels
pub fn save_png_packed(p: &crate::band::Packed, checks: &Checks, dpi: f64, path: &Path) -> Result<()> {
    if checks.semi > 0 || checks.impure > 0 || checks.hidden > 0 {
        bail!("refusing to write {}: {:?}", path.display(), checks);
    }
    write_png_rows(p.w, p.h, dpi, path, |y0, y1| Ok(p.rows(y0, y1)))
}

/// margin in mm -> whole print pixels
pub fn margin_px(mm: f64, dpi: f64) -> usize {
    if mm > 0.0 {
        // at most 1 m (callers allow 100 mm)
        (mm.min(1000.0) / 25.4 * dpi).round() as usize
    } else {
        0
    }
}

/// `save_png_packed` with `pad` transparent pixels on every side
pub fn save_png_packed_margin(p: &crate::band::Packed, checks: &Checks, dpi: f64, path: &Path, pad: usize) -> Result<()> {
    if pad == 0 {
        return save_png_packed(p, checks, dpi, path);
    }
    if checks.semi > 0 || checks.impure > 0 || checks.hidden > 0 {
        bail!("refusing to write {}: {:?}", path.display(), checks);
    }
    let (w, h) = (p.w + 2 * pad, p.h + 2 * pad);
    write_png_rows(w, h, dpi, path, |y0, y1| {
        let mut out = vec![0u8; (y1 - y0) * w * 4];
        let (a, b) = (y0.max(pad), y1.min(pad + p.h));
        if a < b {
            let inner = p.rows(a - pad, b - pad);
            for y in a..b {
                let src = &inner[(y - a) * p.w * 4..(y - a + 1) * p.w * 4];
                let o = ((y - y0) * w + pad) * 4;
                out[o..o + p.w * 4].copy_from_slice(src);
            }
        }
        Ok(out)
    })
}

/// RGBA8 PNG with pHYs, rows produced in chunks by `rows(y0, y1)` (an error stops the
/// export). Written to `<path>.part` first and renamed at the end, so a failed or
/// cancelled export never leaves a broken file or destroys the file it would replace.
pub fn write_png_rows(
    w: usize,
    h: usize,
    dpi: f64,
    path: &Path,
    rows: impl FnMut(usize, usize) -> Result<Vec<u8>>,
) -> Result<()> {
    if w == 0 || h == 0 || w > u32::MAX as usize || h > u32::MAX as usize {
        bail!("cannot write a {w} x {h} PNG");
    }
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = std::path::PathBuf::from(part);
    let r = write_png_file(w, h, dpi, png::ColorType::Rgba, &part, rows).and_then(|_| {
        std::fs::rename(&part, path).with_context(|| format!("cannot write {}", path.display()))
    });
    if r.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    r
}

/// `write_png_rows` for an 8-bit grayscale image (films)
pub fn write_png_gray_rows(
    w: usize,
    h: usize,
    dpi: f64,
    path: &Path,
    rows: impl FnMut(usize, usize) -> Result<Vec<u8>>,
) -> Result<()> {
    if w == 0 || h == 0 || w > u32::MAX as usize || h > u32::MAX as usize {
        bail!("cannot write a {w} x {h} PNG");
    }
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = std::path::PathBuf::from(part);
    let r = write_png_file(w, h, dpi, png::ColorType::Grayscale, &part, rows).and_then(|_| {
        std::fs::rename(&part, path).with_context(|| format!("cannot write {}", path.display()))
    });
    if r.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    r
}

fn write_png_file(
    w: usize,
    h: usize,
    dpi: f64,
    color: png::ColorType,
    path: &Path,
    mut rows: impl FnMut(usize, usize) -> Result<Vec<u8>>,
) -> Result<()> {
    let file = File::create(path).with_context(|| format!("cannot create {}", path.display()))?;
    let mut enc = png::Encoder::new(BufWriter::new(file), w as u32, h as u32);
    enc.set_color(color);
    enc.set_depth(png::BitDepth::Eight);
    let ppm = (dpi / 0.0254 + 0.5) as u32;
    enc.set_pixel_dims(Some(png::PixelDimensions {
        xppu: ppm,
        yppu: ppm,
        unit: png::Unit::Meter,
    }));
    let mut writer = enc.write_header()?;
    let mut stream = writer.stream_writer()?;
    use std::io::Write;
    let chunk = (1 << 22) / (w * color.samples()).max(1) + 1;
    let mut y0 = 0;
    while y0 < h {
        let y1 = (y0 + chunk).min(h);
        stream.write_all(&rows(y0, y1)?)?;
        y0 = y1;
    }
    stream.finish()?;
    Ok(())
}

/// save a decoded / upscaled image as PNG: RGB when it has no alpha channel, RGBA
/// otherwise (no DPI: a source image, not a print). Written to `<path>.part` first.
pub fn save_rgba8(img: &Rgba8, path: &Path) -> Result<()> {
    let (w, h) = (img.w, img.h);
    if w == 0 || h == 0 || w > u32::MAX as usize || h > u32::MAX as usize {
        bail!("cannot write a {w} x {h} PNG");
    }
    let color = if img.has_alpha { png::ColorType::Rgba } else { png::ColorType::Rgb };
    let mut part = path.as_os_str().to_owned();
    part.push(".part");
    let part = std::path::PathBuf::from(part);
    let rows = |y0: usize, y1: usize| -> Result<Vec<u8>> {
        let px = &img.px[y0 * w * 4..y1 * w * 4];
        Ok(if img.has_alpha { px.to_vec() } else { px.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect() })
    };
    let r = write_png_file(w, h, 72.0, color, &part, rows).and_then(|_| {
        std::fs::rename(&part, path).with_context(|| format!("cannot write {}", path.display()))
    });
    if r.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    r
}

/// encode to PNG bytes (preview / tests)
pub fn encode_png(o: &[u8], w: usize, h: usize, dpi: f64) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut buf, w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        let ppm = (dpi / 0.0254 + 0.5) as u32;
        enc.set_pixel_dims(Some(png::PixelDimensions {
            xppu: ppm,
            yppu: ppm,
            unit: png::Unit::Meter,
        }));
        let mut wr = enc.write_header()?;
        wr.write_image_data(o)?;
    }
    Ok(buf)
}
