//! Gang sheet: many print-ready designs (already halftoned PNGs at the sheet DPI) packed
//! on one roll of film of a fixed width, copied pixel for pixel (no resampling, so the
//! dots stay exactly as exported). Packing: MaxRects, bottom-left rule (lowest top edge
//! first, then leftmost), optional 90° rotation, a gap between designs and a margin at
//! the film edges. With a maximum length the designs continue on a next sheet.

use std::path::Path;

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// one design: size in print pixels and number of copies
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Piece {
    pub w: usize,
    pub h: usize,
    pub count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    /// index into the pieces
    pub piece: usize,
    /// top-left corner on the sheet (pixels)
    pub x: usize,
    pub y: usize,
    /// turned 90° clockwise
    pub rot: bool,
    /// placed by hand: index into the pins (`pack_with`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin: Option<usize>,
}

/// one copy of a design placed by hand: sheet, top-left corner on that sheet (pixels,
/// margin included) and turn. The other copies are packed around it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub piece: usize,
    pub sheet: usize,
    pub x: usize,
    pub y: usize,
    #[serde(default)]
    pub rot: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sheet {
    pub w: usize,
    pub h: usize,
    pub placed: Vec<Placed>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Opts {
    /// film width in pixels
    pub width: usize,
    /// maximum sheet length in pixels (None = one sheet as long as needed)
    pub max_len: Option<usize>,
    /// space between designs and at the film edges (pixels)
    pub gap: usize,
    pub margin: usize,
    pub rotate: bool,
}

/// most copies in one gang sheet job
pub const MAX_COPIES: usize = 2000;
/// longest side of a film or design in pixels (about 42 m at 1200 dpi)
pub const MAX_SIDE: usize = 2_000_000;

#[derive(Clone, Copy, Debug)]
struct Rect {
    x: usize,
    y: usize,
    w: usize,
    h: usize,
}

impl Rect {
    fn contains(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.x + o.w <= self.x + self.w && o.y + o.h <= self.y + self.h
    }
}

/// MaxRects bin: free rectangles that may overlap each other
struct Bin {
    free: Vec<Rect>,
    placed: Vec<Placed>,
    used_h: usize,
}

impl Bin {
    fn new(w: usize, h: usize) -> Bin {
        Bin {
            free: vec![Rect { x: 0, y: 0, w, h }],
            placed: Vec::new(),
            used_h: 0,
        }
    }

    /// best spot for a w x h box (bottom-left rule): (top edge, x, y)
    fn find(&self, w: usize, h: usize) -> Option<(usize, usize, usize)> {
        self.free
            .iter()
            .filter(|f| f.w >= w && f.h >= h)
            .map(|f| (f.y + h, f.x, f.y))
            .min()
    }

    fn place(&mut self, r: Rect) {
        let mut out = Vec::with_capacity(self.free.len() + 4);
        for f in &self.free {
            let overlap = r.x < f.x + f.w && r.x + r.w > f.x && r.y < f.y + f.h && r.y + r.h > f.y;
            if !overlap {
                out.push(*f);
                continue;
            }
            if r.x > f.x {
                out.push(Rect { x: f.x, y: f.y, w: r.x - f.x, h: f.h });
            }
            if r.x + r.w < f.x + f.w {
                out.push(Rect { x: r.x + r.w, y: f.y, w: f.x + f.w - (r.x + r.w), h: f.h });
            }
            if r.y > f.y {
                out.push(Rect { x: f.x, y: f.y, w: f.w, h: r.y - f.y });
            }
            if r.y + r.h < f.y + f.h {
                out.push(Rect { x: f.x, y: r.y + r.h, w: f.w, h: f.y + f.h - (r.y + r.h) });
            }
        }
        // drop free rects contained in another one
        let mut keep = vec![true; out.len()];
        for i in 0..out.len() {
            if !keep[i] {
                continue;
            }
            for j in 0..out.len() {
                if i != j && keep[j] && out[i].contains(&out[j]) {
                    keep[j] = false;
                }
            }
        }
        self.free = out.into_iter().zip(keep).filter(|(_, k)| *k).map(|(r, _)| r).collect();
        self.used_h = self.used_h.max(r.y + r.h);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Turn {
    /// never turn
    Never,
    /// turn only when the design does not fit upright anywhere
    Needed,
    /// turn whenever that puts the design lower (bottom-left rule)
    Free,
}

/// one greedy run over `order` (piece index per copy), after the pinned boxes
/// (`pinned` = (sheet, box in bin coordinates incl. gap, placement))
fn pack_once(pieces: &[Piece], order: &[usize], gap: usize, bw: usize, bh: usize, mode: Turn, pinned: &[(usize, Rect, Placed)]) -> Vec<Bin> {
    let mut bins: Vec<Bin> = Vec::new();
    for &(k, r, p) in pinned {
        while bins.len() <= k {
            bins.push(Bin::new(bw, bh));
        }
        bins[k].place(r);
        bins[k].placed.push(p);
    }
    for &i in order {
        let p = pieces[i];
        let (w, h) = (p.w + gap, p.h + gap);
        let turnable = mode != Turn::Never && p.w != p.h;
        let upright = |b: &Bin| b.find(w, h).map(|(k, x, y)| (k, x, y, false));
        let turned = |b: &Bin| if turnable { b.find(h, w).map(|(k, x, y)| (k, x, y, true)) } else { None };
        let free = |b: &Bin| match (upright(b), turned(b)) {
            (Some(a), Some(r)) => Some(if (r.0, r.1) < (a.0, a.1) { r } else { a }),
            (a, r) => a.or(r),
        };
        let spot = if mode == Turn::Needed {
            // upright in any sheet first, then turned
            bins.iter()
                .enumerate()
                .find_map(|(k, b)| upright(b).map(|s| (k, s)))
                .or_else(|| bins.iter().enumerate().find_map(|(k, b)| turned(b).map(|s| (k, s))))
        } else {
            bins.iter().enumerate().find_map(|(k, b)| free(b).map(|s| (k, s)))
        };
        let (k, (_, x, y, rot)) = match spot {
            Some(s) => s,
            None => {
                bins.push(Bin::new(bw, bh));
                let b = bins.last().unwrap();
                let s = if mode == Turn::Needed { upright(b).or_else(|| turned(b)) } else { free(b) };
                (bins.len() - 1, s.expect("checked by pack"))
            }
        };
        let (rw, rh) = if rot { (h, w) } else { (w, h) };
        bins[k].place(Rect { x, y, w: rw, h: rh });
        bins[k].placed.push(Placed { piece: i, x, y, rot, pin: None });
    }
    // a sheet that only existed for pins that all moved away
    bins.retain(|b| !b.placed.is_empty());
    bins
}

/// pack all copies; every design must fit the film (rotated if allowed)
pub fn pack(pieces: &[Piece], o: &Opts) -> std::result::Result<Vec<Sheet>, String> {
    pack_with(pieces, o, &[])
}

/// `pack` with copies placed by hand (`pins`). A pin is used when it is inside the film
/// (within the margins, and the maximum length), turned only when turns are allowed,
/// does not come closer than the gap to an
/// earlier pin and its design still has a copy left; other pins are ignored (that copy is
/// packed like the rest). Placements from a pin carry its index (`Placed::pin`).
pub fn pack_with(pieces: &[Piece], o: &Opts, pins: &[Pin]) -> std::result::Result<Vec<Sheet>, String> {
    let total = pieces.iter().fold(0usize, |a, p| a.saturating_add(p.count));
    if total == 0 {
        return Ok(Vec::new());
    }
    if total > MAX_COPIES {
        return Err(format!("terlalu banyak salinan ({total}, maksimal {MAX_COPIES})"));
    }
    // sizes far beyond any film (also keeps the sums below from overflowing)
    if o.width > MAX_SIDE || o.max_len.is_some_and(|l| l > MAX_SIDE) || pieces.iter().any(|p| p.w > MAX_SIDE || p.h > MAX_SIDE) {
        return Err("ukuran lembar atau desain tidak masuk akal".into());
    }
    if o.gap >= o.width || o.margin >= o.width {
        return Err("jarak atau margin lebih besar dari lebar film".into());
    }
    if o.width <= 2 * o.margin {
        return Err("lebar film terlalu kecil untuk marginnya".into());
    }
    // the gap is added to every box and to the usable area, so boxes end up `gap` apart
    let bw = o.width - 2 * o.margin + o.gap;
    let bh = match o.max_len {
        Some(l) if l <= 2 * o.margin => return Err("panjang maksimal terlalu kecil untuk marginnya".into()),
        Some(l) => l - 2 * o.margin + o.gap,
        None => {
            let low = pins.iter().map(|p| p.y.min(MAX_SIDE)).max().unwrap_or(0);
            pieces.iter().map(|p| (p.w.max(p.h) + o.gap) * p.count).sum::<usize>() + low + 1
        }
    };
    for (i, p) in pieces.iter().enumerate() {
        if p.count == 0 {
            continue;
        }
        let fits = |w: usize, h: usize| w + o.gap <= bw && h + o.gap <= bh;
        if p.w == 0 || p.h == 0 || !(fits(p.w, p.h) || (o.rotate && fits(p.h, p.w))) {
            return Err(format!("desain {} ({} × {} px) lebih besar dari lembar", i + 1, p.w, p.h));
        }
    }
    // the usable pins, in bin coordinates (the boxes include the gap, like the others)
    let mut left: Vec<usize> = pieces.iter().map(|p| p.count).collect();
    let mut pinned: Vec<(usize, Rect, Placed)> = Vec::new();
    for (n, pin) in pins.iter().enumerate() {
        let Some(p) = pieces.get(pin.piece) else { continue };
        if left[pin.piece] == 0 || (pin.rot && !o.rotate) || pin.sheet >= MAX_COPIES || pin.x < o.margin || pin.y < o.margin || pin.x > MAX_SIDE || pin.y > MAX_SIDE {
            continue;
        }
        let (w, h) = if pin.rot { (p.h, p.w) } else { (p.w, p.h) };
        let r = Rect { x: pin.x - o.margin, y: pin.y - o.margin, w: w + o.gap, h: h + o.gap };
        let clash = pinned.iter().any(|(k, q, _)| *k == pin.sheet && r.x < q.x + q.w && q.x < r.x + r.w && r.y < q.y + q.h && q.y < r.y + r.h);
        if r.x + r.w > bw || r.y + r.h > bh || clash {
            continue;
        }
        left[pin.piece] -= 1;
        pinned.push((pin.sheet, r, Placed { piece: pin.piece, x: r.x, y: r.y, rot: pin.rot, pin: Some(n) }));
    }
    // several greedy runs (sort orders x rotation rules); keep the shortest film, then
    // the fewest sheets, then the fewest turned designs
    let copies: Vec<usize> = left.iter().enumerate().flat_map(|(i, &n)| std::iter::repeat_n(i, n)).collect();
    let keys: [fn(&Piece) -> (usize, usize); 4] = [
        |p| (p.w.max(p.h), p.w * p.h),
        |p| (p.h, p.w),
        |p| (p.w, p.h),
        |p| (p.w * p.h, p.w.max(p.h)),
    ];
    let all_upright = pieces.iter().all(|p| p.count == 0 || (p.w + o.gap <= bw && p.h + o.gap <= bh));
    let mut modes = vec![];
    if all_upright {
        modes.push(Turn::Never);
    }
    if o.rotate {
        modes.push(Turn::Needed);
        modes.push(Turn::Free);
    }
    // big jobs: fewer runs (each is O(copies x free rects))
    let keys = if copies.len() > 400 { &keys[..1] } else { &keys[..] };
    let mut best: Option<((usize, usize, usize), Vec<Bin>)> = None;
    for key in keys {
        let mut order = copies.clone();
        order.sort_by_key(|&i| (std::cmp::Reverse(key(&pieces[i])), i));
        for &mode in &modes {
            let bins = pack_once(pieces, &order, o.gap, bw, bh, mode, &pinned);
            // real film used: every sheet also has its edge margins
            let score = (
                bins.iter().map(|b| b.used_h - o.gap + 2 * o.margin).sum::<usize>(),
                bins.len(),
                bins.iter().map(|b| b.placed.iter().filter(|p| p.rot).count()).sum::<usize>(),
            );
            if best.as_ref().is_none_or(|(s, _)| score < *s) {
                best = Some((score, bins));
            }
        }
    }
    let bins = best.expect("at least one run").1;
    Ok(bins
        .into_iter()
        .map(|b| Sheet {
            w: o.width,
            h: b.used_h - o.gap + 2 * o.margin,
            placed: b
                .placed
                .into_iter()
                .map(|p| Placed { x: p.x + o.margin, y: p.y + o.margin, ..p })
                .collect(),
        })
        .collect())
}

/// a print-ready design: RGBA8 with alpha 0 or 255 only
pub struct Design {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

/// DPI stored in a PNG (pHYs), if any
pub fn png_dpi(path: &Path) -> Option<f64> {
    let f = std::fs::File::open(path).ok()?;
    let r = png::Decoder::new(std::io::BufReader::new(f)).read_info().ok()?;
    let d = r.info().pixel_dims?;
    match d.unit {
        png::Unit::Meter if d.xppu > 0 => Some(d.xppu as f64 * 0.0254),
        _ => None,
    }
}

/// load a print-ready design; refuses semi-transparent pixels (not a DTF print file)
pub fn load_design(path: &Path) -> Result<Design> {
    let img = crate::io::load_rgba8(path)?;
    if !img.has_alpha {
        bail!("tidak transparan: bukan file hasil export (background masih ada)");
    }
    let semi = img.px.chunks_exact(4).filter(|p| p[3] != 0 && p[3] != 255).count();
    if semi > 0 {
        bail!("{semi} piksel setengah transparan: pakai file hasil export StudioZea");
    }
    Ok(Design { w: img.w, h: img.h, px: img.px })
}

/// RGBA8 rows [y0, y1) of a sheet
pub fn sheet_rows(sheet: &Sheet, designs: &[&Design], y0: usize, y1: usize) -> Vec<u8> {
    let w = sheet.w;
    let mut out = vec![0u8; (y1 - y0) * w * 4];
    for p in &sheet.placed {
        let d = designs[p.piece];
        let (pw, ph) = if p.rot { (d.h, d.w) } else { (d.w, d.h) };
        let (ya, yb) = (p.y.max(y0), (p.y + ph).min(y1));
        for y in ya..yb {
            let ly = y - p.y;
            let row = &mut out[(y - y0) * w * 4..(y - y0 + 1) * w * 4];
            if !p.rot {
                let src = &d.px[ly * d.w * 4..(ly + 1) * d.w * 4];
                let dst = &mut row[p.x * 4..(p.x + pw) * 4];
                // copy only ink (the boxes never overlap, but keep transparent pixels clean)
                for (s, t) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
                    if s[3] != 0 {
                        t.copy_from_slice(s);
                    }
                }
            } else {
                // 90° clockwise: sheet (lx, ly) <- design (ly, h - 1 - lx)
                for lx in 0..pw {
                    let (sx, sy) = (ly, d.h - 1 - lx);
                    let q = (sy * d.w + sx) * 4;
                    if d.px[q + 3] != 0 {
                        let t = (p.x + lx) * 4;
                        row[t..t + 4].copy_from_slice(&d.px[q..q + 4]);
                    }
                }
            }
        }
    }
    out
}

/// write one sheet as a PNG (streamed in bands); `on_rows(done)` reports progress and
/// can stop the export by returning false
pub fn save_sheet(sheet: &Sheet, designs: &[&Design], dpi: f64, path: &Path, mut on_rows: impl FnMut(usize) -> bool) -> Result<()> {
    crate::io::write_png_rows(sheet.w, sheet.h, dpi, path, |y0, y1| {
        if !on_rows(y1) {
            bail!("dibatalkan");
        }
        Ok(sheet_rows(sheet, designs, y0, y1))
    })
}
