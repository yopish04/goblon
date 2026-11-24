//! Exact ports of the scipy.ndimage pieces the engine uses:
//! uniform_filter (reflect), min/max filter (reflect), label (4-connected),
//! binary_dilation (cross), Euclidean feature transform (Maurer, as scipy).

use rayon::prelude::*;

/// scipy NI_ExtendLine, mode 'reflect' (d c b a | a b c d | d c b a)
fn extend_reflect(line: &[f64], before: usize, after: usize) -> Vec<f64> {
    let len = line.len();
    let mut buf = vec![0f64; before + len + after];
    buf[before..before + len].copy_from_slice(line);
    let first = before as isize;
    let last = (before + len) as isize;
    // before
    let mut src = first;
    let mut dst = first - 1;
    let mut sb = before;
    while sb > 0 && src < last {
        buf[dst as usize] = buf[src as usize];
        dst -= 1;
        src += 1;
        sb -= 1;
    }
    let mut src = last - 1;
    while sb > 0 {
        buf[dst as usize] = buf[src as usize];
        dst -= 1;
        src -= 1;
        sb -= 1;
    }
    // after
    let mut src = last - 1;
    let mut dst = last;
    let mut sa = after;
    while sa > 0 && src >= first {
        buf[dst as usize] = buf[src as usize];
        dst += 1;
        src -= 1;
        sa -= 1;
    }
    let mut src = first;
    while sa > 0 {
        buf[dst as usize] = buf[src as usize];
        dst += 1;
        src += 1;
        sa -= 1;
    }
    buf
}

/// scipy NI_UniformFilter1D on one line (origin 0)
fn uniform_line(line: &[f64], size: usize, out: &mut [f32]) {
    let size1 = size / 2;
    let size2 = size - size1 - 1;
    let buf = extend_reflect(line, size1, size2);
    let fs = size as f64;
    let mut tmp = 0.0f64;
    for v in buf.iter().take(size) {
        tmp += *v;
    }
    out[0] = (tmp / fs) as f32;
    for ll in 1..line.len() {
        tmp += buf[ll + size - 1] - buf[ll - 1];
        out[ll] = (tmp / fs) as f32;
    }
}

/// scipy.ndimage.uniform_filter1d(x, size) for a 1-D f32 array
pub fn uniform_filter1d(x: &[f32], size: usize) -> Vec<f32> {
    let line: Vec<f64> = x.iter().map(|&v| v as f64).collect();
    let mut out = vec![0f32; x.len()];
    uniform_line(&line, size, &mut out);
    out
}

/// scipy.ndimage.uniform_filter(img, size) for a 2-D f32 image (axis 0, then axis 1)
pub fn uniform_filter(img: &[f32], w: usize, h: usize, size: usize) -> Vec<f32> {
    if size <= 1 {
        return img.to_vec();
    }
    // axis 0: columns
    let mut tmp = vec![0f32; w * h];
    let cols: Vec<Vec<f32>> = (0..w)
        .into_par_iter()
        .map(|x| {
            let line: Vec<f64> = (0..h).map(|y| img[y * w + x] as f64).collect();
            let mut out = vec![0f32; h];
            uniform_line(&line, size, &mut out);
            out
        })
        .collect();
    for (x, col) in cols.iter().enumerate() {
        for y in 0..h {
            tmp[y * w + x] = col[y];
        }
    }
    // axis 1: rows
    let mut out = vec![0f32; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let line: Vec<f64> = tmp[y * w..(y + 1) * w].iter().map(|&v| v as f64).collect();
        uniform_line(&line, size, row);
    });
    out
}

#[inline]
fn reflect_idx(i: isize, len: isize) -> usize {
    // half-sample symmetric for small overshoot (filters here are much shorter than lines)
    let mut i = i;
    if len <= 1 {
        return 0;
    }
    loop {
        if i < 0 {
            i = -i - 1;
        } else if i >= len {
            i = 2 * len - i - 1;
        } else {
            return i as usize;
        }
    }
}

fn minmax_1d<T: Copy + PartialOrd + Send + Sync>(
    img: &[T],
    w: usize,
    h: usize,
    size: usize,
    axis: usize,
    want_max: bool,
) -> Vec<T> {
    let size1 = (size / 2) as isize;
    let size2 = (size - size / 2 - 1) as isize;
    let mut out = img.to_vec();
    let pick = |a: T, b: T| -> T {
        if want_max {
            if b > a {
                b
            } else {
                a
            }
        } else if b < a {
            b
        } else {
            a
        }
    };
    if axis == 1 {
        out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            let src = &img[y * w..(y + 1) * w];
            for x in 0..w as isize {
                let mut m = src[reflect_idx(x - size1, w as isize)];
                for k in (x - size1 + 1)..=(x + size2) {
                    m = pick(m, src[reflect_idx(k, w as isize)]);
                }
                row[x as usize] = m;
            }
        });
    } else {
        out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            for (x, o) in row.iter_mut().enumerate() {
                let yy = y as isize;
                let mut m = img[reflect_idx(yy - size1, h as isize) * w + x];
                for k in (yy - size1 + 1)..=(yy + size2) {
                    m = pick(m, img[reflect_idx(k, h as isize) * w + x]);
                }
                *o = m;
            }
        });
    }
    out
}

/// scipy maximum_filter / minimum_filter with a square `size`, mode reflect (separable)
pub fn max_filter<T: Copy + PartialOrd + Send + Sync>(
    img: &[T],
    w: usize,
    h: usize,
    size: usize,
) -> Vec<T> {
    let a = minmax_1d(img, w, h, size, 0, true);
    minmax_1d(&a, w, h, size, 1, true)
}

pub fn min_filter<T: Copy + PartialOrd + Send + Sync>(
    img: &[T],
    w: usize,
    h: usize,
    size: usize,
) -> Vec<T> {
    let a = minmax_1d(img, w, h, size, 0, false);
    minmax_1d(&a, w, h, size, 1, false)
}

/// 4-connected components of `mask`. Returns (label per pixel, 0 = background,
/// size per label with sizes[0] = number of background pixels).
pub fn label(mask: &[bool], w: usize, h: usize) -> (Vec<u32>, Vec<u32>) {
    let mut lab = vec![0u32; w * h];
    let mut sizes = vec![0u32];
    let mut stack: Vec<usize> = Vec::new();
    let mut next = 1u32;
    for start in 0..w * h {
        if !mask[start] || lab[start] != 0 {
            continue;
        }
        let mut count = 0u32;
        lab[start] = next;
        stack.push(start);
        while let Some(p) = stack.pop() {
            count += 1;
            let x = p % w;
            let y = p / w;
            if x > 0 && mask[p - 1] && lab[p - 1] == 0 {
                lab[p - 1] = next;
                stack.push(p - 1);
            }
            if x + 1 < w && mask[p + 1] && lab[p + 1] == 0 {
                lab[p + 1] = next;
                stack.push(p + 1);
            }
            if y > 0 && mask[p - w] && lab[p - w] == 0 {
                lab[p - w] = next;
                stack.push(p - w);
            }
            if y + 1 < h && mask[p + w] && lab[p + w] == 0 {
                lab[p + w] = next;
                stack.push(p + w);
            }
        }
        sizes.push(count);
        next += 1;
    }
    sizes[0] = mask.iter().filter(|&&m| !m).count() as u32;
    (lab, sizes)
}

/// binary_dilation with the cross structure, border value 0
pub fn dilate_cross(mask: &[bool], w: usize, h: usize, iterations: usize) -> Vec<bool> {
    let mut cur = mask.to_vec();
    for _ in 0..iterations {
        let prev = cur.clone();
        cur.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
            for x in 0..w {
                let p = y * w + x;
                row[x] = prev[p]
                    || (x > 0 && prev[p - 1])
                    || (x + 1 < w && prev[p + 1])
                    || (y > 0 && prev[p - w])
                    || (y + 1 < h && prev[p + w]);
            }
        });
    }
    cur
}

/// scipy _VoronoiFT for one line. `f` holds (dim0, dim1) per element; `d` = axis.
fn voronoi_line(f: &mut [[i64; 2]], d: usize, coor: [i64; 2]) {
    let len = f.len();
    let src: Vec<[i64; 2]> = f.to_vec();
    let mut g: Vec<usize> = vec![0; len];
    let mut l: isize = -1;
    for ii in 0..len {
        if src[ii][0] >= 0 {
            let fd = src[ii][d] as f64;
            let mut wr = 0.0f64;
            for jj in 0..2 {
                if jj != d {
                    let tw = (src[ii][jj] - coor[jj]) as f64;
                    wr += tw * tw;
                }
            }
            while l >= 1 {
                let idx1 = g[l as usize];
                let f1 = src[idx1][d] as f64;
                let idx2 = g[(l - 1) as usize];
                let a = f1 - src[idx2][d] as f64;
                let b = fd - f1;
                let c = a + b;
                let mut ur = 0.0f64;
                let mut vr = 0.0f64;
                for jj in 0..2 {
                    if jj != d {
                        let cc = coor[jj] as f64;
                        let tu = src[idx2][jj] as f64 - cc;
                        let tv = src[idx1][jj] as f64 - cc;
                        ur += tu * tu;
                        vr += tv * tv;
                    }
                }
                if c * vr - b * ur - a * wr - a * b * c <= 0.0 {
                    break;
                }
                l -= 1;
            }
            l += 1;
            g[l as usize] = ii;
        }
    }
    let maxl = l;
    if maxl >= 0 {
        let mut l: isize = 0;
        for ii in 0..len {
            let delta = |site: usize| -> f64 {
                let mut s = 0.0f64;
                for jj in 0..2 {
                    let t = if jj == d {
                        (src[site][jj] - ii as i64) as f64
                    } else {
                        (src[site][jj] - coor[jj]) as f64
                    };
                    s += t * t;
                }
                s
            };
            let mut delta1 = delta(g[l as usize]);
            while l < maxl {
                let delta2 = delta(g[(l + 1) as usize]);
                if delta1 <= delta2 {
                    break;
                }
                delta1 = delta2;
                l += 1;
            }
            f[ii] = src[g[l as usize]];
        }
    }
}

/// scipy distance_transform_edt(input, return_indices=True) feature transform:
/// for every `true` pixel the coordinates (y, x) of the nearest `false` pixel.
pub fn feature_transform(input: &[bool], w: usize, h: usize) -> Vec<[i64; 2]> {
    let mut ft = vec![[0i64; 2]; w * h];
    // pass 1 (d = 0): per column
    let cols: Vec<Vec<[i64; 2]>> = (0..w)
        .into_par_iter()
        .map(|x| {
            let mut f: Vec<[i64; 2]> = (0..h)
                .map(|y| {
                    if input[y * w + x] {
                        [-1, 0]
                    } else {
                        [y as i64, x as i64]
                    }
                })
                .collect();
            voronoi_line(&mut f, 0, [0, x as i64]);
            f
        })
        .collect();
    for (x, col) in cols.iter().enumerate() {
        for y in 0..h {
            ft[y * w + x] = col[y];
        }
    }
    // pass 2 (d = 1): per row
    ft.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        voronoi_line(row, 1, [y as i64, 0]);
    });
    ft
}

/// Euclidean distance of every `true` pixel to the nearest `false` pixel (0 for false)
pub fn edt(input: &[bool], w: usize, h: usize) -> Vec<f64> {
    let ft = feature_transform(input, w, h);
    (0..w * h)
        .map(|p| {
            let dy = (ft[p][0] - (p / w) as i64) as f64;
            let dx = (ft[p][1] - (p % w) as i64) as f64;
            (dy * dy + dx * dx).sqrt()
        })
        .collect()
}
