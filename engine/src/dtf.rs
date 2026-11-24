//! DTF-specific screening: FM clusters of the minimum printable size, the per-shape
//! safe AM tone zone, the hybrid channel screen, automatic tone calibration and the
//! final clean-up. Port of prototype/studiozea_ref.py.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use rayon::prelude::*;

use crate::filters::{
    edt, feature_transform, label, max_filter, min_filter, uniform_filter, uniform_filter1d,
};
use crate::noise::blue_noise;
use crate::screen::{threshold_tile, Grid, Shape};

pub const MIN_DOT_MM: f64 = 0.15;

/// edge of the smallest FM cluster (300 dpi -> 2 px, 600 dpi -> 4 px)
pub fn mindot_px(dpi: f64) -> usize {
    ((MIN_DOT_MM * dpi / 25.4).ceil() as usize).max(1)
}

/// smallest island / hole kept (300 dpi -> 3 px, 600 dpi -> 10 px)
pub fn minarea_px(dpi: f64) -> u32 {
    let r = MIN_DOT_MM / 2.0 * dpi / 25.4;
    ((std::f64::consts::PI * (r * r)).round_ties_even() as u32).max(3)
}

/// FM: k x k clusters, blue-noise threshold on the k x k block means
pub fn fm_bits(t: &[f32], w: usize, h: usize, dpi: f64) -> Vec<bool> {
    fm_bits_at(t, w, h, dpi, 0, 0)
}

/// `fm_bits` for a window whose top-left pixel is (ox, oy) in the full print.
/// ox and oy must be multiples of `mindot_px(dpi)` so the k x k blocks line up.
pub fn fm_bits_at(t: &[f32], w: usize, h: usize, dpi: f64, ox: usize, oy: usize) -> Vec<bool> {
    let k = mindot_px(dpi);
    debug_assert!(ox % k == 0 && oy % k == 0, "window origin must be block aligned");
    let (obx, oby) = (ox / k, oy / k);
    let bw = w.div_ceil(k);
    let bn = blue_noise();
    let p = block_means(t, w, h, k);
    let blocks: Vec<bool> = p.par_iter().enumerate().map(|(bi, &v)| v > bn.at(bi % bw + obx, bi / bw + oby)).collect();
    expand_blocks(&blocks, w, h, k)
}

/// k x k block means of a w x h plane (edge blocks repeat the edge pixels, like
/// numpy.pad(mode='edge') + mean), row-major, ceil(w/k) x ceil(h/k)
pub fn block_means(t: &[f32], w: usize, h: usize, k: usize) -> Vec<f32> {
    let bh = h.div_ceil(k);
    let bw = w.div_ceil(k);
    let kk = (k * k) as f32;
    (0..bh * bw)
        .into_par_iter()
        .map(|bi| {
            let by = bi / bw;
            let bx = bi % bw;
            let mut acc = 0f32;
            for i in 0..k {
                let y = (by * k + i).min(h - 1);
                let mut row = t[y * w + (bx * k).min(w - 1)];
                for j in 1..k {
                    let x = (bx * k + j).min(w - 1);
                    row += t[y * w + x];
                }
                acc += row;
            }
            acc / kk
        })
        .collect()
}

/// block bits -> pixel bits
fn expand_blocks(blocks: &[bool], w: usize, h: usize, k: usize) -> Vec<bool> {
    let bw = w.div_ceil(k);
    let mut out = vec![false; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let by = y / k;
        for (x, o) in row.iter_mut().enumerate() {
            *o = blocks[by * bw + x / k];
        }
    });
    out
}

/// Bayer 8 x 8 threshold ((rank + 0.5) / 64), row-major
fn bayer8() -> &'static [f32; 64] {
    static B: OnceLock<[f32; 64]> = OnceLock::new();
    B.get_or_init(|| {
        let mut m = vec![0usize];
        let mut n = 1;
        while n < 8 {
            let mut next = vec![0usize; 4 * n * n];
            for y in 0..n {
                for x in 0..n {
                    let v = 4 * m[y * n + x];
                    next[y * 2 * n + x] = v;
                    next[y * 2 * n + x + n] = v + 2;
                    next[(y + n) * 2 * n + x] = v + 3;
                    next[(y + n) * 2 * n + x + n] = v + 1;
                }
            }
            m = next;
            n *= 2;
        }
        let mut out = [0f32; 64];
        for (o, &r) in out.iter_mut().zip(&m) {
            *o = ((r as f64 + 0.5) / 64.0) as f32;
        }
        out
    })
}

/// ordered dither (Bayer 8 x 8) on k x k blocks (port of `bayer_bits`)
pub fn bayer_bits_at(t: &[f32], w: usize, h: usize, dpi: f64, ox: usize, oy: usize) -> Vec<bool> {
    let k = mindot_px(dpi);
    let (obx, oby) = (ox / k, oy / k);
    let bw = w.div_ceil(k);
    let bm = bayer8();
    let p = block_means(t, w, h, k);
    let blocks: Vec<bool> = p
        .par_iter()
        .enumerate()
        .map(|(bi, &v)| v > bm[((bi / bw + oby) % 8) * 8 + (bi % bw + obx) % 8])
        .collect();
    expand_blocks(&blocks, w, h, k)
}

/// error diffusion tile and warm-up in blocks (see `ed_bits_at`)
pub const ED_TILE: usize = 32;
pub const ED_WARM: usize = 16;
/// threshold 0.5 +- ED_JITTER / 2 by the blue-noise tile (no worms, no tile seams)
pub const ED_JITTER: f64 = 0.4;

/// Floyd-Steinberg error diffusion (serpentine, f64) on k x k blocks, per tile of
/// ED_TILE blocks of the absolute block grid after ED_WARM blocks of warm-up above and
/// on both sides: a window with (ED_TILE + ED_WARM + 2) blocks of halo gives the same
/// dots as the full print. Port of `ed_bits`.
pub fn ed_bits_at(t: &[f32], w: usize, h: usize, dpi: f64, ox: usize, oy: usize) -> Vec<bool> {
    let k = mindot_px(dpi);
    let (bx0, by0) = (ox / k, oy / k);
    let bw = w.div_ceil(k);
    let bh = h.div_ceil(k);
    let p = block_means(t, w, h, k);
    let bn = blue_noise();
    let (tt, m) = (ED_TILE, ED_WARM);
    let tiles: Vec<(usize, usize)> = (by0 / tt..=(by0 + bh - 1) / tt)
        .flat_map(|ty| (bx0 / tt..=(bx0 + bw - 1) / tt).map(move |tx| (ty, tx)))
        .collect();
    let parts: Vec<Vec<(usize, bool)>> = tiles
        .par_iter()
        .map(|&(ty, tx)| {
            let r0 = (ty * tt).saturating_sub(m).max(by0);
            let r1 = (ty * tt + tt).min(by0 + bh);
            let c0 = (tx * tt).saturating_sub(m).max(bx0);
            let c1 = (tx * tt + tt + m).min(bx0 + bw);
            let mut res = Vec::new();
            if r1 <= r0 || c1 <= c0 {
                return res;
            }
            let nw = c1 - c0;
            let mut v: Vec<Vec<f64>> = (r0..r1)
                .map(|r| (c0..c1).map(|c| p[(r - by0) * bw + (c - bx0)] as f64).collect())
                .collect();
            for i in 0..r1 - r0 {
                let r = r0 + i;
                let ltr = r % 2 == 0;
                let (cur, rest) = v.split_at_mut(i + 1);
                let row = &mut cur[i];
                let mut nxt = rest.first_mut();
                for s in 0..nw {
                    let j = if ltr { s } else { nw - 1 - s };
                    let c = c0 + j;
                    let old = row[j];
                    let on = old > 0.5 + ED_JITTER * (bn.at(c, r) as f64 - 0.5);
                    let e = old - if on { 1.0 } else { 0.0 };
                    if c >= tx * tt && c < tx * tt + tt && r >= ty * tt {
                        res.push(((r - by0) * bw + (c - bx0), on));
                    }
                    let fwd = if ltr { j + 1 } else { j.wrapping_sub(1) };
                    let back = if ltr { j.wrapping_sub(1) } else { j + 1 };
                    if fwd < nw {
                        row[fwd] += e * 0.4375;
                    }
                    if let Some(n) = nxt.as_deref_mut() {
                        if back < nw {
                            n[back] += e * 0.1875;
                        }
                        n[j] += e * 0.3125;
                        if fwd < nw {
                            n[fwd] += e * 0.0625;
                        }
                    }
                }
            }
            res
        })
        .collect();
    let mut blocks = vec![false; bw * bh];
    for part in parts {
        for (i, on) in part {
            blocks[i] = on;
        }
    }
    expand_blocks(&blocks, w, h, k)
}

/// halo (print px) that error diffusion needs around a window
pub fn ed_halo(dpi: f64) -> usize {
    (ED_TILE + ED_WARM + 2) * mindot_px(dpi)
}

/// interlock: outside the printable AM zone [lo, hi] the tone jumps to nothing / the
/// smallest printable dot, and to the smallest printable hole / solid (port of
/// `interlock_snap`), so no FM speckles: every dot keeps the same neat shape
pub fn interlock_snap(t: &[f32], lo: f64, hi: f64) -> Vec<f32> {
    // numpy compares a float32 array with a Python float in float32
    let (lo32, lo2, hi32, hi2) = (lo as f32, (lo / 2.0) as f32, hi as f32, ((1.0 + hi) / 2.0) as f32);
    t.iter()
        .map(|&v| {
            let v = if v < lo32 { if v < lo2 { 0.0 } else { lo32 } } else { v };
            if v > hi32 {
                if v > hi2 { 1.0 } else { hi32 }
            } else {
                v
            }
        })
        .collect()
}

/// numpy.linspace(start, stop, num) in f64 (endpoint included)
pub fn linspace(start: f64, stop: f64, num: usize) -> Vec<f64> {
    let div = (num - 1) as f64;
    let step = (stop - start) / div;
    let mut y: Vec<f64> = (0..num).map(|i| i as f64 * step + start).collect();
    if num > 1 {
        y[num - 1] = stop;
    }
    y
}

/// numpy.interp(x, xp, fp) (xp strictly increasing)
pub fn interp(x: f64, xp: &[f64], fp: &[f64]) -> f64 {
    let n = xp.len();
    if x > xp[n - 1] {
        return fp[n - 1];
    }
    if x < xp[0] {
        return fp[0];
    }
    let j = xp.partition_point(|&v| v <= x) - 1;
    if j == n - 1 || xp[j] == x {
        return fp[j];
    }
    let slope = (fp[j + 1] - fp[j]) / (xp[j + 1] - xp[j]);
    slope * (x - xp[j]) + fp[j]
}

type ZoneKey = (u64, u64, u64, Shape, Grid);
fn zone_cache() -> &'static Mutex<HashMap<ZoneKey, (f64, f64)>> {
    static C: OnceLock<Mutex<HashMap<ZoneKey, (f64, f64)>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

fn tiny_frac(m: &[bool], n: usize, amin: u32) -> f64 {
    let (lab, sizes) = label(m, n, n);
    if sizes.len() <= 1 {
        return 0.0;
    }
    let mut small_px = 0u64;
    for &l in &lab {
        if l != 0 && sizes[l as usize] < amin {
            small_px += 1;
        }
    }
    let total = m.iter().filter(|&&b| b).count() as u64;
    small_px as f64 / total.max(1) as f64
}

/// tone range in which the AM screen makes only printable islands/holes
pub fn safe_zone(dpi: f64, lpi: f64, deg: f64, shape: Shape, grid: Grid) -> (f64, f64) {
    let key = (dpi.to_bits(), lpi.to_bits(), deg.to_bits(), shape, grid);
    if let Some(z) = zone_cache().lock().unwrap().get(&key) {
        return *z;
    }
    let tile = threshold_tile(dpi, lpi, deg, shape, grid);
    let n = ((6.0 * dpi / lpi) as usize + 64).min(1024);
    let tt: Vec<f64> = (0..n * n).map(|p| tile.at(p % n, p / n) as f64).collect();
    let amin = minarea_px(dpi);
    let levels = linspace(0.005, 0.995, 100);
    let ok: Vec<bool> = levels
        .par_iter()
        .map(|&t| {
            let b: Vec<bool> = tt.iter().map(|&v| t > v).collect();
            let nb: Vec<bool> = b.iter().map(|&v| !v).collect();
            tiny_frac(&b, n, amin).max(tiny_frac(&nb, n, amin)) <= 0.02
        })
        .collect();
    let z = match (ok.iter().position(|&v| v), ok.iter().rposition(|&v| v)) {
        (Some(a), Some(b)) => (levels[a], levels[b]),
        _ => (0.5, 0.5),
    };
    zone_cache().lock().unwrap().insert(key, z);
    z
}

/// one channel: ranked AM screen, FM outside the printable tone zone
pub fn channel(
    t: &[f32],
    w: usize,
    h: usize,
    dpi: f64,
    lpi: f64,
    deg: f64,
    shape: Shape,
    grid: Grid,
    hybrid: bool,
) -> Vec<bool> {
    channel_at(t, w, h, 0, 0, dpi, lpi, deg, shape, grid, hybrid)
}

/// `channel` for a window at (ox, oy) of the full print (block aligned, see fm_bits_at)
pub fn channel_at(
    t: &[f32],
    w: usize,
    h: usize,
    ox: usize,
    oy: usize,
    dpi: f64,
    lpi: f64,
    deg: f64,
    shape: Shape,
    grid: Grid,
    hybrid: bool,
) -> Vec<bool> {
    let tile = threshold_tile(dpi, lpi, deg, shape, grid);
    let mut am = vec![false; w * h];
    am.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let mut th = vec![0f32; w];
        tile.row(ox, y + oy, &mut th);
        for (x, o) in row.iter_mut().enumerate() {
            *o = t[y * w + x] > th[x];
        }
    });
    if !hybrid {
        return am;
    }
    let (lo, hi) = safe_zone(dpi, lpi, deg, shape, grid);
    let (lo, hi) = (lo as f32, hi as f32);
    let ts = uniform_filter(t, w, h, ((dpi / lpi) as usize).max(3));
    let fm = fm_bits_at(t, w, h, dpi, ox, oy);
    (0..w * h)
        .into_par_iter()
        .map(|p| {
            if ts[p] < lo || ts[p] > hi {
                fm[p]
            } else {
                am[p]
            }
        })
        .collect()
}

type CalKey = (u64, Shape, u64, [u64; 3], Grid);
type Lut = Arc<(Vec<f64>, Vec<f64>)>;
fn cal_cache() -> &'static Mutex<HashMap<CalKey, Lut>> {
    static C: OnceLock<Mutex<HashMap<CalKey, Lut>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// measured tone -> coverage curve (AM + FM zones, no clean-up), inverted.
/// Returns (lut_t, inv) both as f64 copies of the f32 values numpy holds.
pub fn calib_curve(lpi: f64, shape: Shape, dpi: f64, angles: [f64; 3], grid: Grid) -> Lut {
    let key = (
        lpi.to_bits(),
        shape,
        dpi.to_bits(),
        angles.map(f64::to_bits),
        grid,
    );
    if let Some(l) = cal_cache().lock().unwrap().get(&key) {
        return l.clone();
    }
    let k = ((dpi as usize) / 300).max(1);
    // enough cells for big dots (low LPI); unchanged for the usual sizes
    let cell = dpi / lpi;
    let (w, h) = ((2048 * k).max((64.0 * cell).ceil() as usize), (96 * k).max((4.0 * cell).ceil() as usize));
    let t32: Vec<f32> = linspace(0.0, 1.0, w).iter().map(|&v| v as f32).collect();
    let ramp: Vec<f32> = (0..w * h).map(|p| t32[p % w]).collect();
    let fsize = (31usize).max((4.0 * dpi / lpi) as usize);
    let covs: Vec<Vec<f32>> = (0..3)
        .map(|c| {
            let b = channel(&ramp, w, h, dpi, lpi, angles[c], shape, grid, true);
            let mean: Vec<f32> = (0..w)
                .map(|x| {
                    let cnt = (0..h).filter(|&y| b[y * w + x]).count();
                    (cnt as f64 / h as f64) as f32
                })
                .collect();
            uniform_filter1d(&mean, fsize)
        })
        .collect();
    let mut cov: Vec<f32> = (0..w)
        .map(|x| (covs[0][x] + covs[1][x] + covs[2][x]) / 3.0)
        .collect();
    for i in 1..w {
        if cov[i] < cov[i - 1] {
            cov[i] = cov[i - 1];
        }
    }
    cov[0] = 0.0;
    if cov[w - 1] < 1.0 {
        cov[w - 1] = 1.0;
    }
    let xp: Vec<f64> = (0..w).map(|i| cov[i] as f64 + i as f64 * 1e-7).collect();
    let fp: Vec<f64> = t32.iter().map(|&v| v as f64).collect();
    let lut_t: Vec<f64> = linspace(0.0, 1.0, 1025)
        .iter()
        .map(|&v| v as f32 as f64)
        .collect();
    let inv: Vec<f64> = lut_t
        .iter()
        .map(|&x| interp(x, &xp, &fp) as f32 as f64)
        .collect();
    let l = Arc::new((lut_t, inv));
    cal_cache().lock().unwrap().insert(key, l.clone());
    l
}

pub fn calibrate(
    t: &[f32],
    lpi: f64,
    shape: Shape,
    dpi: f64,
    angles: [f64; 3],
    grid: Grid,
) -> Vec<f32> {
    let l = calib_curve(lpi, shape, dpi, angles, grid);
    let (xp, fp) = (&l.0, &l.1);
    t.par_iter()
        .map(|&v| interp(v as f64, xp, fp) as f32)
        .collect()
}

/// RGBA image: remove ink islands < min area, fill holes < min area with the nearest
/// ink colour, RGB = 0 under transparency. Alpha stays 0/255.
pub fn clean(o: &mut [u8], w: usize, h: usize, dpi: f64) {
    let amin = minarea_px(dpi);
    let op: Vec<bool> = (0..w * h).map(|p| o[p * 4 + 3] == 255).collect();
    let (lab, sizes) = label(&op, w, h);
    let op2: Vec<bool> = (0..w * h)
        .map(|p| op[p] && !(lab[p] != 0 && sizes[lab[p] as usize] < amin))
        .collect();
    let holes: Vec<bool> = op2.iter().map(|&v| !v).collect();
    let (hl, hs) = label(&holes, w, h);
    let fill: Vec<bool> = (0..w * h)
        .map(|p| hl[p] != 0 && hs[hl[p] as usize] < amin)
        .collect();
    let src = o.to_vec();
    if fill.iter().any(|&f| f) {
        let ft = feature_transform(&holes, w, h);
        for p in 0..w * h {
            if fill[p] {
                // numpy indexing: -1 wraps to the last row / column
                let fy = ft[p][0].rem_euclid(h as i64) as usize;
                let fx = ft[p][1].rem_euclid(w as i64) as usize;
                let q = fy * w + fx;
                o[p * 4] = src[q * 4];
                o[p * 4 + 1] = src[q * 4 + 1];
                o[p * 4 + 2] = src[q * 4 + 2];
            }
        }
    }
    for p in 0..w * h {
        let on = op2[p] || fill[p];
        o[p * 4 + 3] = if on { 255 } else { 0 };
        if !on {
            o[p * 4] = 0;
            o[p * 4 + 1] = 0;
            o[p * 4 + 2] = 0;
        }
    }
}

/// solid ring on hard edges, stray specks just outside removed (per channel bits)
pub fn sharp_edges(tone: &[f32], bits: &mut [bool], w: usize, h: usize, dpi: f64) {
    let band = 2.2f64.max(MIN_DOT_MM * dpi / 25.4 + 0.4);
    let ink: Vec<f32> = (0..w * h)
        .map(|p| tone[p * 3].max(tone[p * 3 + 1]).max(tone[p * 3 + 2]))
        .collect();
    let s: Vec<bool> = ink.iter().map(|&v| v > 0.5).collect();
    let mx = max_filter(&ink, w, h, 5);
    let mn = min_filter(&ink, w, h, 5);
    let hard: Vec<bool> = (0..w * h).map(|p| mx[p] - mn[p] > 0.45f32).collect();
    let d_in = edt(&s, w, h);
    let ns: Vec<bool> = s.iter().map(|&v| !v).collect();
    let d_out = edt(&ns, w, h);
    for p in 0..w * h {
        if hard[p] && s[p] && d_in[p] <= band {
            for c in 0..3 {
                bits[p * 3 + c] = tone[p * 3 + c] > 0.5;
            }
        } else if hard[p] && !s[p] && d_out[p] <= 3.0 {
            for c in 0..3 {
                bits[p * 3 + c] = false;
            }
        }
    }
}
