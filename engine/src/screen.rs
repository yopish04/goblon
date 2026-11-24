//! Rational-tangent AM screens with exact, ranked threshold tiles.
//! Port of `rational`, `choose_grid`, `screen_info`, `threshold_tile` in
//! prototype/studiozea_ref.py. Floating point order follows numpy exactly.

use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};

const TAU: f64 = 2.0 * PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Shape {
    Bulat,
    BulatMurni,
    Elips,
    Kotak,
    Berlian,
    Garis,
    Silang,
    /// wavy lines
    Gelombang,
    /// crosshatch: one line set, the perpendicular set joins at 50 %
    Arsir,
    /// rough / analog round dot (per pixel-class jitter, see `jitter`)
    Kasar,
    /// five-point star (one point up at 0 deg), for big decorative dots
    Bintang,
    /// triangle, point up
    Segitiga,
    /// chain: diamonds that link up along u first
    Rantai,
}

/// wave amplitude of `Gelombang` (cell units)
pub const WAVE: f64 = 0.16;
/// `Bintang`: depth of the star points
pub const STAR: f64 = 0.38;
/// `Arsir`: offset and slope of the second (perpendicular) line set
pub const HATCH: (f64, f64) = (1.0, 0.5);
/// max pixel classes of a jittered super-cell (see `threshold_tile`)
pub const JITTER_CLASSES: i64 = 1600;

/// deterministic noise in [-0.5, 0.5) per pixel class (a, b) of the super-cell:
/// splitmix64 of (a << 32 | b), top 53 bits. Integer only, so it is bit-identical to
/// `class_noise` in studiozea_ref.py (a trig hash would differ in the last ulp).
#[inline]
pub fn class_noise(a: i64, b: i64) -> f64 {
    let mut z = ((a as u64) << 32) | (b as u64);
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 * (1.0 / (1u64 << 53) as f64) - 0.5
}

impl Shape {
    pub const ALL: [Shape; 13] = [
        Shape::Bulat,
        Shape::BulatMurni,
        Shape::Elips,
        Shape::Kotak,
        Shape::Berlian,
        Shape::Garis,
        Shape::Silang,
        Shape::Gelombang,
        Shape::Arsir,
        Shape::Kasar,
        Shape::Bintang,
        Shape::Segitiga,
        Shape::Rantai,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Shape::Bulat => "bulat",
            Shape::BulatMurni => "bulat-murni",
            Shape::Elips => "elips",
            Shape::Kotak => "kotak",
            Shape::Berlian => "berlian",
            Shape::Garis => "garis",
            Shape::Silang => "silang",
            Shape::Gelombang => "gelombang",
            Shape::Arsir => "arsir",
            Shape::Kasar => "kasar",
            Shape::Bintang => "bintang",
            Shape::Segitiga => "segitiga",
            Shape::Rantai => "rantai",
        }
    }

    /// amplitude of the per pixel-class noise added to the spot function (None = exact shape)
    pub fn jitter(self) -> Option<f64> {
        match self {
            Shape::Kasar => Some(0.8),
            _ => None,
        }
    }

    pub fn parse(s: &str) -> Option<Shape> {
        Shape::ALL.iter().copied().find(|x| x.name() == s)
    }

    /// spot function s(u, v), u, v in [-0.5, 0.5)
    #[inline]
    pub fn spot(self, u: f64, v: f64) -> f64 {
        match self {
            Shape::Bulat => 2.0 - (TAU * u).cos() - (TAU * v).cos(),
            Shape::BulatMurni => u * u + v * v,
            Shape::Elips => {
                let a = u / 0.8;
                let b = v * 0.8;
                a * a + b * b
            }
            Shape::Kotak => u.abs().max(v.abs()),
            Shape::Berlian => u.abs() + v.abs(),
            Shape::Garis => v.abs() + u.abs() * 1e-3,
            Shape::Silang => u.abs().min(v.abs()),
            Shape::Gelombang => {
                1.0 - (TAU * (v + WAVE * (TAU * u).sin())).cos()
                    + 1e-3 * (1.0 - (TAU * u).cos())
            }
            Shape::Arsir => (1.0 - (TAU * v).cos()).min(HATCH.0 + HATCH.1 * (1.0 - (TAU * u).cos())),
            Shape::Kasar => 2.0 - (TAU * u).cos() - (TAU * v).cos(),
            Shape::Bintang => {
                // r (1 + STAR sin 5a), sin 5a = Im((u + iv)^5) / r^5 (no trig: same bits as numpy)
                let (u2, v2) = (u * u, v * v);
                let r2 = u2 + v2;
                if r2 > 0.0 {
                    let p5 = v * (5.0 * u2 * u2 - 10.0 * u2 * v2 + v2 * v2);
                    r2.sqrt() + STAR * p5 / (r2 * r2)
                } else {
                    0.0
                }
            }
            Shape::Segitiga => v.max(0.866 * u - 0.5 * v).max(-0.866 * u - 0.5 * v),
            Shape::Rantai => u.abs() / 1.3 + v.abs() * 1.3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Grid {
    /// one cell per step (same family as the reference: 35 LPI 22.5 deg -> 8:3)
    Rapi,
    /// up to 6 cells per step, prefers many grey levels
    Presisi,
}

fn gcd(a: i64, b: i64) -> i64 {
    let (mut a, mut b) = (a.abs(), b.abs());
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// |a - b| in radians, the short way round (screen angles may be 0-360 deg)
#[inline]
pub fn angle_diff(a: f64, b: f64) -> f64 {
    // a in (-pi, pi], b in [0, 2 pi): |a - b| can reach 3 pi
    let d = (a - b).abs() % TAU;
    if d > PI {
        TAU - d
    } else {
        d
    }
}

/// rational screen (P, Q, n): cell vector = (P, Q) / n pixels
pub fn rational(cellpx: f64, deg: f64, maxn: i64, max_tile: i64) -> (i64, i64, i64) {
    let a0 = deg.to_radians();
    let mut best: Option<(f64, i64, i64, i64)> = None;
    for n in 1..=maxn {
        let tx = cellpx * n as f64 * a0.cos();
        let ty = cellpx * n as f64 * a0.sin();
        let mut ps = vec![tx.floor() as i64, tx.ceil() as i64];
        ps.dedup();
        let mut qs = vec![ty.floor() as i64, ty.ceil() as i64];
        qs.dedup();
        for &p in &ps {
            for &q in &qs {
                if p == 0 && q == 0 {
                    continue;
                }
                if gcd(gcd(p, q), n) != 1 {
                    continue;
                }
                let s = p * p + q * q;
                let c = (s as f64).sqrt() / n as f64;
                let err = (c - cellpx).abs() / cellpx + angle_diff((q as f64).atan2(p as f64), a0) * 0.5;
                if (maxn > 1 && err > 0.03) || s > max_tile {
                    continue;
                }
                let score = err + 1.0 / s as f64;
                if best.is_none_or(|b| score < b.0 - 1e-12) {
                    best = Some((score, p, q, n));
                }
            }
        }
    }
    match best {
        Some((_, p, q, n)) => (p, q, n),
        None => (
            (cellpx * a0.cos()).round_ties_even() as i64,
            (cellpx * a0.sin()).round_ties_even() as i64,
            1,
        ),
    }
}

pub fn rational_grid(dpi: f64, lpi: f64, deg: f64, grid: Grid) -> (i64, i64, i64) {
    let maxn = if grid == Grid::Rapi { 1 } else { 6 };
    rational(dpi / lpi, deg, maxn, 1600)
}

/// 'auto': the reference-style grid when it is close enough and has >= 64 levels
pub fn choose_grid(dpi: f64, lpi: f64, deg: f64) -> Grid {
    let cellpx = dpi / lpi;
    let (p, q, _) = rational(cellpx, deg, 1, 1600);
    let s = p * p + q * q;
    let err = ((s as f64).sqrt() - cellpx).abs() / cellpx
        + angle_diff((q as f64).atan2(p as f64), deg.to_radians()) * 0.5;
    if err <= 0.03 && s >= 64 {
        Grid::Rapi
    } else {
        Grid::Presisi
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ScreenInfo {
    pub p: i64,
    pub q: i64,
    pub n: i64,
    pub lpi: f64,
    pub angle: f64,
    pub levels: i64,
}

pub fn screen_info(dpi: f64, lpi: f64, deg: f64, grid: Grid) -> ScreenInfo {
    let (p, q, n) = rational_grid(dpi, lpi, deg, grid);
    let c = ((p * p + q * q) as f64).sqrt() / n as f64;
    ScreenInfo {
        p,
        q,
        n,
        lpi: dpi / c,
        angle: (q as f64).atan2(p as f64).to_degrees(),
        levels: p * p + q * q,
    }
}

/// periodic S x S threshold tile
pub struct Tile {
    pub s: usize,
    pub data: Vec<f32>,
}

impl Tile {
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.data[(y % self.s) * self.s + (x % self.s)]
    }
}

/// Thresholds of a rational screen, one per pixel class (O(S) memory instead of an
/// S x S tile, so big cells / low LPI work). Pixel (x, y) belongs to class
/// `((x mod g) * g + (y mod g)) * s1 + ((x div g) * p1 + (y div g) * q1) mod s1` with
/// g = gcd(P, Q), P = g p1, Q = g q1, s1 = p1^2 + q1^2 (g^2 s1 = S classes): two pixels
/// are in the same class exactly when they differ by a vector of the lattice spanned by
/// (P, Q) and (-Q, P). Same as `ScreenTable` in studiozea_ref.py.
pub struct ScreenTile {
    g: usize,
    p1: i64,
    q1: i64,
    s1: i64,
    table: Vec<f32>,
}

impl ScreenTile {
    #[inline]
    fn index(&self, x: usize, y: usize) -> usize {
        let g = self.g;
        let a = ((x / g) as i64 * self.p1 + (y / g) as i64 * self.q1).rem_euclid(self.s1) as usize;
        ((x % g) * g + y % g) * self.s1 as usize + a
    }

    #[inline]
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.table[self.index(x, y)]
    }

    /// thresholds of row y, pixels x0 .. x0 + out.len() (incremental: no division per pixel)
    pub fn row(&self, x0: usize, y: usize, out: &mut [f32]) {
        let (g, s1) = (self.g, self.s1);
        let su = s1 as usize;
        let step = self.p1.rem_euclid(s1);
        let ym = y % g;
        let mut xm = x0 % g;
        let mut a = ((x0 / g) as i64 * self.p1 + (y / g) as i64 * self.q1).rem_euclid(s1);
        for o in out.iter_mut() {
            *o = self.table[(xm * g + ym) * su + a as usize];
            xm += 1;
            if xm == g {
                xm = 0;
                a += step;
                if a >= s1 {
                    a -= s1;
                }
            }
        }
    }
}

type TileKey = (u64, u64, u64, Shape, Grid);
fn tile_cache() -> &'static Mutex<HashMap<TileKey, Arc<ScreenTile>>> {
    static C: OnceLock<Mutex<HashMap<TileKey, Arc<ScreenTile>>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Exact thresholds: every pixel-position class of the super-cell gets a unique rank
/// (ties broken by cell, then angle, then class id - numpy lexsort, stable).
pub fn threshold_tile(dpi: f64, lpi: f64, deg: f64, shape: Shape, grid: Grid) -> Arc<ScreenTile> {
    let key = (dpi.to_bits(), lpi.to_bits(), deg.to_bits(), shape, grid);
    if let Some(t) = tile_cache().lock().unwrap().get(&key) {
        return t.clone();
    }
    let (mut p, mut q, mut n) = rational_grid(dpi, lpi, deg, grid);
    let jitter = shape.jitter();
    if jitter.is_some() {
        // same screen (cell vector (kP, kQ)/(kn) = (P, Q)/n), k^2 times more cells per
        // super-cell, so neighbouring rough dots differ (k^2 * S <= JITTER_CLASSES)
        let s0 = p * p + q * q;
        let mut k = 1;
        while (k + 1) * (k + 1) * s0 <= JITTER_CLASSES {
            k += 1;
        }
        (p, q, n) = (k * p, k * q, k * n);
    }
    let s = p * p + q * q;
    let g = gcd(p, q);
    let (p1, q1) = (p / g, q / g);
    let s1 = p1 * p1 + q1 * q1;
    // one pixel per class: x = g X + xm, y = ym (X p1 runs through every residue mod s1)
    let mut cls: Vec<(i64, i64, i64, usize, usize)> = Vec::with_capacity(s as usize);
    for xm in 0..g {
        for ym in 0..g {
            for xx in 0..s1 {
                let (x, y) = (g * xx + xm, ym);
                let a = (x * p + y * q).rem_euclid(s);
                let b = (-x * q + y * p).rem_euclid(s);
                cls.push((a * s + b, a, b, x as usize, y as usize));
            }
        }
    }
    cls.sort_unstable_by_key(|c| c.0);
    let nuc = cls.len();
    // per-class keys
    let off = 0.5 / s as f64;
    let mut rsc = vec![0f64; nuc];
    let mut frac = vec![0f64; nuc];
    let mut ang = vec![0f64; nuc];
    for (i, &(_, a, b, _, _)) in cls.iter().enumerate() {
        let u = ((a * n).rem_euclid(s)) as f64 / s as f64 - 0.5;
        let v = ((b * n).rem_euclid(s)) as f64 / s as f64 - 0.5;
        let mut sc = shape.spot(u + off, v + off);
        if let Some(amp) = jitter {
            sc += amp * class_noise(a, b);
        }
        rsc[i] = (sc * 1e9).round_ties_even() / 1e9;
        frac[i] = ((((a * n / s) * n + (b * n / s)) as f64) * 0.618) % 1.0;
        ang[i] = v.atan2(u);
    }
    let mut order: Vec<usize> = (0..nuc).collect();
    order.sort_by(|&i, &j| {
        rsc[i]
            .total_cmp(&rsc[j])
            .then(frac[i].total_cmp(&frac[j]))
            .then(ang[i].total_cmp(&ang[j]))
    });
    let mut t = ScreenTile {
        g: g as usize,
        p1,
        q1,
        s1,
        table: vec![0f32; nuc],
    };
    for (r, &i) in order.iter().enumerate() {
        let (_, _, _, x, y) = cls[i];
        let k = t.index(x, y);
        t.table[k] = ((r as f64 + 0.5) / nuc as f64) as f32;
    }
    let t = Arc::new(t);
    let mut cache = tile_cache().lock().unwrap();
    // bounded: experimenting with many LPI / shapes must not keep every tile alive
    if cache.len() >= 24 {
        cache.clear();
    }
    cache.insert(key, t.clone());
    t
}
