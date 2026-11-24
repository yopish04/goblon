//! "Lindungi teks": lettering stays solid (its own true colours, like a spot colour)
//! while the rest of the design is screened, so small text does not break up into dots.
//!
//! 1. `detect` finds text with the PP-OCRv4 detector (PaddleOCR, Apache-2.0, see
//!    models/LICENSE-PaddleOCR; run with tract, pure Rust): text probability maps at
//!    1280 px and 640 px (big logos), words = connected parts above 0.3 with a mean
//!    above 0.6, grown by DB's "unclip" distance (area x 1.8 / perimeter) to cover the
//!    whole letters; a big word from 640 px replaces the pieces found at 1280 px.
//! 2. `solid_mask` decides per source pixel inside those regions which pixels are the
//!    letters (per word: Otsu split of the colours, the part that holds the region's
//!    border is the background). Where the surroundings are transparent (background
//!    removed), every opaque pixel of the region counts.
//! 3. The mask follows crop / trim / turn and is resized with the print; `render_at`
//!    prints masked pixels solid (see render.rs). Manual strokes add or remove text.
//!
//! Detection runs once per image (the app caches it); everything is deterministic.

use std::sync::{Arc, OnceLock};

use anyhow::{anyhow, Result};
use tract_onnx::prelude::*;

use crate::io::Rgba8;

static MODEL: &[u8] = include_bytes!("../models/ppocrv4-det.onnx");
/// detector input sides: small lettering at 1280 px, big lettering (logos that fill the
/// design, which the detector breaks up at 1280) at 640 px
const SIDE: usize = 1280;
const COARSE: usize = 640;
const THRESH: f32 = 0.3;
const BOX_THRESH: f32 = 0.6;
const UNCLIP: f32 = 1.8;

type Loaded = OnceLock<std::result::Result<Arc<TypedRunnableModel>, String>>;

fn model(side: usize) -> Result<Arc<TypedRunnableModel>> {
    static FINE: Loaded = OnceLock::new();
    static COARSE_M: Loaded = OnceLock::new();
    let m = if side == SIDE { &FINE } else { &COARSE_M };
    m.get_or_init(|| {
        tract_onnx::onnx()
            .model_for_read(&mut &MODEL[..])
            .and_then(|m| m.with_input_fact(0, f32::fact([1, 3, side, side]).into()))
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(|e| format!("{e:#}"))
    })
    .clone()
    .map_err(|e| anyhow!("detektor teks tidak bisa dimuat: {e}"))
}

/// where the text is, at detector resolution (`dw` x `dh` covers the whole image)
#[derive(Clone, Debug)]
pub struct TextMap {
    /// source size
    pub w: usize,
    pub h: usize,
    pub dw: usize,
    pub dh: usize,
    /// region label per detector pixel (0 = no text, 1.. = word)
    pub label: Vec<u16>,
    /// number of words
    pub words: usize,
}

impl TextMap {
    /// word label at source pixel (x, y)
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> u16 {
        let dx = (x * self.dw / self.w).min(self.dw - 1);
        let dy = (y * self.dh / self.h).min(self.dh - 1);
        self.label[dy * self.dw + dx]
    }
}

/// find the text of an image (transparent parts on mid grey): words at 1280 px, plus the
/// big words found at 640 px that the fine pass broke up (see `merge`)
pub fn detect(img: &Rgba8) -> Result<TextMap> {
    let (w, h) = (img.w, img.h);
    if w == 0 || h == 0 {
        return Err(anyhow!("gambar kosong"));
    }
    // fit in SIDE; small images are enlarged (at most 2x) like PaddleOCR does, so the
    // lettering of a small logo is big enough for the detector
    let (p, dw, dh) = probability(img, SIDE, 2.0)?;
    let (fine, _) = regions(&p, dw, dh);
    let (p, cw, ch) = probability(img, COARSE, 1.0)?;
    let (coarse, _) = regions(&p, cw, ch);
    let (label, words) = merge(&fine, dw, dh, &coarse, cw, ch);
    Ok(TextMap { w, h, dw, dh, label, words })
}

/// text probability at `side` px (image fitted in, enlarged at most `grow` times)
fn probability(img: &Rgba8, side: usize, grow: f64) -> Result<(Vec<f32>, usize, usize)> {
    let (w, h) = (img.w, img.h);
    let r = (side as f64 / w.max(h) as f64).min(grow);
    let (dw, dh) = (((w as f64 * r).round() as usize).clamp(1, side), ((h as f64 * r).round() as usize).clamp(1, side));
    // box-average down to dw x dh (nearest when enlarging; alpha over grey), normalised
    // exactly like PaddleOCR: the BGR image with mean / std listed in that order
    let mean = [0.485f32, 0.456, 0.406];
    let std = [0.229f32, 0.224, 0.225];
    let mut x = tract_ndarray::Array4::<f32>::zeros((1, 3, side, side));
    for dy in 0..dh {
        let (y0, y1) = (dy * h / dh, ((dy + 1) * h / dh).max(dy * h / dh + 1));
        for dx in 0..dw {
            let (x0, x1) = (dx * w / dw, ((dx + 1) * w / dw).max(dx * w / dw + 1));
            let mut s = [0f32; 3];
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let p = &img.px[(yy * w + xx) * 4..][..4];
                    let a = if img.has_alpha { p[3] as f32 / 255.0 } else { 1.0 };
                    for c in 0..3 {
                        s[c] += p[c] as f32 / 255.0 * a + 0.5 * (1.0 - a);
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as f32;
            for c in 0..3 {
                // channel 0 = blue
                x[[0, c, dy, dx]] = (s[2 - c] / n - mean[c]) / std[c];
            }
        }
    }
    let out = model(side)?.run(tvec!(Tensor::from(x).into())).map_err(|e| anyhow!("deteksi teks gagal: {e:#}"))?;
    let prob = out[0].to_plain_array_view::<f32>().map_err(|e| anyhow!("{e:#}"))?;
    Ok(((0..dh).flat_map(|y| (0..dw).map(move |x| (y, x))).map(|(y, x)| prob[[0, 0, y, x]]).collect(), dw, dh))
}

/// a coarse word replaces the fine words it overlaps when it is clearly taller than them
/// (big lettering the fine pass broke into pieces) or overlaps none; fine words mostly
/// inside such a coarse word go. Returns labels on the fine grid and the word count.
fn merge(fine: &[u16], dw: usize, dh: usize, coarse: &[u16], cw: usize, ch: usize) -> (Vec<u16>, usize) {
    let nf = fine.iter().copied().max().unwrap_or(0) as usize;
    let nc = coarse.iter().copied().max().unwrap_or(0) as usize;
    let at_c = |x: usize, y: usize| coarse[(y * ch / dh).min(ch - 1) * cw + (x * cw / dw).min(cw - 1)] as usize;
    // heights (fine pixels), areas, overlaps
    let (mut f_top, mut f_bot, mut f_area) = (vec![usize::MAX; nf + 1], vec![0usize; nf + 1], vec![0usize; nf + 1]);
    let (mut c_top, mut c_bot) = (vec![usize::MAX; nc + 1], vec![0usize; nc + 1]);
    let mut over = std::collections::HashMap::<(usize, usize), usize>::new();
    for y in 0..dh {
        for x in 0..dw {
            let f = fine[y * dw + x] as usize;
            let c = at_c(x, y);
            if f > 0 {
                f_top[f] = f_top[f].min(y);
                f_bot[f] = f_bot[f].max(y + 1);
                f_area[f] += 1;
            }
            if c > 0 {
                c_top[c] = c_top[c].min(y);
                c_bot[c] = c_bot[c].max(y + 1);
                if f > 0 {
                    *over.entry((c, f)).or_default() += 1;
                }
            }
        }
    }
    let mut wins = vec![false; nc + 1];
    let mut gone = vec![false; nf + 1];
    for c in 1..=nc {
        if c_top[c] == usize::MAX {
            continue;
        }
        let hc = c_bot[c] - c_top[c];
        let tallest = over.keys().filter(|k| k.0 == c).map(|k| f_bot[k.1] - f_top[k.1]).max().unwrap_or(0);
        if tallest == 0 || hc * 10 > tallest * 16 {
            wins[c] = true;
            for (&(cc, f), &n) in &over {
                if cc == c && n * 2 >= f_area[f] {
                    gone[f] = true;
                }
            }
        }
    }
    // renumber: kept fine words, then winning coarse words painted over them
    let mut id_f = vec![0u16; nf + 1];
    let mut id_c = vec![0u16; nc + 1];
    let mut next = 0usize;
    for f in 1..=nf {
        if !gone[f] && f_area[f] > 0 && next < u16::MAX as usize - 1 {
            next += 1;
            id_f[f] = next as u16;
        }
    }
    for c in 1..=nc {
        if wins[c] && next < u16::MAX as usize - 1 {
            next += 1;
            id_c[c] = next as u16;
        }
    }
    let mut label = vec![0u16; dw * dh];
    let mut used = vec![false; next + 1];
    for y in 0..dh {
        for x in 0..dw {
            let i = y * dw + x;
            let c = id_c[at_c(x, y)];
            let l = if c > 0 { c } else { id_f[fine[i] as usize] };
            label[i] = l;
            used[l as usize] = true;
        }
    }
    // labels must be 1..=words without gaps (fine words fully painted over vanish)
    let mut remap = vec![0u16; next + 1];
    let mut words = 0usize;
    for l in 1..=next {
        if used[l] {
            words += 1;
            remap[l] = words as u16;
        }
    }
    for v in label.iter_mut() {
        *v = remap[*v as usize];
    }
    (label, words)
}

/// DB post-processing: words (prob > THRESH, mean > BOX_THRESH, at least 3 px each way),
/// each grown by the unclip distance; returns labels per pixel and the word count
fn regions(p: &[f32], w: usize, h: usize) -> (Vec<u16>, usize) {
    let on: Vec<bool> = p.iter().map(|&v| v > THRESH).collect();
    let (comp, sizes) = crate::filters::label(&on, w, h);
    let n = sizes.len().saturating_sub(1);
    let mut area = vec![0usize; n + 1];
    let mut sum = vec![0f32; n + 1];
    let mut edge = vec![0usize; n + 1];
    let mut bb = vec![[usize::MAX, usize::MAX, 0, 0]; n + 1];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let l = comp[i] as usize;
            if l == 0 {
                continue;
            }
            area[l] += 1;
            sum[l] += p[i];
            let b = &mut bb[l];
            b[0] = b[0].min(x);
            b[1] = b[1].min(y);
            b[2] = b[2].max(x + 1);
            b[3] = b[3].max(y + 1);
            let border = x == 0 || y == 0 || x + 1 == w || y + 1 == h || !on[i - 1] || !on[i + 1] || !on[i - w] || !on[i + w];
            if border {
                edge[l] += 1;
            }
        }
    }
    let mut label = vec![0u16; w * h];
    let mut words = 0usize;
    for l in 1..=n {
        let b = bb[l];
        if area[l] == 0 || sum[l] / (area[l] as f32) < BOX_THRESH || b[2] - b[0] < 3 || b[3] - b[1] < 3 || words >= u16::MAX as usize - 1 {
            continue;
        }
        words += 1;
        let d = (area[l] as f32 * UNCLIP / edge[l].max(1) as f32).max(1.0);
        let r = d.ceil() as usize;
        let (x0, y0, x1, y1) = (b[0].saturating_sub(r), b[1].saturating_sub(r), (b[2] + r).min(w), (b[3] + r).min(h));
        // distance (chamfer 3-4, / 3 = pixels) to the word inside its grown box
        let (bw, bh) = (x1 - x0, y1 - y0);
        let mut dist = vec![u32::MAX / 2; bw * bh];
        for y in y0..y1 {
            for x in x0..x1 {
                if comp[y * w + x] as usize == l {
                    dist[(y - y0) * bw + x - x0] = 0;
                }
            }
        }
        chamfer(&mut dist, bw, bh);
        let lim = (d * 3.0) as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = y * w + x;
                if dist[(y - y0) * bw + x - x0] <= lim && label[i] == 0 {
                    label[i] = words as u16;
                }
            }
        }
    }
    (label, words)
}

/// two-pass chamfer distance (3 straight, 4 diagonal) in place
fn chamfer(d: &mut [u32], w: usize, h: usize) {
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let mut v = d[i];
            if x > 0 {
                v = v.min(d[i - 1] + 3);
            }
            if y > 0 {
                v = v.min(d[i - w] + 3);
                if x > 0 {
                    v = v.min(d[i - w - 1] + 4);
                }
                if x + 1 < w {
                    v = v.min(d[i - w + 1] + 4);
                }
            }
            d[i] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            let i = y * w + x;
            let mut v = d[i];
            if x + 1 < w {
                v = v.min(d[i + 1] + 3);
            }
            if y + 1 < h {
                v = v.min(d[i + w] + 3);
                if x + 1 < w {
                    v = v.min(d[i + w + 1] + 4);
                }
                if x > 0 {
                    v = v.min(d[i + w - 1] + 4);
                }
            }
            d[i] = v;
        }
    }
}

/// per source pixel 1.0 = print solid (letters), from the detected words and the
/// prepared colours (`rgb` interleaved, `alpha`, after background removal), w x h =
/// source size.
///
/// Per word (like OCR binarisation): the opaque pixels of its region are split in two
/// by Otsu's threshold along their main colour axis; the part that makes up most of the
/// region's border is the background, the other part the letters. A region whose border
/// is mostly transparent (background removed) keeps all its opaque pixels; a region
/// without contrast keeps nothing (nothing to tell apart).
pub fn solid_mask(map: &TextMap, rgb: &[f32], alpha: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0f32; w * h];
    if map.words == 0 || map.w != w || map.h != h {
        return out;
    }
    let (dw, dh) = (map.dw, map.dh);
    let n = map.words;
    // per word: source bounding box; detector cells on the region border
    let mut bb = vec![[usize::MAX, usize::MAX, 0usize, 0usize]; n + 1];
    let mut border = vec![false; dw * dh];
    for dy in 0..dh {
        for dx in 0..dw {
            let i = dy * dw + dx;
            let l = map.label[i];
            if l == 0 {
                continue;
            }
            let b = &mut bb[l as usize];
            b[0] = b[0].min(dx * w / dw);
            b[1] = b[1].min(dy * h / dh);
            b[2] = b[2].max(((dx + 1) * w).div_ceil(dw).min(w));
            b[3] = b[3].max(((dy + 1) * h).div_ceil(dh).min(h));
            border[i] = dx == 0 || dy == 0 || dx + 1 == dw || dy + 1 == dh || map.label[i - 1] != l || map.label[i + 1] != l || map.label[i - dw] != l || map.label[i + dw] != l;
        }
    }
    let cell = |x: usize, y: usize| (y * dh / h).min(dh - 1) * dw + (x * dw / w).min(dw - 1);
    for l in 1..=n {
        let b = bb[l];
        if b[0] >= b[2] || b[1] >= b[3] {
            continue;
        }
        let (mut px, mut edge) = (Vec::new(), Vec::new());
        let (mut edge_all, mut edge_clear) = (0usize, 0usize);
        for y in b[1]..b[3] {
            for x in b[0]..b[2] {
                let c = cell(x, y);
                if map.label[c] as usize != l {
                    continue;
                }
                let i = y * w + x;
                let on_edge = border[c];
                edge_all += on_edge as usize;
                if alpha[i] < 0.5 {
                    edge_clear += on_edge as usize;
                    continue;
                }
                px.push(i);
                edge.push(on_edge);
            }
        }
        if px.is_empty() {
            continue;
        }
        if edge_clear * 10 >= edge_all * 7 {
            // background removed around the word: every opaque pixel is lettering
            for &i in &px {
                out[i] = 1.0;
            }
            continue;
        }
        let Some(rule) = split(&px, &edge, rgb) else { continue };
        for &i in &px {
            if rule.is_text(&rgb[i * 3..i * 3 + 3]) {
                out[i] = 1.0;
            }
        }
    }
    // anti-aliased letter edges: one pixel around the letters (opaque, in a word region)
    let core: Vec<bool> = out.iter().map(|&v| v > 0.0).collect();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if core[i] || alpha[i] < 0.5 || map.label[cell(x, y)] == 0 {
                continue;
            }
            let nb = (x > 0 && core[i - 1]) || (x + 1 < w && core[i + 1]) || (y > 0 && core[i - w]) || (y + 1 < h && core[i + w]);
            if nb {
                out[i] = 1.0;
            }
        }
    }
    out
}

/// two classes of the word's pixels must differ at least this much (mean colours,
/// largest channel difference) to tell letters from background
const MIN_CONTRAST: f32 = 0.12;

/// how one word tells letters from background: the side of an Otsu threshold along the
/// main colour axis of its pixels
struct Rule {
    mean: [f64; 3],
    axis: [f64; 3],
    lo: f64,
    hi: f64,
    t: usize,
    text_high: bool,
}

impl Rule {
    fn bin(&self, c: &[f32]) -> usize {
        let p = (c[0] as f64 - self.mean[0]) * self.axis[0] + (c[1] as f64 - self.mean[1]) * self.axis[1] + (c[2] as f64 - self.mean[2]) * self.axis[2];
        ((p - self.lo) / (self.hi - self.lo) * 255.0).clamp(0.0, 255.0) as usize
    }
    fn is_text(&self, c: &[f32]) -> bool {
        (self.bin(c) > self.t) == self.text_high
    }
}

/// the rule for the pixels `px` (indices into `rgb`) of one word: Otsu split along the
/// main colour axis; the class holding most of the border pixels is the background
fn split(px: &[usize], edge: &[bool], rgb: &[f32]) -> Option<Rule> {
    let n = px.len() as f64;
    let col = |i: usize| [rgb[i * 3] as f64, rgb[i * 3 + 1] as f64, rgb[i * 3 + 2] as f64];
    let mut mean = [0f64; 3];
    for &i in px {
        let c = col(i);
        for k in 0..3 {
            mean[k] += c[k] / n;
        }
    }
    let mut cov = [[0f64; 3]; 3];
    for &i in px {
        let c = col(i);
        let d = [c[0] - mean[0], c[1] - mean[1], c[2] - mean[2]];
        for a in 0..3 {
            for b in 0..3 {
                cov[a][b] += d[a] * d[b] / n;
            }
        }
    }
    // main axis by power iteration (deterministic start)
    let mut v = [0.577f64, 0.577, 0.577];
    for _ in 0..32 {
        let m = [0, 1, 2].map(|a| cov[a][0] * v[0] + cov[a][1] * v[1] + cov[a][2] * v[2]);
        let len = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
        if len < 1e-12 {
            return None; // one flat colour
        }
        v = [m[0] / len, m[1] / len, m[2] / len];
    }
    let proj: Vec<f64> = px.iter().map(|&i| {
        let c = col(i);
        (c[0] - mean[0]) * v[0] + (c[1] - mean[1]) * v[1] + (c[2] - mean[2]) * v[2]
    }).collect();
    let (lo, hi) = proj.iter().fold((f64::MAX, f64::MIN), |(a, b), &p| (a.min(p), b.max(p)));
    if hi - lo < 1e-6 {
        return None;
    }
    let bin = |p: f64| ((p - lo) / (hi - lo) * 255.0).clamp(0.0, 255.0) as usize;
    let mut hist = [0f64; 256];
    for &p in &proj {
        hist[bin(p)] += 1.0;
    }
    // Otsu
    let total: f64 = hist.iter().enumerate().map(|(i, c)| i as f64 * c).sum();
    let (mut wb, mut sb, mut best, mut t) = (0f64, 0f64, -1f64, 0usize);
    for (i, &c) in hist.iter().enumerate() {
        wb += c;
        if wb == 0.0 || wb == n {
            continue;
        }
        sb += i as f64 * c;
        let (mb, mf) = (sb / wb, (total - sb) / (n - wb));
        let between = wb * (n - wb) * (mb - mf) * (mb - mf);
        if between > best {
            (best, t) = (between, i);
        }
    }
    let high: Vec<bool> = proj.iter().map(|&p| bin(p) > t).collect();
    // contrast between the two classes
    let mut m = [[0f64; 3]; 2];
    let mut cnt = [0f64; 2];
    for (k, &i) in px.iter().enumerate() {
        let c = col(i);
        let j = high[k] as usize;
        cnt[j] += 1.0;
        for a in 0..3 {
            m[j][a] += c[a];
        }
    }
    if cnt[0] == 0.0 || cnt[1] == 0.0 {
        return None;
    }
    let diff = (0..3).map(|a| (m[0][a] / cnt[0] - m[1][a] / cnt[1]).abs()).fold(0.0, f64::max);
    if diff < MIN_CONTRAST as f64 {
        return None;
    }
    // background = the class with the larger share of the border
    let (mut eb, mut et) = ([0f64; 2], [0f64; 2]);
    for (k, &e) in edge.iter().enumerate() {
        let j = high[k] as usize;
        et[j] += 1.0;
        if e {
            eb[j] += 1.0;
        }
    }
    let share = |j: usize| eb[j] / et[j].max(1.0);
    let text_high = share(1) < share(0);
    Some(Rule { mean, axis: v, lo, hi, t, text_high })
}
