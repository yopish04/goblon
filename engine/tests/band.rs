//! Band / window rendering must give exactly the pixels of the full-image render
//! (same golden inputs and settings as tests/golden.rs, many band and window cuts).

use std::path::{Path, PathBuf};

use serde_json::Value;
use studiozea_engine::{
    band::{render_packed, Plan, Resizer},
    bg, distress, io, noise::Grain, render, resize, GridChoice, KMode, Pattern, Prepared, Settings, TrueDots, Shape, Shirt,
};

fn settings(p: &Value) -> Settings {
    let shirt = match &p["shirt"] {
        Value::String(s) if s == "hitam" => Shirt::Hitam,
        Value::String(s) if s == "putih" => Shirt::Putih,
        Value::Array(a) => Shirt::Warna([0, 1, 2].map(|i| a[i].as_f64().unwrap() as f32)),
        x => panic!("shirt {x}"),
    };
    let pattern = match p["pattern"].as_str().unwrap_or("am") {
        "am" => Pattern::Am,
        "fm" => Pattern::Fm,
        "grain-halus" => Pattern::Grain(Grain::Halus),
        "grain-sedang" | "grain" => Pattern::Grain(Grain::Sedang),
        "grain-kasar" => Pattern::Grain(Grain::Kasar),
        "bayer" => Pattern::Bayer,
        "difusi" => Pattern::Difusi,
        "interlock" => Pattern::Interlock,
        x => panic!("pattern {x}"),
    };
    Settings {
        shirt,
        lpi: p["lpi"].as_f64().unwrap_or(35.0),
        shape: Shape::parse(p["shape"].as_str().unwrap_or("bulat")).unwrap(),
        dpi: p["dpi"].as_f64().unwrap_or(300.0),
        // studiozea_ref.render defaults to 'presisi'
        grid: match p["grid"].as_str().unwrap_or("presisi") {
            "rapi" => GridChoice::Rapi,
            _ => GridChoice::Presisi,
        },
        pattern,
        k: match p["k"].as_str().unwrap_or("off") {
            "gcr" => KMode::Gcr,
            "garis" => KMode::Garis,
            _ => KMode::Off,
        },
        knockout: p["knockout"].as_f64().unwrap_or(0.0) as f32,
        edges: p["edges"].as_bool().unwrap_or(false),
        angle: p["angle"].as_f64(),
        rosette: p["rosette"].as_bool(),
        halftone_area: p["halftone_area"].as_f64().map(|v| v as f32),
        shirt_cut: p["shirt_cut"].as_f64().unwrap_or(0.12) as f32,
        resample: if p["resample"].as_str() == Some("bicubic") {
            studiozea_engine::resize::Resample::Bicubic
        } else {
            studiozea_engine::resize::Resample::Lanczos
        },
        ..Settings::default()
    }
}

/// two built-in library textures: one knocks holes, one fades (rotated, mirrored)
fn texture_stack(seed: u32) -> Vec<distress::Layer> {
    let names = studiozea_engine::library::names();
    if names.is_empty() {
        return Vec::new();
    }
    let mut a = distress::Layer::new(&format!("lib:{}", names[seed as usize % names.len()]));
    a.mode = Some(distress::Mode::Lubang);
    a.angle = Some(30.0 * seed as f64);
    let mut b = distress::Layer::new(&format!("lib:{}", names[(seed as usize + 1) % names.len()]));
    b.mode = Some(distress::Mode::Pudar);
    b.mirror = Some(true);
    b.zoom = Some(0.7);
    vec![a, b]
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// deterministic pseudo random numbers (no extra dependency)
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

#[test]
fn bands_and_windows_match_full_render() {
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(golden_dir().join("cases.json")).unwrap())
            .unwrap();
    let mut rng = Lcg(7);
    let mut bad = Vec::new();
    for (ci, case) in cases.iter().enumerate() {
        let name = case["name"].as_str().unwrap();
        let p = &case["params"];
        let src = io::load(&golden_dir().join(case["input"].as_str().unwrap())).unwrap();
        let (w0, h0, mut rgb, mut alpha) = (src.w, src.h, src.rgb.clone(), src.alpha.clone());
        if p["remove_bg"].as_bool().unwrap_or(false) {
            let r = bg::remove_bg(&rgb, w0, h0);
            rgb = r.rgb;
            alpha = r.alpha;
        }
        let (ow, oh) = match p["resize"].as_array() {
            Some(rs) => (rs[0].as_u64().unwrap() as usize, rs[1].as_u64().unwrap() as usize),
            None => (w0, h0),
        };
        // full reference
        let (fc, fa) = if (ow, oh) != (w0, h0) {
            resize::premul_resize_with(&rgb, &alpha, w0, h0, ow, oh, settings(p).resample)
        } else {
            (rgb.clone(), alpha.clone())
        };
        if (ow, oh) != (w0, h0) {
            let src = Prepared { w: w0, h: h0, pre_w: w0, src_box: [0, 0, w0, h0], rgb: rgb.clone(), alpha: alpha.clone(), bg: None, bg_used: None, solid: None };
            let rz = Resizer::from_prepared(std::sync::Arc::new(src), ow, oh, settings(p).resample);
            for _ in 0..4 {
                let x0 = rng.next(ow - 1);
                let y0 = rng.next(oh - 1);
                let x1 = (x0 + 1 + rng.next(ow - x0)).min(ow);
                let y1 = (y0 + 1 + rng.next(oh - y0)).min(oh);
                let (c, a) = rz.window(x0, y0, x1, y1);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let q = y * ow + x;
                        let r = (y - y0) * (x1 - x0) + (x - x0);
                        assert_eq!(fa[q].to_bits(), a[r].to_bits(), "{name}: resize alpha at {x},{y}");
                        for ch in 0..3 {
                            assert_eq!(fc[q * 3 + ch].to_bits(), c[r * 3 + ch].to_bits(), "{name}: resize rgb");
                        }
                    }
                }
            }
        }
        // every case as it is, then with a stack of texture layers (distress::*)
        let plain = settings(p);
        let mut textured = plain.clone();
        textured.distress = texture_stack(ci as u32);
        // true-colour dots (Settings::true_dots) on a few cases: one screen, own colours
        let mut variants = vec![(plain.clone(), ""), (textured, " +tekstur")];
        if ci < 4 && plain.halftone_area != Some(0.0) {
            variants.push((Settings { true_dots: Some(TrueDots::default()), halftone_area: None, ..plain }, " +titik-warna"));
        }
        for (s, tag) in variants {
            let name = format!("{name}{tag}");
            let full = render(&fc, &fa, ow, oh, &s);
            let plan = Plan::new(
                Prepared { w: w0, h: h0, pre_w: w0, src_box: [0, 0, w0, h0], rgb: rgb.clone(), alpha: alpha.clone(), bg: None, bg_used: None, solid: None },
                ow,
                oh,
                s.clone(),
            );
            // bands of ~40 rows
            let (packed, checks) = render_packed(&plan, ow * 40, |_, _, _| true).unwrap();
            let rows = packed.rows(0, oh);
            let nb = (0..ow * oh).filter(|&q| rows[q * 4..q * 4 + 4] != full[q * 4..q * 4 + 4]).count();
            let fch = io::checks_for(&full, ow, oh, s.dpi, s.pure_colours());
            if nb > 0 {
                let q = (0..ow * oh).find(|&q| rows[q * 4..q * 4 + 4] != full[q * 4..q * 4 + 4]).unwrap();
                bad.push(format!("{name}: bands {nb} px differ, first {},{}", q % ow, q / ow));
            }
            if checks != fch {
                bad.push(format!("{name}: checks {checks:?} vs {fch:?}"));
            }
            // random windows
            let mut nw = 0;
            for _ in 0..6 {
                let x0 = rng.next(ow - 1);
                let y0 = rng.next(oh - 1);
                let x1 = (x0 + 1 + rng.next(300)).min(ow);
                let y1 = (y0 + 1 + rng.next(300)).min(oh);
                let o = plan.render(x0, y0, x1, y1);
                for y in y0..y1 {
                    let a = &o[(y - y0) * (x1 - x0) * 4..(y - y0 + 1) * (x1 - x0) * 4];
                    let b = &full[(y * ow + x0) * 4..(y * ow + x1) * 4];
                    if a != b {
                        nw += 1;
                    }
                }
            }
            if nw > 0 {
                bad.push(format!("{name}: {nw} window rows differ"));
            }
            println!("{name:36} bands {}  windows {}", if nb == 0 { "IDENTIK" } else { "BEDA" }, if nw == 0 { "IDENTIK" } else { "BEDA" });
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// the app keeps sources as 8-bit RGBA; converting back must equal io::load exactly
#[test]
fn rgba8_source_equals_float_load() {
    for f in std::fs::read_dir(golden_dir()).unwrap() {
        let p = f.unwrap().path();
        if p.extension().is_some_and(|e| e == "png") {
            let a = io::load(&p).unwrap();
            let b = io::load_rgba8(&p).unwrap().to_loaded();
            assert_eq!((a.w, a.h, a.has_alpha), (b.w, b.h, b.has_alpha), "{p:?}");
            assert!(a.rgb.iter().zip(&b.rgb).all(|(x, y)| x.to_bits() == y.to_bits()), "{p:?} rgb");
            assert!(a.alpha.iter().zip(&b.alpha).all(|(x, y)| x.to_bits() == y.to_bits()), "{p:?} alpha");
        }
    }
}

/// the running overview averages real colour values (true-colour output of
/// "halftone only on fades" once showed up white when zoomed out)
#[test]
fn overview_keeps_true_colours() {
    let (w, h) = (8usize, 4usize);
    let mut rgba = vec![0u8; w * h * 4];
    for px in rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&[200, 100, 50, 255]);
    }
    let mut ov = studiozea_engine::band::Overview::new(w, h, 2);
    ov.add_rows(0, &rgba);
    for px in ov.rgba().chunks_exact(4) {
        assert_eq!(px, [200, 100, 50, 255]);
    }
    // pure colours: same result as before (0/255 averages)
    let mut half = vec![0u8; w * h * 4];
    for (i, px) in half.chunks_exact_mut(4).enumerate() {
        px.copy_from_slice(if i % 2 == 0 { &[255, 0, 255, 255] } else { &[0, 0, 0, 0] });
    }
    let mut ov = studiozea_engine::band::Overview::new(w, h, 2);
    ov.add_rows(0, &half);
    for px in ov.rgba().chunks_exact(4) {
        assert_eq!(px, [255, 0, 255, 128]);
    }
}

#[test]
fn flat_background_colour() {
    use studiozea_engine::{bg::flat_background, io::Rgba8};
    let mk = |w: usize, h: usize, f: &dyn Fn(usize, usize) -> [u8; 4], alpha: bool| Rgba8 {
        w,
        h,
        px: (0..w * h).flat_map(|q| f(q % w, q / w)).collect(),
        has_alpha: alpha,
    };
    // navy border, design inside
    let navy = mk(60, 40, &|x, y| if (10..50).contains(&x) && (10..30).contains(&y) { [250, 200, 0, 255] } else { [28, 37, 65, 255] }, false);
    let c = flat_background(&navy).unwrap();
    assert_eq!(c.map(|v| (v * 255.0).round() as u8), [28, 37, 65]);
    // transparent image: no suggestion
    let tr = mk(60, 40, &|x, _| if x < 30 { [0, 0, 0, 0] } else { [9, 9, 9, 255] }, true);
    assert!(flat_background(&tr).is_none());
    // busy border (gradient): no suggestion
    let busy = mk(60, 40, &|x, y| [(x * 4) as u8, (y * 6) as u8, 100, 255], false);
    assert!(flat_background(&busy).is_none());
}
