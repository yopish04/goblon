//! Golden test: the Rust engine must produce exactly the pixels of the Python
//! reference (prototype/studiozea_ref.py) for every case in tests/golden/cases.json.
//! Regenerate with `python prototype/make_golden.py`. To see pixel diffs set
//! GOLDEN_EXPECTED to the folder written by `make_golden.py --expected DIR`.

use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use studiozea_engine::{
    bg, io, noise::Grain, render, resize, GridChoice, KMode, Pattern, Settings, Shape, Shirt,
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

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

#[test]
fn golden_cases_are_pixel_identical() {
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(golden_dir().join("cases.json")).unwrap())
            .unwrap();
    let only = std::env::var("GOLDEN_ONLY").ok();
    let mut failed = Vec::new();
    for case in &cases {
        let name = case["name"].as_str().unwrap();
        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }
        let p = &case["params"];
        let src = io::load(&golden_dir().join(case["input"].as_str().unwrap())).unwrap();
        let (mut w, mut h, mut rgb, mut alpha) = (src.w, src.h, src.rgb.clone(), src.alpha.clone());
        if p["remove_bg"].as_bool().unwrap_or(false) {
            let r = bg::remove_bg(&rgb, w, h);
            let want: Vec<f32> = case["bg"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect();
            assert_eq!(r.bg.to_vec(), want, "{name}: background colour");
            rgb = r.rgb;
            alpha = r.alpha;
        }
        if p["erase"].is_array() {
            let strokes: Vec<studiozea_engine::erase::Stroke> = serde_json::from_value(p["erase"].clone()).unwrap();
            studiozea_engine::erase::apply(&mut rgb, &mut alpha, &src.rgb, &src.alpha, w, h, &strokes);
        }
        let (rot, mir) = (p["rotate"].as_u64().unwrap_or(0) as u16, p["mirror"].as_bool().unwrap_or(false));
        if rot != 0 || mir {
            let (c, a, ow, oh) = resize::orient(&rgb, &alpha, w, h, rot, mir);
            (rgb, alpha, w, h) = (c, a, ow, oh);
        }
        if let Some(rs) = p["resize"].as_array() {
            let (ow, oh) = (
                rs[0].as_u64().unwrap() as usize,
                rs[1].as_u64().unwrap() as usize,
            );
            let (c, a) = resize::premul_resize_with(&rgb, &alpha, w, h, ow, oh, settings(p).resample);
            rgb = c;
            alpha = a;
            w = ow;
            h = oh;
        }
        let s = settings(p);
        let t0 = std::time::Instant::now();
        let o = render(&rgb, &alpha, w, h, &s);
        let dt = t0.elapsed().as_secs_f64();
        let hash = format!("{:x}", Sha256::digest(&o));
        let ch = io::checks_for(&o, w, h, s.dpi, s.pure_colours());
        assert!(ch.ok(), "{name}: checks {ch:?}");
        let box_ok = match &case["autotrim"] {
            Value::Array(b) => {
                let want: Vec<usize> = b.iter().map(|v| v.as_u64().unwrap() as usize).collect();
                resize::autotrim(&rgb, &alpha, w, h, s.shirt).map(|(a, b, c, d)| vec![a, b, c, d])
                    == Some(want)
            }
            _ => true,
        };
        let same = hash == case["sha256"].as_str().unwrap();
        let mut detail = String::new();
        if !same {
            if let Ok(dir) = std::env::var("GOLDEN_EXPECTED") {
                let e = io::load(&Path::new(&dir).join(format!("{name}.png"))).unwrap();
                let mut diff = 0;
                let mut first = None;
                for q in 0..w * h {
                    let er = [e.rgb[q * 3], e.rgb[q * 3 + 1], e.rgb[q * 3 + 2], e.alpha[q]]
                        .map(|v| (v * 255.0).round() as u8);
                    if er != [o[q * 4], o[q * 4 + 1], o[q * 4 + 2], o[q * 4 + 3]] {
                        diff += 1;
                        first.get_or_insert((q % w, q / w));
                    }
                }
                detail = format!(" ({diff} px differ, first at {first:?})");
            }
            failed.push(format!("{name}{detail}"));
        }
        println!(
            "{name:28} {}  autotrim {}  {dt:.2}s{detail}",
            if same { "IDENTIK" } else { "BEDA" },
            if box_ok { "ok" } else { "BEDA" }
        );
        assert!(box_ok, "{name}: autotrim");
    }
    assert!(failed.is_empty(), "not pixel-identical: {failed:?}");
}
