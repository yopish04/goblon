//! Sablon colour separation: inks, underbase, films and the simulated print.
use studiozea_engine::render::{Settings, Shirt};
use studiozea_engine::sablon::{auto_inks, preset_inks, save_film, separate, DotStyle, Ink, InkSet, Method, Role, SepSettings};
use studiozea_engine::Prepared;

/// 240 x 160 design: red, yellow, blue and white patches, a grey ramp, on a transparent
/// frame (8 px)
fn design() -> Prepared {
    let (w, h) = (240, 160);
    let mut rgb = vec![0f32; w * h * 3];
    let mut alpha = vec![0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let q = y * w + x;
            if x < 8 || y < 8 || x >= w - 8 || y >= h - 8 {
                continue;
            }
            alpha[q] = 1.0;
            let c = if y < 80 {
                match x * 4 / w {
                    0 => [0.85, 0.08, 0.08],
                    1 => [0.98, 0.85, 0.1],
                    2 => [0.1, 0.25, 0.8],
                    _ => [1.0, 1.0, 1.0],
                }
            } else {
                let v = (x - 8) as f32 / (w - 16) as f32;
                [v, v, v]
            };
            rgb[q * 3..q * 3 + 3].copy_from_slice(&c);
        }
    }
    Prepared { w, h, pre_w: w, src_box: [0, 0, w, h], rgb, alpha, bg: None, bg_used: None, solid: None }
}

fn settings(shirt: Shirt) -> Settings {
    Settings { shirt, dpi: 300.0, ..Settings::default() }
}

#[test]
fn inks_cover_the_design_in_print_order() {
    let p = design();
    let inks = auto_inks(&p, [0.0; 3], 5, true);
    println!("{:?}", inks.iter().map(|i| i.rgb).collect::<Vec<_>>());
    assert!(inks.len() >= 4 && inks.len() <= 5);
    // the highlight white last on a black shirt
    assert_eq!(inks.last().unwrap().rgb, [255, 255, 255]);
    // a red, a yellow and a blue ink
    let has = |f: &dyn Fn([u8; 3]) -> bool| inks.iter().any(|i| f(i.rgb));
    assert!(has(&|c| c[0] > 170 && c[1] < 90 && c[2] < 90), "red");
    assert!(has(&|c| c[0] > 200 && c[1] > 170 && c[2] < 110), "yellow");
    assert!(has(&|c| c[2] > 140 && c[0] < 100), "blue");
    // no black ink on a black shirt (the shirt is the black)
    assert!(!has(&|c| c.iter().all(|&v| v < 40)));
    // on a white shirt: no white ink
    let w = auto_inks(&p, [1.0; 3], 5, true);
    assert!(w.iter().all(|i| i.rgb != [255, 255, 255]));
}

#[test]
fn separates_close_to_the_design_with_base_under_the_colours() {
    let p = design();
    for method in [Method::Simulasi, Method::Index, Method::Spot] {
        let sep = SepSettings { method, colors: 5, ..Default::default() };
        let r = separate(&p, &settings(Shirt::Hitam), &sep);
        println!("{method:?}: delta E {:.2}, films {}", r.delta_e, r.films.len());
        assert_eq!(r.films[0].role, Role::Underbase);
        assert_eq!(r.films.last().unwrap().role, Role::Highlight);
        let lim = if method == Method::Spot { 14.0 } else { 9.0 };
        assert!(r.delta_e < lim, "{method:?} delta E {}", r.delta_e);
        // nothing prints outside the design (the transparent frame)
        for f in &r.films {
            for y in 0..p.h {
                for x in [0, 3, 7, p.w - 1] {
                    assert!(!f.bits.get(y * p.w + x), "{:?} prints outside", f.role);
                }
            }
        }
        // the base is choked: never in the outermost design pixel ring
        let base = &r.films[0].bits;
        for y in 8..p.h - 8 {
            assert!(!base.get(y * p.w + 8) && !base.get(y * p.w + p.w - 9), "base at the edge, row {y}");
        }
        // the red patch has a dense base (bright red on black needs white below)
        let mut on = 0;
        for y in 20..60 {
            for x in 20..40 {
                on += base.get(y * p.w + x) as usize;
            }
        }
        assert!(on * 10 >= 20 * 40 * 7, "{method:?}: base under red {on}");
        // index / spot: one top ink per pixel at most
        if method != Method::Simulasi {
            for q in 0..p.w * p.h {
                let n = r.films[1..].iter().filter(|f| f.bits.get(q)).count();
                assert!(n <= 1, "{method:?}: {n} inks on pixel {q}");
            }
        }
    }
}

#[test]
fn white_shirt_needs_no_base_and_results_are_repeatable() {
    let p = design();
    let sep = SepSettings { colors: 4, ..Default::default() };
    let a = separate(&p, &settings(Shirt::Putih), &sep);
    assert!(a.films.iter().all(|f| f.role == Role::Ink), "no base, no highlight on white");
    let b = separate(&p, &settings(Shirt::Putih), &sep);
    assert_eq!(a.preview, b.preview);
    // forced base on a white shirt works too
    let c = separate(&p, &settings(Shirt::Putih), &SepSettings { underbase: Some(true), ..sep });
    assert_eq!(c.films[0].role, Role::Underbase);
}

#[test]
fn film_png_has_the_margin_and_marks() {
    let p = design();
    let r = separate(&p, &settings(Shirt::Hitam), &SepSettings { colors: 3, ..Default::default() });
    let path = std::env::temp_dir().join(format!("sz_film_{}.png", std::process::id()));
    save_film(&r.films[1], 300.0, 12.0, "2 TEST", &path).unwrap();
    let img = studiozea_engine::io::load_rgba8(&path).unwrap();
    let pad = (12.0 / 25.4 * 300.0f64).round() as usize;
    assert_eq!((img.w, img.h), (p.w + 2 * pad, p.h + 2 * pad));
    // a registration mark centre (top middle) is black
    let q = ((pad / 2) * img.w + img.w / 2) * 4;
    assert_eq!(img.px[q], 0);
    let _ = std::fs::remove_file(&path);
}

/// OKLab of a linear colour (for the checks below)
fn ok(c: [f32; 3]) -> [f32; 3] {
    let l = (0.4122 * c[0] + 0.5363 * c[1] + 0.0514 * c[2]).cbrt();
    let m = (0.2119 * c[0] + 0.6807 * c[1] + 0.1074 * c[2]).cbrt();
    let s = (0.0883 * c[0] + 0.2817 * c[1] + 0.63 * c[2]).cbrt();
    [0.2105 * l + 0.7936 * m - 0.0041 * s, 1.978 * l - 2.4286 * m + 0.4506 * s, 0.0259 * l + 0.7828 * m - 0.8087 * s]
}

fn lin(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

#[test]
fn gradients_print_without_bands() {
    // teal -> blue -> purple -> pink across, the kind of gradient where ink mixes change
    let (w, h) = (600, 120);
    let stops = [[0.02f32, 0.76, 0.73], [0.2, 0.55, 0.9], [0.55, 0.2, 0.85], [1.0, 0.15, 0.6]];
    let mut rgb = vec![0f32; w * h * 3];
    for y in 0..h {
        for x in 0..w {
            let f = x as f32 / (w - 1) as f32 * 3.0;
            let (i, t) = ((f.floor() as usize).min(2), f - (f.floor()).min(2.0));
            for c in 0..3 {
                rgb[(y * w + x) * 3 + c] = stops[i][c] * (1.0 - t) + stops[i + 1][c] * t;
            }
        }
    }
    let p = Prepared { w, h, pre_w: w, src_box: [0, 0, w, h], rgb: rgb.clone(), alpha: vec![1.0; w * h], bg: None, bg_used: None, solid: None };
    let r = separate(&p, &settings(Shirt::Hitam), &SepSettings { colors: 5, ..Default::default() });
    // design vs simulated print, averaged over 12 x 12 px blocks (two 55 LPI cells at 300 dpi)
    let b = 12;
    let mut worst = 0f32;
    let mut sum = 0f32;
    let mut n = 0f32;
    for by in 1..h / b - 1 {
        for bx in 0..w / b {
            let (mut d, mut s) = ([0f32; 3], [0f32; 3]);
            for y in by * b..(by + 1) * b {
                for x in bx * b..(bx + 1) * b {
                    let q = y * w + x;
                    for c in 0..3 {
                        d[c] += lin(rgb[q * 3 + c]);
                        s[c] += if r.preview[q * 4 + 3] == 255 { lin(r.preview[q * 4 + c] as f32 / 255.0) } else { 0.0 };
                    }
                }
            }
            let (a, z) = (ok(d.map(|v| v / (b * b) as f32)), ok(s.map(|v| v / (b * b) as f32)));
            let e = ((a[0] - z[0]).powi(2) + (a[1] - z[1]).powi(2) + (a[2] - z[2]).powi(2)).sqrt() * 100.0;
            worst = worst.max(e);
            sum += e;
            n += 1.0;
        }
    }
    println!("gradient: mean delta E {:.2}, worst block {:.2}", sum / n, worst);
    assert!(sum / n < 4.0, "mean {}", sum / n);
    assert!(worst < 10.0, "a band: worst block {worst}");
}

#[test]
fn ink_sets_fit_the_shirt() {
    let grey = [140, 140, 140];
    let white = [255, 255, 255];
    let blackish = |i: &Ink| i.rgb.iter().all(|&v| v < 40);
    // black shirt: grey, highlight white last, no black
    let (k, ba) = preset_inks(InkSet::Dasar, [0.0; 3], true);
    assert_eq!(ba, 0);
    assert_eq!(k.len(), 8);
    assert!(k.iter().any(|i| i.rgb == grey));
    assert_eq!(k.last().unwrap().rgb, white);
    assert!(!k.iter().any(blackish));
    // light to dark: yellow first
    assert_eq!(k[0].rgb, [255, 212, 0]);
    // grey first, before the base
    let (k, ba) = preset_inks(InkSet::DasarAbuDulu, [0.0; 3], true);
    assert_eq!((ba, k[0].rgb), (1, grey));
    // white shirt: black last, no white, no grey, nothing before a base
    let (k, ba) = preset_inks(InkSet::DasarAbuDulu, [1.0; 3], true);
    assert_eq!(ba, 0);
    assert!(blackish(k.last().unwrap()));
    assert!(!k.iter().any(|i| i.rgb == white || i.rgb == grey));
    // cheap set on black: three colours + highlight
    assert_eq!(preset_inks(InkSet::Hemat, [0.0; 3], true).0.len(), 4);
    // no ink of the shirt colour (red shirt, red ink)
    let (k, _) = preset_inks(InkSet::Hemat, [226.0 / 255.0, 30.0 / 255.0, 38.0 / 255.0], false);
    assert!(!k.iter().any(|i| i.rgb == [226, 30, 38]));
    assert!(k.iter().any(blackish), "black on a coloured shirt");
}

#[test]
fn unused_inks_are_dropped_and_dot_styles_work() {
    let p = design();
    for style in [DotStyle::SatuSudut, DotStyle::Roset, DotStyle::Interlock] {
        let sep = SepSettings { ink_set: InkSet::Dasar, dot_style: style, ..Default::default() };
        let r = separate(&p, &settings(Shirt::Hitam), &sep);
        let srcs: Vec<usize> = r.films.iter().filter_map(|f| f.src).collect();
        println!("{style:?}: delta E {:.2}, films {}, inks used {srcs:?}", r.delta_e, r.films.len());
        assert_eq!(r.inks.len(), 8);
        // the design has no use for all 8: some are dropped
        assert!(srcs.len() < 8, "{srcs:?}");
        // still in print order and pointing at the right inks
        assert!(srcs.windows(2).all(|w| w[0] < w[1]));
        assert!(r.films.iter().all(|f| f.src.is_none_or(|i| r.inks[i] == f.ink)));
        assert!(r.delta_e < 10.0, "{style:?} delta E {}", r.delta_e);
        let angles: Vec<f64> = r.films.iter().map(|f| f.angle.unwrap()).collect();
        match style {
            DotStyle::Roset => assert!(angles[1..].iter().all(|&a| a != angles[0])),
            _ => assert!(angles.iter().all(|&a| a == angles[0])),
        }
        assert_eq!(r.films.iter().any(|f| f.inverted), style == DotStyle::Interlock);
    }
    // nothing dropped when switched off
    let sep = SepSettings { ink_set: InkSet::Dasar, drop_below: 0.0, ..Default::default() };
    assert_eq!(separate(&p, &settings(Shirt::Hitam), &sep).films.len(), 9);
}

#[test]
fn base_position_expand_and_boost() {
    let p = design();
    let ink = |rgb| Ink::new(rgb, 0.6);
    let inks = vec![ink([140, 140, 140]), ink([226, 30, 38]), ink([255, 212, 0]), ink([30, 70, 180]), Ink::new([255, 255, 255], 0.85)];
    let sep = SepSettings { method: Method::Spot, inks: inks.clone(), base_after: 1, drop_below: 0.0, ..Default::default() };
    let r = separate(&p, &settings(Shirt::Hitam), &sep);
    let roles: Vec<Role> = r.films.iter().map(|f| f.role).collect();
    assert_eq!(roles, [Role::Ink, Role::Underbase, Role::Ink, Role::Ink, Role::Ink, Role::Highlight]);
    assert_eq!(r.films[0].ink.rgb, [140, 140, 140]);
    // expand: the colour films grow, the base and the highlight do not
    let e = separate(&p, &settings(Shirt::Hitam), &SepSettings { expand_px: 2, ..sep.clone() });
    for (a, b) in r.films.iter().zip(&e.films) {
        let (ca, cb) = (a.bits.coverage(), b.bits.coverage());
        match a.role {
            Role::Ink if ca > 0.01 => assert!(cb > ca, "{:?} {ca} -> {cb}", a.ink.rgb),
            Role::Ink => {}
            _ => assert_eq!(ca, cb, "{:?}", a.role),
        }
    }
    // boost one ink by hand (simulated process): its film prints more (solid parts
    // stay solid)
    let sim = SepSettings { method: Method::Simulasi, ..sep };
    let a = separate(&p, &settings(Shirt::Hitam), &sim);
    let mut boosted = inks.clone();
    boosted[3].boost = 0.4;
    let b = separate(&p, &settings(Shirt::Hitam), &SepSettings { inks: boosted, ..sim });
    let blue = |r: &studiozea_engine::sablon::Separation| r.films.iter().find(|f| f.src == Some(3)).unwrap().bits.coverage();
    assert!(blue(&b) > blue(&a) * 1.05, "{} -> {}", blue(&a), blue(&b));
}
