//! Vintage texture layers (`Settings.distress`, Rust-native, no Python golden):
//! the built-in library (repository folder textures/), imported files (1-bit TIFF,
//! RGBA PNG), every mode x shirt x dpi prints clean and deterministic, band /
//! window identity, cover placement, old jobs and removed procedural looks.
//! Set DISTRESS_PRINT=1 to print the regression hashes instead of checking them.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use studiozea_engine::{
    band::{render_packed, Plan},
    distress::{self, Layer, Mode},
    io, library,
    noise::Grain,
    render, resize, run, Job, Pattern, Prepared, Settings, Shirt,
};

const NAVY: [f32; 3] = [17.0 / 255.0, 25.0 / 255.0, 61.0 / 255.0];
const MODES: [Mode; 3] = [Mode::Lubang, Mode::Campur, Mode::Pudar];

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

fn sha(o: &[u8]) -> String {
    Sha256::digest(o)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn printing() -> bool {
    std::env::var("DISTRESS_PRINT").is_ok()
}

/// fresh temp folder for one test (tests run in parallel)
fn tmp(test: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("studiozea-distress-{}-{test}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// minimal uncompressed 1-bit TIFF (like a "bitmap" texture from an image editor)
fn write_tiff_1bit(
    path: &Path,
    w: u32,
    h: u32,
    white_is_zero: bool,
    black: impl Fn(u32, u32) -> bool,
) {
    let rb = w.div_ceil(8) as usize;
    let mut data = vec![0u8; rb * h as usize];
    for y in 0..h {
        for x in 0..w {
            let bit = black(x, y) == white_is_zero;
            if bit {
                data[y as usize * rb + (x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
    }
    let mut f = Vec::new();
    f.extend_from_slice(b"II");
    f.extend_from_slice(&42u16.to_le_bytes());
    let ifd = 8 + data.len() as u32;
    let ifd = ifd + (ifd & 1);
    f.extend_from_slice(&ifd.to_le_bytes());
    f.extend_from_slice(&data);
    while f.len() < ifd as usize {
        f.push(0);
    }
    let photometric = if white_is_zero { 0 } else { 1 };
    let entries: [(u16, u16, u32); 9] = [
        (256, 4, w),
        (257, 4, h),
        (258, 3, 1),
        (259, 3, 1),
        (262, 3, photometric),
        (273, 4, 8),
        (277, 3, 1),
        (278, 4, h),
        (279, 4, data.len() as u32),
    ];
    f.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    for (tag, typ, val) in entries {
        f.extend_from_slice(&tag.to_le_bytes());
        f.extend_from_slice(&typ.to_le_bytes());
        f.extend_from_slice(&1u32.to_le_bytes());
        if typ == 3 {
            f.extend_from_slice(&(val as u16).to_le_bytes());
            f.extend_from_slice(&0u16.to_le_bytes());
        } else {
            f.extend_from_slice(&val.to_le_bytes());
        }
    }
    f.extend_from_slice(&0u32.to_le_bytes());
    std::fs::write(path, f).unwrap();
}

fn write_png_rgba(path: &Path, w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; 4]) {
    let mut buf = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            buf.extend_from_slice(&px(x, y));
        }
    }
    image::save_buffer_with_format(
        path,
        &buf,
        w,
        h,
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .unwrap();
}

/// deterministic blob texture (black discs), 1-bit TIFF
fn blob_texture(dir: &Path) -> PathBuf {
    let p = dir.join("blobs.tif");
    let mut s = 12345u64;
    let mut next = || {
        s = s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (s >> 33) as f64 / (1u64 << 31) as f64
    };
    let discs: Vec<(f64, f64, f64)> = (0..60)
        .map(|_| (next() * 400.0, next() * 300.0, 4.0 + next() * 14.0))
        .collect();
    write_tiff_1bit(&p, 400, 300, true, |x, y| {
        discs.iter().any(|&(cx, cy, r)| {
            let (dx, dy) = (x as f64 - cx, y as f64 - cy);
            dx * dx + dy * dy < r * r
        })
    });
    p
}

fn tex_layer(path: &str, mode: Mode) -> Layer {
    let mut l = Layer::new(path);
    l.mode = Some(mode);
    l
}

fn settings(shirt: Shirt, dpi: f64, pattern: Pattern, layers: Vec<Layer>) -> Settings {
    Settings {
        shirt,
        dpi,
        pattern,
        distress: layers,
        ..Settings::default()
    }
}

struct Img {
    w: usize,
    h: usize,
    rgb: Vec<f32>,
    alpha: Vec<f32>,
}

fn load(name: &str) -> Img {
    let l = io::load(&golden(name)).unwrap();
    Img {
        w: l.w,
        h: l.h,
        rgb: l.rgb,
        alpha: l.alpha,
    }
}

/// the texture sources tested: every built-in one + two generated files
fn sources(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = library::names()
        .into_iter()
        .map(|n| format!("{}{n}", distress::LIB_PREFIX))
        .collect();
    v.push(blob_texture(dir).to_string_lossy().into_owned());
    let png = dir.join("rgba.png");
    write_png_rgba(&png, 300, 200, |x, y| {
        let a = if ((x / 15) + (y / 15)) % 3 == 0 { 255 } else { 0 };
        [0, 0, 0, a]
    });
    v.push(png.to_string_lossy().into_owned());
    v
}

#[test]
fn library_lists_the_repository_textures() {
    // every file the user put in textures/ must load (a broken upload stops the build
    // here, with its name, instead of silently missing from the app)
    let names = library::names();
    for n in names {
        let p = format!("{}{n}", distress::LIB_PREFIX);
        let (w, h) = distress::texture_size(&p).unwrap();
        assert!(w > 0 && h > 0, "{n}");
        assert!(distress::texture(&p, Some(500.0), 300.0).is_some(), "{n}");
        assert!(!library::title(n).is_empty());
    }
    assert!(distress::texture_size("lib:does-not-exist.png").is_err());
}

#[test]
fn every_texture_and_mode_prints_clean_and_is_deterministic() {
    let dir = tmp("a");
    let mut bad = Vec::new();
    let black = load("in_art_black.png");
    let white = load("in_art_white.png");
    for src in sources(&dir) {
        for mode in MODES {
            for (shirt, img) in [
                (Shirt::Hitam, &black),
                (Shirt::Putih, &white),
                (Shirt::Warna(NAVY), &black),
            ] {
                for dpi in [300.0, 600.0] {
                    let (ow, oh) = ((img.w as f64 * dpi / 300.0) as usize, (img.h as f64 * dpi / 300.0) as usize);
                    let (c, a) = resize::premul_resize(&img.rgb, &img.alpha, img.w, img.h, ow, oh);
                    let s = settings(shirt, dpi, Pattern::Am, vec![tex_layer(&src, mode)]);
                    let o1 = render(&c, &a, ow, oh, &s);
                    let o2 = render(&c, &a, ow, oh, &s);
                    let ch = io::checks(&o1, ow, oh, dpi);
                    let base = render(&c, &a, ow, oh, &settings(shirt, dpi, Pattern::Am, vec![]));
                    let name = format!("{src} {} {:?} {dpi}", mode.name(), shirt);
                    if !ch.ok() {
                        bad.push(format!("{name}: checks {ch:?}"));
                    }
                    if o1 != o2 {
                        bad.push(format!("{name}: not deterministic"));
                    }
                    // a light texture may leave the small test art untouched; only the
                    // generated test textures must always change something
                    if o1 == base && !src.starts_with(distress::LIB_PREFIX) {
                        bad.push(format!("{name}: texture changed nothing"));
                    }
                    // holes only ever remove ink; a fade never adds any
                    let more = (0..ow * oh).filter(|&p| o1[p * 4 + 3] == 255 && base[p * 4 + 3] != 255).count();
                    if mode == Mode::Lubang && more > 0 {
                        bad.push(format!("{name}: {more} px gained ink"));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

/// with a texture that covers the print, the mirrored repeat is never used
#[test]
fn cover_placement_covers_the_print() {
    let img = load("in_art_black.png");
    for name in library::names() {
        let p = format!("{}{name}", distress::LIB_PREFIX);
        for angle in [0.0, 90.0, 30.0, -125.0] {
            let mut a = tex_layer(&p, Mode::Lubang);
            a.angle = Some(angle);
            let mut b = a.clone();
            b.tile = Some(false);
            let sa = settings(Shirt::Hitam, 300.0, Pattern::Am, vec![a]);
            let sb = settings(Shirt::Hitam, 300.0, Pattern::Am, vec![b]);
            assert_eq!(
                render(&img.rgb, &img.alpha, img.w, img.h, &sa),
                render(&img.rgb, &img.alpha, img.w, img.h, &sb),
                "{name} angle {angle}"
            );
        }
    }
}

/// deterministic pseudo random numbers (no extra dependency)
struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n.max(1)
    }
}

/// band render (~40-row bands) and random windows must equal the full render
fn band_identity(
    name: &str,
    img: &Img,
    ow: usize,
    oh: usize,
    s: &Settings,
    rng: &mut Lcg,
) -> Vec<String> {
    let mut bad = Vec::new();
    let (fc, fa) = if (ow, oh) != (img.w, img.h) {
        resize::premul_resize(&img.rgb, &img.alpha, img.w, img.h, ow, oh)
    } else {
        (img.rgb.clone(), img.alpha.clone())
    };
    let full = render(&fc, &fa, ow, oh, s);
    let fch = io::checks(&full, ow, oh, s.dpi);
    if !fch.ok() {
        bad.push(format!("{name}: full render checks {fch:?}"));
    }
    let plan = Plan::new(
        Prepared {
            w: img.w,
            h: img.h,
            pre_w: img.w,
            src_box: [0, 0, img.w, img.h],
            rgb: img.rgb.clone(),
            alpha: img.alpha.clone(),
            bg: None,
            bg_used: None,
            solid: None,
        },
        ow,
        oh,
        s.clone(),
    );
    let (packed, checks) = render_packed(&plan, ow * 40, |_, _, _| true).unwrap();
    let rows = packed.rows(0, oh);
    let nb = (0..ow * oh)
        .filter(|&q| rows[q * 4..q * 4 + 4] != full[q * 4..q * 4 + 4])
        .count();
    if nb > 0 {
        let q = (0..ow * oh)
            .find(|&q| rows[q * 4..q * 4 + 4] != full[q * 4..q * 4 + 4])
            .unwrap();
        bad.push(format!(
            "{name}: bands {nb} px differ, first {},{}",
            q % ow,
            q / ow
        ));
    }
    if checks != fch {
        bad.push(format!("{name}: band checks {checks:?} vs {fch:?}"));
    }
    let mut nw = 0;
    for _ in 0..6 {
        let x0 = rng.next(ow - 1);
        let y0 = rng.next(oh - 1);
        let x1 = (x0 + 1 + rng.next(300)).min(ow);
        let y1 = (y0 + 1 + rng.next(300)).min(oh);
        let o = plan.render(x0, y0, x1, y1);
        for y in y0..y1 {
            if o[(y - y0) * (x1 - x0) * 4..(y - y0 + 1) * (x1 - x0) * 4]
                != full[(y * ow + x0) * 4..(y * ow + x1) * 4]
            {
                nw += 1;
            }
        }
    }
    if nw > 0 {
        bad.push(format!("{name}: {nw} window rows differ"));
    }
    println!(
        "{name:40} halo {:3}  bands {}  windows {}",
        plan.halo,
        if nb == 0 { "IDENTIK" } else { "BEDA" },
        if nw == 0 { "IDENTIK" } else { "BEDA" }
    );
    bad
}

#[test]
fn bands_and_windows_match_full_render_with_textures() {
    let dir = tmp("b");
    let srcs = sources(&dir);
    let mut rng = Lcg(11);
    let mut bad = Vec::new();
    let black = load("in_art_black.png");
    let white = load("in_art_white.png");
    for (i, src) in srcs.iter().enumerate() {
        for mode in MODES {
            let mut l = tex_layer(src, mode);
            // exercise rotation, mirror, zoom < 1 (mirrored repeat) and an offset
            l.angle = Some([0.0, 30.0, 90.0, -15.0][i % 4]);
            l.mirror = Some(i % 2 == 1);
            l.zoom = Some([1.0, 0.6, 1.4, 0.8][i % 4]);
            l.offset_mm = Some([3.0, -2.0]);
            l.threshold = Some(0.6);
            for (shirt, img, dpi, pat) in [
                (Shirt::Hitam, &black, 300.0, Pattern::Am),
                (Shirt::Putih, &white, 600.0, Pattern::Am),
                (Shirt::Warna(NAVY), &black, 300.0, Pattern::Fm),
                (Shirt::Hitam, &white, 300.0, Pattern::Grain(Grain::Sedang)),
            ] {
                let s = settings(shirt, dpi, pat, vec![l.clone()]);
                let (ow, oh) = ((img.w as f64 * dpi / 300.0) as usize, (img.h as f64 * dpi / 300.0) as usize);
                let name = format!("{} {} {:?} {dpi}", Path::new(src).file_name().unwrap().to_string_lossy(), mode.name(), shirt);
                bad.extend(band_identity(&name, img, ow, oh, &s, &mut rng));
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn stacked_layers_print_clean_and_match_bands() {
    // 2 and 3 textures on one design (holes are joined, fades multiply)
    let dir = tmp("stack");
    let srcs = sources(&dir);
    let mut rng = Lcg(23);
    let mut bad = Vec::new();
    let black = load("in_art_black.png");
    let white = load("in_art_white.png");
    let mk = |k: usize, mode: Mode, angle: f64| {
        let mut l = tex_layer(&srcs[k % srcs.len()], mode);
        l.angle = Some(angle);
        l.zoom = Some(1.0 + 0.3 * k as f64);
        l.threshold = Some(0.55);
        l.amount = Some(0.4);
        l
    };
    let stacks = [
        vec![mk(0, Mode::Lubang, 0.0), mk(1, Mode::Lubang, 90.0)],
        vec![mk(0, Mode::Campur, 0.0), mk(1, Mode::Pudar, 45.0), mk(2, Mode::Lubang, 180.0)],
        vec![mk(1, Mode::Pudar, 0.0), mk(2, Mode::Pudar, 270.0)],
    ];
    for (i, layers) in stacks.iter().enumerate() {
        for (shirt, img) in [(Shirt::Hitam, &black), (Shirt::Putih, &white)] {
            let s = settings(shirt, 300.0, Pattern::Am, layers.clone());
            let o = render(&img.rgb, &img.alpha, img.w, img.h, &s);
            let ch = io::checks(&o, img.w, img.h, 300.0);
            if !ch.ok() {
                bad.push(format!("stack {i} {shirt:?}: {ch:?}"));
            }
            // more layers never put ink back: every ink pixel of the stack is ink with one layer
            let one = render(&img.rgb, &img.alpha, img.w, img.h, &settings(shirt, 300.0, Pattern::Am, vec![layers[0].clone()]));
            let ink = |v: &[u8]| v.chunks_exact(4).filter(|p| p[3] > 0).count();
            if ink(&o) > ink(&one) {
                bad.push(format!("stack {i} {shirt:?}: more ink with more layers"));
            }
            bad.extend(band_identity(&format!("stack {i} {shirt:?}"), img, img.w, img.h, &s, &mut rng));
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

#[test]
fn regression_hashes() {
    // pinned with the sample textures vintage-01.jpg / vintage-02.jpg (looked up by name,
    // so adding or removing other textures in the folder does not change these)
    const PINNED: [&str; 2] = ["vintage-01.jpg", "vintage-02.jpg"];
    let names = library::names();
    if !PINNED.iter().all(|p| names.contains(p)) {
        return;
    }
    let img = load("in_art_black.png");
    let mut got = Vec::new();
    for (k, mode) in MODES.iter().enumerate() {
        let mut l = tex_layer(&format!("{}{}", distress::LIB_PREFIX, PINNED[k % PINNED.len()]), *mode);
        l.angle = Some(30.0 * k as f64);
        l.mirror = Some(k == 1);
        let s = settings(Shirt::Hitam, 300.0, Pattern::Am, vec![l]);
        got.push(sha(&render(&img.rgb, &img.alpha, img.w, img.h, &s))[..16].to_string());
    }
    if printing() {
        println!("REGRESSION = {got:?}");
        return;
    }
    assert_eq!(got, REGRESSION, "texture regression hashes");
}

const REGRESSION: [&str; 3] = ["1b1375ee14d0193c", "2aa4c34facc57b15", "926ca45fc25fe99d"];

#[test]
fn removed_procedural_layers_and_bad_files_are_ignored() {
    let img = load("in_art_black.png");
    let base = render(&img.rgb, &img.alpha, img.w, img.h, &Settings::default());
    let old: Settings = serde_json::from_str(
        r#"{"distress":[{"kind":"retak","amount":0.3},{"kind":"pudar","mode":"pudar","amount":0.5},{"kind":"bintik"}]}"#,
    )
    .unwrap();
    assert_eq!(render(&img.rgb, &img.alpha, img.w, img.h, &old), base);
    assert!(distress::validate(&old).is_ok());
    let missing = settings(Shirt::Hitam, 300.0, Pattern::Am, vec![tex_layer("/no/such/file.png", Mode::Lubang)]);
    assert!(distress::validate(&missing).is_err());
    assert_eq!(render(&img.rgb, &img.alpha, img.w, img.h, &missing), base);
    let lib_missing = settings(Shirt::Hitam, 300.0, Pattern::Am, vec![tex_layer("lib:nope.jpg", Mode::Lubang)]);
    assert!(distress::validate(&lib_missing).is_err());
}

#[test]
fn layer_json_schema() {
    // exactly what the UI sends (app/src/presets.js)
    let job: Job = serde_json::from_str(
        r#"{"remove_bg":"otomatis","width_cm":30,"settings":{"shirt":{"type":"hitam"},"lpi":35,"dpi":300,
        "pattern":"am","distress":[
          {"kind":"gambar","path":"lib:vintage-01.jpg","mode":"lubang","threshold":0.5,"amount":0.5,"zoom":1,
           "angle":90,"mirror":true,"invert":false,"tile":true,"offset_mm":[0,0]},
          {"kind":"gambar","path":"C:\\tekstur\\a.tif","mode":"campur","threshold":0.4,"amount":0.6}]}}"#,
    )
    .unwrap();
    let d = &job.settings.distress;
    assert_eq!(d.len(), 2);
    let a = d[0].params();
    assert_eq!((a.mode, a.threshold, a.angle, a.mirror, a.zoom, a.scale_mm), (Mode::Lubang, 0.5, 90.0, true, 1.0, None));
    let b = d[1].params();
    assert_eq!((b.mode, b.threshold, b.amount), (Mode::Campur, 0.4, 0.6));
    // round trip
    let txt = serde_json::to_string(&job.settings).unwrap();
    let back: Settings = serde_json::from_str(&txt).unwrap();
    assert_eq!(back.distress, job.settings.distress);
    // no texture: field absent from the JSON, old JSON still parses
    let s0: Settings = serde_json::from_str(r#"{"dpi":300}"#).unwrap();
    assert!(s0.distress.is_empty());
    assert!(!serde_json::to_string(&s0).unwrap().contains("distress"));
    // junk values never fail or panic
    let s: Settings = serde_json::from_str(
        r#"{"distress":[{"kind":"gambar","path":"lib:vintage-01.jpg","threshold":1e300,"zoom":-5,"angle":1e300,"offset_mm":"3;4","tile":"no","mode":7}]}"#,
    )
    .unwrap();
    let p = s.distress[0].params();
    assert_eq!((p.threshold, p.zoom, p.angle, p.offset_mm, p.tile, p.mode), (1.0, 1.0, 0.0, [3.0, 4.0], false, Mode::Lubang));
    let img = load("in_art_black.png");
    let o = render(&img.rgb, &img.alpha, img.w, img.h, &s);
    assert!(io::checks(&o, img.w, img.h, s.dpi).ok());
}

const UI_JOBS_BEFORE: [&str; 45] = [
    "64f7c7f4a330f614",
    "e7c8e0bdded495b1",
    "5be076d45d87da04",
    "c5041a0fc5faf54f",
    "5969248ea000056d",
    "bea26083d76e9bce",
    "b2fda569f6b1b668",
    "eddf34a7701f2c48",
    "ba840da9a30766e2",
    "113f370ce342ef6e",
    "3e695e83d5fb19c7",
    "d1102a53b6fae6bd",
    "64f7c7f4a330f614",
    "e7c8e0bdded495b1",
    "5be076d45d87da04",
    "ca0ec105eb41122e",
    "4f0ca3edd47c5cda",
    "d415723d52ac3329",
    "5ee52a2acb1b43dd",
    "b30bdf81250acc61",
    "b7ead8ece266cee4",
    "6a9d20128eeda528",
    "db987c6edf81be67",
    "d933a9b1db1b3d28",
    "a800ef249d2f3a6c",
    "e103e253a634da15",
    "07fe1ea9da6aa9f0",
    "a8a533e9de42eb76",
    "96f70fbac6ae4b15",
    "e9c6074286c13741",
    "9b4bb3982b8763c8",
    "c3eb93a2e5291d1c",
    "fd28d7881dfd2d9f",
    "2e7bdce1346d56a1",
    "5dc1baca90427e6e",
    "28a46126afc0b4b1",
    "9b51498684877212",
    "84db09800b0c690b",
    "c5eb8fbd23216ef1",
    "7f28fcd347e90fc7",
    "a24dec86c94a2420",
    "b76578d12dda824b",
    "7a965190b82c8ef4",
    "759597d59bd41962",
    "1d41bc9958e9978b",
];

#[test]
fn old_ui_jobs_render_unchanged() {
    let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/ui_jobs.json"))
        .unwrap();
    let jobs: Vec<Job> = serde_json::from_str(&raw).unwrap();
    let src = io::load(&golden("in_art_rgba.png")).unwrap();
    // the jobs recorded before textures existed; jobs added later (e.g. the band-tee
    // preset) have no "before" render, they are checked by ui_jobs.rs
    assert!(jobs.len() >= UI_JOBS_BEFORE.len());
    for (i, mut job) in jobs.into_iter().take(UI_JOBS_BEFORE.len()).enumerate() {
        assert!(job.settings.distress.is_empty());
        job.width_cm = Some(5.0);
        let out = run(&src, &job);
        assert_eq!(&sha(&out.rgba)[..16], UI_JOBS_BEFORE[i], "ui job {i}");
    }
}

