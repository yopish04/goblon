//! "Lindungi teks": text found, letters solid in their own colours, background still
//! screened; band render = full render; off (or no text) = unchanged pixels.
use std::path::Path;

use studiozea_engine::band::{render_packed, Plan};
use studiozea_engine::erase::{BrushMode, Stroke};
use studiozea_engine::render::render_masked;
use studiozea_engine::{io, prepare_source_text, print_size, run, run_text, text, Job, Settings};

fn sample() -> io::Loaded {
    io::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/text_sample.jpg")).unwrap()
}

fn job(protect: bool) -> Job {
    Job {
        auto_trim: false,
        width_cm: Some(7.62), // 900 px at 300 dpi: one source pixel = one print pixel
        settings: Settings { protect_text: protect, ..Settings::default() },
        ..Job::default()
    }
}

/// share of the pixels of rectangle r (x0, y0, x1, y1) that are printed solid in a
/// colour other than the 8 pure ones (= true colour, only the protected text)
fn true_colour_share(o: &[u8], w: usize, r: [usize; 4]) -> f64 {
    let mut n = 0;
    let mut t = 0;
    for y in r[1]..r[3] {
        for x in r[0]..r[2] {
            let p = &o[(y * w + x) * 4..][..4];
            t += 1;
            if p[3] == 255 && p[..3].iter().any(|&v| v != 0 && v != 255) {
                n += 1;
            }
        }
    }
    n as f64 / t as f64
}

#[test]
fn finds_the_words_and_keeps_the_letters_solid() {
    let src = sample();
    let map = text::detect(&src.to_rgba8()).unwrap();
    println!("{} words", map.words);
    assert!(map.words >= 2, "STUDIOZEA and DTF PRINT 2026");
    // the letters are in the regions, the blue disc and the corners are not
    assert!(map.at(300, 130) > 0 && map.at(380, 270) > 0);
    assert_eq!(map.at(650, 470), 0);
    assert_eq!(map.at(20, 580), 0);
    let out = run_text(&src, &job(true), Some(&map));
    let plain = run(&src, &job(false));
    assert!(out.checks.semi == 0 && out.checks.tiny == 0, "{:?}", out.checks);
    let w = out.w;
    // the yellow STUDIOZEA (letters cover about half of its box): solid yellow
    let letters = [70, 95, 760, 165];
    let share = true_colour_share(&out.rgba, w, letters);
    println!("solid share in STUDIOZEA: {share:.2}");
    assert!(share > 0.35, "{share}");
    assert_eq!(true_colour_share(&plain.rgba, w, letters), 0.0);
    // the background between the lines and the disc stay screened (pure colours)
    assert!(true_colour_share(&out.rgba, w, [100, 195, 700, 215]) < 0.05);
    assert_eq!(true_colour_share(&out.rgba, w, [600, 430, 700, 510]), 0.0);
    // outside the text everything is as before
    let (a, b) = (&out.rgba[w * 4 * 360..], &plain.rgba[w * 4 * 360..]);
    let same = a.iter().zip(b).filter(|(x, y)| x == y).count() as f64 / a.len() as f64;
    assert!(same > 0.999, "{same}");
}

#[test]
fn off_or_without_text_changes_nothing() {
    let src = sample();
    let map = text::detect(&src.to_rgba8()).unwrap();
    let plain = run(&src, &job(false));
    assert_eq!(run_text(&src, &job(false), Some(&map)).rgba, plain.rgba, "option off");
    // on, but nothing detected and no strokes: same pixels as off (8 pure colours)
    let on = run_text(&src, &job(true), None);
    assert_eq!(on.rgba, plain.rgba, "no text");
}

#[test]
fn bands_equal_the_full_render_and_strokes_add_or_remove_text() {
    let src = sample();
    let map = text::detect(&src.to_rgba8()).unwrap();
    let mut j = job(true);
    j.width_cm = Some(11.0); // resized: the mask is resized with the print
    j.erase = vec![
        // the disc becomes "text", the first letter not
        Stroke { mode: BrushMode::Teks, r: 60.0, pts: vec![[650.0, 470.0]] },
        Stroke { mode: BrushMode::BukanTeks, r: 45.0, pts: vec![[110.0, 130.0]] },
    ];
    let full = run_text(&src, &j, Some(&map));
    let prep = prepare_source_text(&src, &j, Some(&map));
    let (pw, ph) = print_size(&prep, &j);
    let plan = Plan::new(prep, pw, ph, j.settings.clone());
    let (packed, checks) = render_packed(&plan, pw * 37, |_, _, _| true).unwrap();
    assert_eq!((packed.w, packed.h), (full.w, full.h));
    assert_eq!(packed.rows(0, full.h), full.rgba, "bands = full");
    assert_eq!(checks.semi, 0);
    let s = full.w as f64 / 900.0;
    let at = |x: f64, y: f64| [(x * s) as usize, (y * s) as usize, (x * s) as usize + 10, (y * s) as usize + 10];
    assert!(true_colour_share(&full.rgba, full.w, at(645.0, 465.0)) > 0.9, "disc marked as text");
    assert_eq!(true_colour_share(&full.rgba, full.w, at(100.0, 120.0)), 0.0, "S unmarked");
    // the stroke only marks: the image itself is unchanged where nothing is printed solid
}

/// ink / no-ink changes along the rows of rectangle r (more = finer dots)
fn transitions(o: &[u8], w: usize, r: [usize; 4]) -> usize {
    let mut n = 0;
    for y in r[1]..r[3] {
        for x in r[0] + 1..r[2] {
            if (o[(y * w + x) * 4 + 3] > 0) != (o[(y * w + x - 1) * 4 + 3] > 0) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn fine_dots_keep_the_screen_finer_in_the_lettering() {
    use studiozea_engine::TextStyle;
    let src = sample();
    let map = text::detect(&src.to_rgba8()).unwrap();
    let mut j = job(true);
    j.settings.text_style = TextStyle::Halus;
    assert_eq!(j.settings.text_fine_lpi(), Some(70.0));
    // low DPI: never finer than dpi / 4 (and never coarser than the screen)
    let low = Settings { dpi: 150.0, ..j.settings.clone() };
    assert_eq!(low.text_fine_lpi(), Some(37.5));
    let big = Settings { lpi: 90.0, ..j.settings.clone() };
    assert_eq!(big.text_fine_lpi(), Some(90.0));
    assert!(j.settings.pure_colours());
    let out = run_text(&src, &j, Some(&map));
    let plain = run(&src, &job(false));
    let w = out.w;
    assert!(out.checks.semi == 0 && out.checks.impure == 0 && out.checks.tiny == 0, "{:?}", out.checks);
    // still halftone (8 pure colours), and the words are screened anew
    let letters = [70, 95, 760, 165];
    assert_eq!(true_colour_share(&out.rgba, w, letters), 0.0);
    assert_ne!(transitions(&out.rgba, w, letters), transitions(&plain.rgba, w, letters));
    // a 50 % grey marked as text: about twice as many dots across (70 vs 35 LPI)
    let (gw, gh) = (240, 120);
    let grey = vec![0.5f32; gw * gh * 3];
    let alpha = vec![1.0f32; gw * gh];
    let all = vec![1.0f32; gw * gh];
    let fine = render_masked(&grey, &alpha, Some(&all), gw, gh, &j.settings);
    let coarse = render_masked(&grey, &alpha, Some(&all), gw, gh, &job(false).settings);
    let (tf, tc) = (transitions(&fine, gw, [0, 0, gw, gh]), transitions(&coarse, gw, [0, 0, gw, gh]));
    println!("transitions in grey: fine {tf}, plain {tc}");
    assert!(tf as f64 > tc as f64 * 1.6, "{tf} vs {tc}");
    // outside the text everything is as before
    let (a, b) = (&out.rgba[w * 4 * 360..], &plain.rgba[w * 4 * 360..]);
    let same = a.iter().zip(b).filter(|(x, y)| x == y).count() as f64 / a.len() as f64;
    assert!(same > 0.999, "{same}");
    // bands = full render (resized print)
    j.width_cm = Some(11.0);
    let full = run_text(&src, &j, Some(&map));
    let prep = prepare_source_text(&src, &j, Some(&map));
    let (pw, ph) = print_size(&prep, &j);
    let plan = Plan::new(prep, pw, ph, j.settings.clone());
    let (packed, _) = render_packed(&plan, pw * 37, |_, _, _| true).unwrap();
    assert_eq!(packed.rows(0, full.h), full.rgba, "bands = full");
    // patterns without an LPI (FM, dither, grain) keep the lettering solid
    j.settings.pattern = studiozea_engine::Pattern::Fm;
    assert_eq!(j.settings.text_fine_lpi(), None);
    assert!(!j.settings.pure_colours());
}
