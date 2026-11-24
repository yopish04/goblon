//! AI upscale (Real-ESRGAN Compact): same pixels as the PyTorch reference
//! (tools/make_ai_ref.py), tiles invisible, 2x, alpha, cancel and limits.
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use studiozea_engine::io::{load_rgba8, Rgba8};
use studiozea_engine::upscale::{upscale, upscale_on, upscale_tiled, Gpu, Scale, MAX_OUT_PX};

fn data(name: &str) -> Rgba8 {
    load_rgba8(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data").join(name)).unwrap()
}

/// largest and mean difference per channel value
fn diff(a: &Rgba8, b: &Rgba8) -> (u8, f64) {
    assert_eq!((a.w, a.h), (b.w, b.h));
    let d: Vec<u8> = a.px.iter().zip(&b.px).map(|(x, y)| x.abs_diff(*y)).collect();
    (*d.iter().max().unwrap(), d.iter().map(|&v| v as f64).sum::<f64>() / d.len() as f64)
}

#[test]
fn matches_the_pytorch_reference() {
    for (input, expected, dn) in [("ai_in_rgb.png", "ai_x4_rgb_dn0.5.png", 0.5), ("ai_in_rgba.png", "ai_x4_rgba_dn1.0.png", 1.0)] {
        let src = data(input);
        let out = upscale(&src, Scale::X4, dn, |_, _| true).unwrap();
        let want = data(expected);
        assert_eq!(out.has_alpha, src.has_alpha, "{input}");
        // float sums in another order: at most one level here and there
        let (max, mean) = diff(&out, &want);
        println!("{input}: max {max} mean {mean:.5}");
        assert!(max <= 1 && mean < 0.01, "{input}: max {max} mean {mean}");
    }
}

#[test]
fn tiles_do_not_change_the_result() {
    let src = data("ai_in_rgb.png");
    let whole = upscale_tiled(&src, Scale::X4, 0.5, 1000, |_, _| true).unwrap();
    for tile in [9, 16, 25] {
        let tiled = upscale_tiled(&src, Scale::X4, 0.5, tile, |_, _| true).unwrap();
        // the matrix products sum a few rows in another order: one level, rarely
        let (max, mean) = diff(&tiled, &whole);
        assert!(max <= 1 && mean < 1e-4, "tile {tile}: max {max} mean {mean}");
    }
}

#[test]
fn half_size_is_the_average_of_the_full_result() {
    let src = data("ai_in_rgba.png");
    let x4 = upscale(&src, Scale::X4, 0.7, |_, _| true).unwrap();
    let x2 = upscale(&src, Scale::X2, 0.7, |_, _| true).unwrap();
    assert_eq!((x2.w, x2.h), (src.w * 2, src.h * 2));
    for y in 0..x2.h {
        for x in 0..x2.w {
            for c in 0..4 {
                let s: u32 = (0..4).map(|i| x4.px[((2 * y + i / 2) * x4.w + 2 * x + i % 2) * 4 + c] as u32).sum();
                let v = x2.px[(y * x2.w + x) * 4 + c] as i32;
                assert!((v - (s as f64 / 4.0).round() as i32).abs() <= 1, "({x}, {y}) channel {c}");
            }
        }
    }
}

#[test]
fn opaque_images_stay_opaque_and_alpha_follows_the_design() {
    let src = data("ai_in_rgba.png");
    let out = upscale(&src, Scale::X4, 0.5, |_, _| true).unwrap();
    assert!(out.has_alpha);
    // the transparent corner stays (almost) transparent, the opaque right side opaque;
    // the print's alpha threshold removes the faint rest
    assert!(out.px[3] <= 8, "{}", out.px[3]);
    assert_eq!(out.px[((out.h - 1) * out.w + out.w - 1) * 4 + 3], 255);
    // an RGBA file without transparent pixels: no alpha pass, opaque result
    let mut opaque = data("ai_in_rgb.png");
    opaque.has_alpha = true;
    let o = upscale(&opaque, Scale::X2, 0.5, |_, _| true).unwrap();
    assert!(!o.has_alpha && o.px.chunks_exact(4).all(|p| p[3] == 255));
}

#[test]
fn reports_progress_and_cancels() {
    let src = data("ai_in_rgb.png");
    let seen = AtomicUsize::new(0);
    upscale_tiled(&src, Scale::X4, 0.5, 16, |done, total| {
        assert!(done <= total && total == 12);
        seen.fetch_max(done, Ordering::Relaxed);
        true
    })
    .unwrap();
    assert_eq!(seen.load(Ordering::Relaxed), 12);
    let calls = AtomicUsize::new(0);
    let r = upscale_tiled(&src, Scale::X4, 0.5, 16, |_, _| calls.fetch_add(1, Ordering::Relaxed) < 3);
    assert!(r.is_err());
    assert!(calls.load(Ordering::Relaxed) < 12, "stops early");
}

#[test]
fn refuses_results_that_are_too_large() {
    let side = ((MAX_OUT_PX as f64).sqrt() / 4.0) as usize + 2;
    let big = Rgba8 { w: side, h: side, px: vec![0; side * side * 4], has_alpha: false };
    let e = upscale(&big, Scale::X4, 0.5, |_, _| true).err().expect("too large");
    assert!(e.to_string().contains("terlalu besar"), "{e}");
    assert!(upscale(&big, Scale::X2, 0.5, |_, _| false).is_err(), "cancelled before the work");
    let empty = Rgba8 { w: 0, h: 0, px: vec![], has_alpha: false };
    assert!(upscale(&empty, Scale::X4, 0.5, |_, _| true).is_err());
}

/// the graphics card (software renderers count here: WARP on Windows CI, llvmpipe on
/// Linux) or None when this machine has none
fn card() -> Option<Gpu> {
    match Gpu::open(true) {
        Ok(g) => Some(g),
        Err(e) => {
            println!("no graphics card, GPU tests skipped: {e:#}");
            None
        }
    }
}

#[test]
fn graphics_card_gives_the_processor_result() {
    let Some(g) = card() else { return };
    println!("card: {}", g.name());
    if let Err(e) = g.self_test() {
        // the app would not use this card; strict on machines known to be good
        assert!(std::env::var("STZ_GPU_STRICT").is_err(), "self-test: {e:#}");
        println!("card failed its self-test, not used: {e:#}");
        return;
    }
    for (input, dn) in [("ai_in_rgb.png", 0.5), ("ai_in_rgba.png", 1.0)] {
        let src = data(input);
        let cpu = upscale(&src, Scale::X4, dn, |_, _| true).unwrap();
        g.testing(20, usize::MAX); // several tiles
        let (gpu, rep) = upscale_on(&src, Scale::X4, dn, Some(&g), |_, _| true).unwrap();
        println!("{rep:?}");
        // whatever the card does, the result is right (a failing card hands over)
        if std::env::var("STZ_GPU_STRICT").is_ok() {
            assert!(rep.cpu_tiles == 0 && rep.gpu_tiles > 1 && rep.fallback.is_none(), "{rep:?}");
        }
        let (max, mean) = diff(&gpu, &cpu);
        println!("{input}: card vs processor max {max} mean {mean:.5}");
        assert!(max <= 1 && mean < 0.01, "{input}: max {max} mean {mean}");
        assert_eq!(gpu.has_alpha, cpu.has_alpha);
    }
}

#[test]
fn a_failing_card_hands_over_to_the_processor() {
    let Some(g) = card() else { return };
    if g.self_test().is_err() {
        return;
    }
    let src = data("ai_in_rgba.png");
    let cpu = upscale(&src, Scale::X2, 0.5, |_, _| true).unwrap();
    // the card stops after 2 tiles: the rest (and the alpha pass) run on the processor
    g.testing(16, 2);
    let (out, rep) = upscale_on(&src, Scale::X2, 0.5, Some(&g), |_, _| true).unwrap();
    println!("{rep:?}");
    assert_eq!(rep.gpu_tiles, 2);
    assert!(rep.cpu_tiles > 0 && rep.fallback.as_deref().is_some_and(|f| f.contains("terputus")));
    let (max, _) = diff(&out, &cpu);
    assert!(max <= 1, "max {max}");
    // cancel while on the card
    g.testing(16, usize::MAX);
    let calls = AtomicUsize::new(0);
    assert!(upscale_on(&src, Scale::X4, 0.5, Some(&g), |_, _| calls.fetch_add(1, Ordering::Relaxed) < 2).is_err());
}

#[test]
fn the_card_shader_translates_for_every_graphics_api() {
    use naga::back::{hlsl, msl, spv};
    let module = naga::front::wgsl::parse_str(studiozea_engine::upscale::GPU_SHADER).expect("WGSL");
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .expect("valid");
    // DirectX 12 (shader model 5.1 for FXC, 6.0 for DXC)
    for sm in [hlsl::ShaderModel::V5_1, hlsl::ShaderModel::V6_0] {
        let mut out = String::new();
        let opts = hlsl::Options { shader_model: sm, ..Default::default() };
        hlsl::Writer::new(&mut out, &opts, &Default::default()).write(&module, &info, None).expect("HLSL");
        assert!(out.contains("GroupMemoryBarrierWithGroupSync"));
    }
    let mut out = String::new();
    msl::Writer::new(&mut out).write(&module, &info, &msl::Options::default(), &msl::PipelineOptions::default()).expect("MSL");
    let mut words = Vec::new();
    spv::Writer::new(&spv::Options::default()).unwrap().write(&module, &info, None, &None, &mut words).expect("SPIR-V");
    assert!(!words.is_empty());
}
