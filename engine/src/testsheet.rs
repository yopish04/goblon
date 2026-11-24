//! Print test sheet for calibrating a real DTF printer: LPI x shape grid with tone
//! ramps, minimum dot / hole sizes, colour chart and a grey ramp with the reference
//! preset. Every block is rendered by the normal engine (except the minimum-dot block,
//! which deliberately contains dots smaller than the clean-up limit).

use crate::render::{render, GridChoice, Settings, Shirt};
use crate::screen::Shape;

const FONT_W: usize = 5;

/// 5x7 bitmap font (rows top to bottom, 5 bits each, MSB = left)
pub(crate) fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [31, 2, 4, 2, 1, 17, 14],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 30, 1, 1, 17, 14],
        '6' => [6, 8, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 2, 12],
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [28, 18, 17, 17, 17, 18, 28],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 17, 25, 21, 19, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        ',' => [0, 0, 0, 0, 12, 4, 8],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '%' => [24, 25, 2, 4, 8, 19, 3],
        '/' => [1, 1, 2, 4, 8, 16, 16],
        ':' => [0, 12, 12, 0, 12, 12, 0],
        '(' => [2, 4, 8, 8, 8, 4, 2],
        ')' => [8, 4, 2, 2, 2, 4, 8],
        _ => [0; 7],
    }
}

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<u8>,
    ink: [u8; 3],
}

impl Canvas {
    fn text(&mut self, x: usize, y: usize, scale: usize, s: &str) {
        let mut cx = x;
        for ch in s.chars() {
            let g = glyph(ch);
            for (r, bits) in g.iter().enumerate() {
                for c in 0..FONT_W {
                    if bits & (1 << (FONT_W - 1 - c)) != 0 {
                        for dy in 0..scale {
                            for dx in 0..scale {
                                self.set(cx + c * scale + dx, y + r * scale + dy, self.ink);
                            }
                        }
                    }
                }
            }
            cx += (FONT_W + 1) * scale;
        }
    }

    fn set(&mut self, x: usize, y: usize, c: [u8; 3]) {
        if x < self.w && y < self.h {
            let p = (y * self.w + x) * 4;
            self.px[p..p + 3].copy_from_slice(&c);
            self.px[p + 3] = 255;
        }
    }

    fn paste(&mut self, x0: usize, y0: usize, w: usize, h: usize, rgba: &[u8]) {
        for y in 0..h {
            for x in 0..w {
                let q = (y * w + x) * 4;
                if rgba[q + 3] == 255 {
                    self.set(x0 + x, y0 + y, [rgba[q], rgba[q + 1], rgba[q + 2]]);
                }
            }
        }
    }

    fn rect(&mut self, x0: usize, y0: usize, w: usize, h: usize, t: usize) {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                if x < x0 + t || x >= x0 + w - t || y < y0 + t || y >= y0 + h - t {
                    self.set(x, y, self.ink);
                }
            }
        }
    }
}

fn mm(dpi: f64, v: f64) -> usize {
    (v / 25.4 * dpi).round() as usize
}

/// grey (or colour) horizontal ramp 0 -> 100 % as engine input (colour + alpha)
fn ramp(w: usize, h: usize, shirt: Shirt) -> (Vec<f32>, Vec<f32>) {
    let mut c = vec![0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let t = x as f32 / (w - 1) as f32;
            // ink amount t: black shirt -> white ink (light), white shirt -> black ink
            let v = if shirt == Shirt::Hitam { t } else { 1.0 - t };
            for k in 0..3 {
                c[(y * w + x) * 3 + k] = v;
            }
        }
    }
    (c, vec![1.0; w * h])
}

/// Build the sheet. `shirt` = Hitam or Putih. Returns (w, h, rgba).
pub fn test_sheet(shirt: Shirt, dpi: f64) -> (usize, usize, Vec<u8>) {
    let (w, h) = (mm(dpi, 220.0), mm(dpi, 300.0));
    let ink = if shirt == Shirt::Hitam {
        [255, 255, 255]
    } else {
        [0, 0, 0]
    };
    let mut cv = Canvas {
        w,
        h,
        px: vec![0u8; w * h * 4],
        ink,
    };
    let fs = (dpi / 100.0).round().max(2.0) as usize; // font scale (~0.6 mm per pixel row at 300 dpi)
    let margin = mm(dpi, 8.0);
    let name = if shirt == Shirt::Hitam {
        "KAOS HITAM"
    } else {
        "KAOS PUTIH"
    };
    cv.text(
        margin,
        margin,
        fs * 2,
        &format!("STUDIOZEA TEST SHEET - {name} - {} DPI", dpi as u32),
    );
    cv.text(
        margin,
        margin + 18 * fs,
        fs,
        "CETAK 100%, TANPA RESAMPLE DI RIP. CATAT LPI/BENTUK MANA YANG BERSIH SETELAH PRES DAN CUCI.",
    );

    // A: LPI x shape grid with 0-100 % ramps
    let lpis = [25.0, 35.0, 45.0, 55.0, 65.0];
    let label_w = mm(dpi, 18.0);
    let cell_w = (w - 2 * margin - label_w) / Shape::ALL.len();
    let cell_h = mm(dpi, 16.0);
    let top = margin + mm(dpi, 16.0);
    // names on two staggered lines, so long names of narrow columns do not overlap
    for (i, s) in Shape::ALL.iter().enumerate() {
        cv.text(
            margin + label_w + i * cell_w + fs,
            top + (i % 2) * 9 * fs,
            fs,
            &s.name().to_uppercase(),
        );
    }
    let gy0 = top + 19 * fs;
    for (r, &l) in lpis.iter().enumerate() {
        let y = gy0 + r * (cell_h + mm(dpi, 2.0));
        cv.text(
            margin,
            y + cell_h / 2 - 4 * fs,
            fs,
            &format!("{} LPI", l as u32),
        );
        for (i, &s) in Shape::ALL.iter().enumerate() {
            let bw = cell_w - mm(dpi, 2.0);
            let (c, a) = ramp(bw, cell_h, shirt);
            let st = Settings {
                shirt,
                lpi: l,
                shape: s,
                dpi,
                grid: GridChoice::Auto,
                ..Settings::default()
            };
            let o = render(&c, &a, bw, cell_h, &st);
            cv.paste(margin + label_w + i * cell_w, y, bw, cell_h, &o);
        }
    }
    let mut y = gy0 + lpis.len() * (cell_h + mm(dpi, 2.0)) + mm(dpi, 4.0);
    cv.text(margin, y, fs, "0%                                              GRADASI 0-100% PER KOTAK                                             100%");
    y += mm(dpi, 8.0);

    // B: minimum dot and minimum hole (no clean-up on purpose)
    cv.text(
        margin,
        y,
        fs,
        "TITIK MINIMUM (TITIK TERPISAH) DAN LUBANG MINIMUM (DI AREA SOLID), DIAMETER DALAM MM:",
    );
    y += 10 * fs;
    let sizes = [0.10, 0.15, 0.20, 0.25, 0.30, 0.40];
    let block = mm(dpi, 26.0);
    for (i, &d) in sizes.iter().enumerate() {
        let x0 = margin + i * (block + mm(dpi, 4.0));
        cv.text(x0, y, fs, &format!("{d:.2} MM"));
        let by = y + 10 * fs;
        let r = d / 2.0 / 25.4 * dpi;
        let pitch = mm(dpi, 1.6) as f64;
        let half = block / 2;
        // dots on the left half
        let mut yy = pitch / 2.0;
        while yy < block as f64 {
            let mut xx = pitch / 2.0;
            while xx < half as f64 {
                for py in 0..block {
                    for px in 0..half {
                        let dx = px as f64 + 0.5 - xx;
                        let dy = py as f64 + 0.5 - yy;
                        if dx * dx + dy * dy <= r * r {
                            cv.set(x0 + px, by + py, ink);
                        }
                    }
                }
                xx += pitch;
            }
            yy += pitch;
        }
        // holes on the right half
        for py in 0..block {
            for px in half..block {
                let lx = (px - half) as f64 + 0.5;
                let ly = py as f64 + 0.5;
                let fx = (lx / pitch).floor() * pitch + pitch / 2.0;
                let fy = (ly / pitch).floor() * pitch + pitch / 2.0;
                let dx = lx - fx;
                let dy = ly - fy;
                if dx * dx + dy * dy > r * r {
                    cv.set(x0 + px, by + py, ink);
                }
            }
        }
        cv.rect(x0, by, block, block, 1);
    }
    y += 10 * fs + block + mm(dpi, 8.0);

    // C: colour chart with the reference preset (35 LPI) + grey ramp
    cv.text(
        margin,
        y,
        fs,
        "WARNA DENGAN PRESET REFERENSI 35 LPI (BULAT, GRID AUTO):",
    );
    y += 10 * fs;
    let cols: [[f32; 3]; 18] = [
        [1.0, 0.0, 0.0],
        [0.0, 0.6, 0.0],
        [0.0, 0.3, 1.0],
        [1.0, 0.85, 0.0],
        [1.0, 0.0, 0.6],
        [0.0, 0.8, 0.9],
        [0.96, 0.80, 0.69],
        [0.76, 0.53, 0.38],
        [0.45, 0.30, 0.20],
        [0.85, 0.20, 0.25],
        [0.30, 0.65, 0.60],
        [0.60, 0.25, 0.55],
        [0.95, 0.75, 0.20],
        [0.20, 0.50, 0.70],
        [0.55, 0.70, 0.20],
        [0.80, 0.40, 0.10],
        [0.75, 0.75, 0.75],
        [0.35, 0.35, 0.35],
    ];
    let pw = (w - 2 * margin) / 9;
    let ph = mm(dpi, 14.0);
    let mut chart = vec![0f32; (w - 2 * margin) * 2 * ph * 3];
    let cw = w - 2 * margin;
    for (i, col) in cols.iter().enumerate() {
        let (cx, cy) = ((i % 9) * pw, (i / 9) * ph);
        for yy in 0..ph - mm(dpi, 1.0) {
            for xx in 0..pw - mm(dpi, 1.0) {
                let p = ((cy + yy) * cw + cx + xx) * 3;
                chart[p..p + 3].copy_from_slice(col);
            }
        }
    }
    // background of the chart = no ink (shirt colour)
    let bgv = if shirt == Shirt::Hitam { 0.0 } else { 1.0 };
    for (i, v) in chart.iter_mut().enumerate() {
        let p = i / 3;
        let (xx, yy) = (p % cw, p / cw);
        let inside = (xx % pw) < pw - mm(dpi, 1.0)
            && (yy % ph) < ph - mm(dpi, 1.0)
            && (yy / ph) * 9 + xx / pw < cols.len();
        if !inside {
            *v = bgv;
        }
    }
    let st = Settings {
        shirt,
        dpi,
        ..Settings::default()
    };
    let o = render(&chart, &vec![1.0; cw * 2 * ph], cw, 2 * ph, &st);
    cv.paste(margin, y, cw, 2 * ph, &o);
    y += 2 * ph + mm(dpi, 4.0);
    let gh = mm(dpi, 12.0);
    if y + gh < h {
        let (c, a) = ramp(cw, gh, shirt);
        let o = render(&c, &a, cw, gh, &st);
        cv.paste(margin, y, cw, gh, &o);
        cv.text(
            margin,
            y + gh + 2 * fs,
            fs,
            "GRADASI 0-100% PRESET REFERENSI",
        );
    }
    (w, h, cv.px)
}
