//! "Titik warna asli" (Settings::true_dots): one screen, dots in the design's own
//! colours, light parts solid, off = render unchanged.

use studiozea_engine::{render, Settings, Shape, Shirt, TrueDots};

/// w x h image: left half a flat colour, right half white, a dark ramp at the bottom
fn image(w: usize, h: usize, col: [f32; 3]) -> (Vec<f32>, Vec<f32>) {
    let mut c = vec![0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let p = (y * w + x) * 3;
            let v = if y >= h * 3 / 4 {
                let k = x as f32 / w as f32;
                [col[0] * k, col[1] * k, col[2] * k]
            } else if x < w / 2 {
                col
            } else {
                [1.0; 3]
            };
            c[p..p + 3].copy_from_slice(&v);
        }
    }
    (c, vec![1.0; w * h])
}

fn settings(td: Option<TrueDots>) -> Settings {
    Settings { shirt: Shirt::Hitam, lpi: 12.0, dpi: 150.0, shape: Shape::BulatMurni, angle: Some(22.5), true_dots: td, ..Settings::default() }
}

fn coverage(o: &[u8], w: usize, x0: usize, x1: usize, y0: usize, y1: usize) -> f64 {
    let mut n = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            n += (o[(y * w + x) * 4 + 3] > 0) as usize;
        }
    }
    n as f64 / ((x1 - x0) * (y1 - y0)) as f64
}

#[test]
fn dots_keep_the_design_colour_and_white_is_solid() {
    let (w, h) = (240, 240);
    let col = [88.0 / 255.0, 192.0 / 255.0, 252.0 / 255.0]; // sky blue: not a pure ink colour
    let (c, a) = image(w, h, col);
    let plain_dots = TrueDots { lift: 0.0, size: 0.85, ..TrueDots::default() };
    let o = render(&c, &a, w, h, &settings(Some(plain_dots)));
    let want = [88u8, 192, 252, 255];
    for p in 0..w * h {
        let px = &o[p * 4..p * 4 + 4];
        assert!(px[3] == 0 || px[3] == 255, "no semi-transparency");
        let (x, y) = (p % w, p / w);
        if px[3] == 255 && y < h * 3 / 4 && x < w / 2 - 2 {
            assert_eq!(px, want, "dot at {x},{y} keeps the design colour");
        }
    }
    // the blue: dots of about the chosen size (85 %, calibrated), not solid, not empty
    let blue = coverage(&o, w, 10, w / 2 - 10, 10, h * 3 / 4 - 10);
    assert!((0.7..0.97).contains(&blue), "blue coverage {blue}");
    // white: lighter than "solid from" -> solid
    let white = coverage(&o, w, w / 2 + 10, w - 10, 10, h * 3 / 4 - 10);
    assert!(white > 0.99, "white coverage {white}");
    // darker shades get smaller dots: the ramp grows from left to right
    let dark = coverage(&o, w, 20, 80, h * 3 / 4 + 5, h - 5);
    let light = coverage(&o, w, 160, 230, h * 3 / 4 + 5, h - 5);
    assert!(dark < light, "ramp {dark} < {light}");
    // bigger dot size = more ink; never solid = the white gets dots too
    let big = render(&c, &a, w, h, &settings(Some(TrueDots { size: 0.95, ..plain_dots })));
    assert!(coverage(&big, w, 10, w / 2 - 10, 10, h * 3 / 4 - 10) > blue);
    let never = render(&c, &a, w, h, &settings(Some(TrueDots { solid: 1.0, ..plain_dots })));
    assert!(coverage(&never, w, w / 2 + 10, w - 10, 10, h * 3 / 4 - 10) < 0.95);
}

#[test]
fn off_or_halftone_off_changes_nothing() {
    let (w, h) = (160, 120);
    let (c, a) = image(w, h, [0.9, 0.3, 0.2]);
    let plain = render(&c, &a, w, h, &settings(None));
    let s = Settings { true_dots: None, ..settings(None) };
    assert_eq!(render(&c, &a, w, h, &s), plain);
    // halftone off wins over true-colour dots
    let off = Settings { halftone_area: Some(0.0), ..settings(None) };
    let both = Settings { halftone_area: Some(0.0), ..settings(Some(TrueDots::default())) };
    assert_eq!(render(&c, &a, w, h, &both), render(&c, &a, w, h, &off));
    assert!(!settings(Some(TrueDots::default())).pure_colours());
}

#[test]
fn settings_without_the_field_still_load() {
    // projects / presets saved before this option: no "true_dots" key
    let s: Settings = serde_json::from_str(r#"{"lpi": 20}"#).unwrap();
    assert!(s.true_dots.is_none());
    let s: Settings = serde_json::from_str(r#"{"true_dots": {"size": 0.6}}"#).unwrap();
    assert_eq!(s.true_dots, Some(TrueDots { size: 0.6, solid: 0.92, lift: 1.0 }));
}

/// regression: a full-colour painting on a navy shirt faded because muted / mid colours
/// (grey stone, skin, dark purple) got small dots. The dot carries the colour, so only
/// fades into the shirt (alpha) and colours right next to the shirt colour get smaller
#[test]
fn muted_colours_on_a_coloured_shirt_keep_full_dots() {
    let navy = [16.0 / 255.0, 24.0 / 255.0, 60.0 / 255.0];
    let (w, h) = (240, 200);
    let grey = [0.42, 0.44, 0.46];
    let mut c = vec![0f32; w * h * 3];
    let mut a = vec![1f32; w * h];
    for p in 0..w * h {
        let (x, y) = (p % w, p / w);
        c[p * 3..p * 3 + 3].copy_from_slice(&grey);
        if y >= h / 2 {
            a[p] = x as f32 / w as f32; // the grey fades into the shirt from left to right
        }
    }
    let td = TrueDots { lift: 0.0, size: 0.85, ..TrueDots::default() };
    let s = Settings { shirt: Shirt::Warna(navy), ..settings(Some(td)) };
    let o = render(&c, &a, w, h, &s);
    let full = coverage(&o, w, 10, w - 10, 10, h / 2 - 10);
    let plain = render(&c, &a, w, h, &Settings { shirt: Shirt::Warna(navy), ..settings(Some(td)) });
    assert_eq!(o, plain);
    assert!(full > 0.7, "grey on navy: coverage {full} (would fade the print)");
    // the fade: smaller dots towards the shirt, but still the grey itself, never darker
    let weak = coverage(&o, w, 30, 90, h / 2 + 10, h - 10);
    let strong = coverage(&o, w, 160, 230, h / 2 + 10, h - 10);
    assert!(weak < strong, "fade {weak} < {strong}");
    let want = grey.map(|v| (v * 255.0f32).round() as u8);
    for p in 0..w * h {
        if o[p * 4 + 3] == 255 {
            assert_eq!(&o[p * 4..p * 4 + 3], &want, "dot colour at {},{}", p % w, p / w);
        } else {
            assert_eq!(o[p * 4 + 3], 0, "no semi-transparency");
        }
    }
}

/// "naik warna": a dark red on a black shirt prints as smaller dots of the same red at
/// full brightness (hue / saturation kept, like colour-halftone band shirts); the ink
/// average stays the original red. Never semi-transparent
#[test]
fn colour_lift_gives_bright_dots_and_keeps_the_average() {
    let (w, h) = (240, 160);
    let red = [190.0 / 255.0, 63.0 / 255.0, 51.0 / 255.0];
    let c: Vec<f32> = (0..w * h).flat_map(|_| red).collect();
    let a = vec![1f32; w * h];
    let s = settings(Some(TrueDots { lift: 1.0, size: 1.0, solid: 1.0 }));
    let o = render(&c, &a, w, h, &s);
    let mut sum = [0f64; 3];
    let mut n = 0usize;
    for p in 0..w * h {
        let px = &o[p * 4..p * 4 + 4];
        assert!(px[3] == 0 || px[3] == 255, "no semi-transparency");
        let (x, y) = (p % w, p / w);
        if px[3] == 255 {
            // full brightness, same hue: 190,63,51 / 0.745 = 255,85,68
            assert!(px[0] == 255 && (84..=86).contains(&px[1]) && (67..=69).contains(&px[2]), "dot colour {px:?} at {x},{y}");
        }
        if (20..w - 20).contains(&x) && (20..h - 20).contains(&y) {
            for ch in 0..3 {
                sum[ch] += px[ch] as f64 * (px[3] as f64 / 255.0);
            }
            n += 1;
        }
    }
    let avg = sum.map(|v| v / n as f64);
    // the dots cover ~75 % (calibrated screen): the average is close to the original red
    assert!((avg[0] - 190.0).abs() < 20.0 && (avg[1] - 63.0).abs() < 10.0, "average {avg:?}");
    // lift 0: the dots are the design colour itself
    let o0 = render(&c, &a, w, h, &settings(Some(TrueDots { lift: 0.0, size: 1.0, solid: 1.0 })));
    assert!(o0.chunks(4).filter(|p| p[3] == 255).all(|p| p[..3] == [190, 63, 51]));
}
