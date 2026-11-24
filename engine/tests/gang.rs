//! Gang sheet packing and composing.
use studiozea_engine::gang::{pack, sheet_rows, Design, Opts, Piece, Sheet};

/// every copy placed once, inside the margins, `gap` apart, within max length
fn check(pieces: &[Piece], o: &Opts, sheets: &[Sheet]) {
    let mut count = vec![0; pieces.len()];
    for s in sheets {
        assert_eq!(s.w, o.width);
        if let Some(l) = o.max_len {
            assert!(s.h <= l, "sheet {} longer than {l}", s.h);
        }
        let boxes: Vec<(usize, usize, usize, usize)> = s
            .placed
            .iter()
            .map(|p| {
                let pc = pieces[p.piece];
                let (w, h) = if p.rot { (pc.h, pc.w) } else { (pc.w, pc.h) };
                (p.x, p.y, w, h)
            })
            .collect();
        for (i, &(x, y, w, h)) in boxes.iter().enumerate() {
            assert!(x >= o.margin && y >= o.margin, "inside the margin");
            assert!(x + w + o.margin <= s.w && y + h + o.margin <= s.h, "past the edge");
            for &(x2, y2, w2, h2) in &boxes[i + 1..] {
                let apart = x + w + o.gap <= x2 || x2 + w2 + o.gap <= x || y + h + o.gap <= y2 || y2 + h2 + o.gap <= y;
                assert!(apart, "overlap or closer than the gap");
            }
        }
        for p in &s.placed {
            count[p.piece] += 1;
            assert!(o.rotate || !p.rot);
        }
    }
    for (i, p) in pieces.iter().enumerate() {
        assert_eq!(count[i], p.count, "copies of piece {i}");
    }
}

#[test]
fn packs_without_overlap_and_respects_limits() {
    let mut seed = 12345u64;
    let mut rnd = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    for case in 0..300 {
        let n = 1 + rnd(8);
        let pieces: Vec<Piece> = (0..n).map(|_| Piece { w: 50 + rnd(900), h: 50 + rnd(1400), count: 1 + rnd(6) }).collect();
        let o = Opts {
            width: 1200 + rnd(800),
            max_len: if case % 3 == 0 { Some(1700 + rnd(3000)) } else { None },
            gap: rnd(40),
            margin: rnd(60),
            rotate: case % 2 == 0,
        };
        match pack(&pieces, &o) {
            Ok(sheets) => check(&pieces, &o, &sheets),
            Err(e) => {
                // only when a design really does not fit the film
                let fits = |w: usize, h: usize| {
                    w + 2 * o.margin <= o.width && o.max_len.is_none_or(|l| h + 2 * o.margin <= l)
                };
                assert!(pieces.iter().any(|p| !(fits(p.w, p.h) || (o.rotate && fits(p.h, p.w)))), "{e}");
            }
        }
    }
}

#[test]
fn fills_the_film_well() {
    // 12 designs of 3 sizes on a 58 cm film at 300 dpi: used area / sheet area
    let pieces = [Piece { w: 3543, h: 4724, count: 4 }, Piece { w: 1181, h: 1181, count: 6 }, Piece { w: 2362, h: 1500, count: 2 }];
    let o = Opts { width: 6850, max_len: None, gap: 59, margin: 59, rotate: true };
    let sheets = pack(&pieces, &o).unwrap();
    check(&pieces, &o, &sheets);
    assert_eq!(sheets.len(), 1);
    let used: usize = pieces.iter().map(|p| p.w * p.h * p.count).sum();
    let fill = used as f64 / (sheets[0].w * sheets[0].h) as f64;
    println!("sheet {} x {} px, fill {:.1} %", sheets[0].w, sheets[0].h, fill * 100.0);
    assert!(fill > 0.75, "fill {fill}");
}

#[test]
fn rotation_and_too_big() {
    // wider than the film: only fits turned
    let p = [Piece { w: 900, h: 300, count: 1 }];
    let o = Opts { width: 500, max_len: None, gap: 0, margin: 10, rotate: true };
    let s = pack(&p, &o).unwrap();
    assert!(s[0].placed[0].rot);
    assert_eq!(s[0].h, 900 + 20);
    assert!(pack(&p, &Opts { rotate: false, ..o }).is_err());
    // max length splits into sheets
    let p = [Piece { w: 400, h: 400, count: 5 }];
    let o = Opts { width: 1000, max_len: Some(1000), gap: 20, margin: 20, rotate: false };
    let s = pack(&p, &o).unwrap();
    check(&p, &o, &s);
    assert_eq!(s.len(), 2, "4 per sheet + 1");
}

#[test]
fn composes_pixels_exactly() {
    // 3 x 2 design with distinct pixels, placed straight and turned
    let mut px = Vec::new();
    for i in 0..6u8 {
        px.extend_from_slice(&[i * 40, 255 - i * 40, i, 255]);
    }
    px[4 * 4 + 3] = 0; // one transparent pixel
    px[4 * 4..4 * 4 + 3].copy_from_slice(&[0, 0, 0]);
    let d = Design { w: 3, h: 2, px };
    let sheet = Sheet {
        w: 10,
        h: 6,
        placed: vec![
            studiozea_engine::gang::Placed { piece: 0, x: 1, y: 1, rot: false, pin: None },
            studiozea_engine::gang::Placed { piece: 0, x: 6, y: 1, rot: true, pin: None },
        ],
    };
    let rows = sheet_rows(&sheet, &[&d], 0, 6);
    let at = |x: usize, y: usize| &rows[(y * 10 + x) * 4..(y * 10 + x) * 4 + 4];
    for y in 0..2 {
        for x in 0..3 {
            assert_eq!(at(1 + x, 1 + y), &d.px[(y * 3 + x) * 4..(y * 3 + x) * 4 + 4]);
        }
    }
    // turned clockwise: 2 wide, 3 tall; top-right of the turned = top-left of the design
    for ly in 0..3 {
        for lx in 0..2 {
            let (sx, sy) = (ly, 1 - lx);
            assert_eq!(at(6 + lx, 1 + ly), &d.px[(sy * 3 + sx) * 4..(sy * 3 + sx) * 4 + 4]);
        }
    }
    assert_eq!(at(7, 1), &d.px[0..4]);
    // everything else transparent
    assert_eq!(at(0, 0), &[0, 0, 0, 0]);
    assert_eq!(at(9, 5), &[0, 0, 0, 0]);
    // banded rows equal the whole
    let mut banded = sheet_rows(&sheet, &[&d], 0, 2);
    banded.extend(sheet_rows(&sheet, &[&d], 2, 6));
    assert_eq!(banded, rows);
}

#[test]
fn turns_only_when_it_saves_film() {
    // three equal designs fit side by side upright: nothing turned
    let p = [Piece { w: 2000, h: 3000, count: 3 }];
    let o = Opts { width: 6850, max_len: None, gap: 59, margin: 59, rotate: true };
    let s = pack(&p, &o).unwrap();
    check(&p, &o, &s);
    assert!(s[0].placed.iter().all(|q| !q.rot));
    assert_eq!(s[0].h, 3000 + 2 * 59);
    // turning the tall one under the other two is shorter than all upright
    let p = [Piece { w: 2598, h: 2903, count: 1 }, Piece { w: 2598, h: 3498, count: 1 }, Piece { w: 984, h: 4723, count: 1 }];
    let up = pack(&p, &Opts { rotate: false, ..o }).unwrap();
    let turned = pack(&p, &o).unwrap();
    check(&p, &o, &turned);
    assert_eq!(up[0].h, 4723 + 2 * 59);
    assert!(turned[0].h < up[0].h, "{} vs {}", turned[0].h, up[0].h);
}

#[test]
fn big_jobs_stay_fast() {
    let p = [Piece { w: 300, h: 420, count: 900 }, Piece { w: 150, h: 150, count: 700 }, Piece { w: 610, h: 200, count: 300 }];
    let o = Opts { width: 6850, max_len: Some(11811), gap: 30, margin: 59, rotate: true };
    let t0 = std::time::Instant::now();
    let s = pack(&p, &o).unwrap();
    let dt = t0.elapsed().as_secs_f64();
    check(&p, &o, &s);
    println!("{} copies -> {} sheets in {dt:.2}s", 1900, s.len());
    assert!(dt < 5.0);
}

#[test]
fn rejects_absurd_options_instead_of_overflowing() {
    let p = [Piece { w: 100, h: 100, count: 1 }];
    let o = Opts { width: 1000, max_len: None, gap: 10, margin: 10, rotate: true };
    assert!(pack(&p, &Opts { margin: usize::MAX / 2 + 1, ..o }).is_err());
    assert!(pack(&p, &Opts { gap: usize::MAX - 5, ..o }).is_err());
    assert!(pack(&[Piece { w: 10, h: 10, count: usize::MAX }, Piece { w: 10, h: 10, count: 2 }], &o).is_err());
    assert!(pack(&p, &Opts { width: usize::MAX, ..o }).is_err());
}

#[test]
fn counts_the_margins_of_every_sheet() {
    // more sheets cost their margins too: never prefer 5 sheets over 4 with less film
    let p = [Piece { w: 827, h: 979, count: 3 }, Piece { w: 847, h: 751, count: 2 }, Piece { w: 978, h: 1208, count: 5 }];
    let o = Opts { width: 2107, max_len: Some(2460), gap: 1, margin: 139, rotate: true };
    let s = pack(&p, &o).unwrap();
    check(&p, &o, &s);
    let film: usize = s.iter().map(|x| x.h).sum();
    println!("{} sheets, {film} px film", s.len());
    assert!(film <= 8411, "{film}");
}

#[test]
fn cancelled_export_keeps_the_old_file() {
    use studiozea_engine::gang::save_sheet;
    let dir = std::env::temp_dir().join(format!("sz_gang_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sheet.png");
    std::fs::write(&path, b"previous export").unwrap();
    let d = Design { w: 2, h: 2, px: vec![255; 16] };
    let sheet = Sheet { w: 4, h: 4, placed: vec![studiozea_engine::gang::Placed { piece: 0, x: 1, y: 1, rot: false, pin: None }] };
    assert!(save_sheet(&sheet, &[&d], 300.0, &path, |_| false).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"previous export");
    assert!(!dir.join("sheet.png.part").exists());
    save_sheet(&sheet, &[&d], 300.0, &path, |_| true).unwrap();
    assert_eq!(&std::fs::read(&path).unwrap()[1..4], b"PNG");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn pinned_copies_stay_where_they_were_put() {
    use studiozea_engine::gang::{pack_with, Pin};
    let pieces = [Piece { w: 300, h: 200, count: 3 }, Piece { w: 150, h: 400, count: 4 }];
    let o = Opts { width: 1000, max_len: None, gap: 10, margin: 20, rotate: true };
    // one copy of each by hand, far from the top left
    let pins = [
        Pin { piece: 0, sheet: 0, x: 600, y: 700, rot: false },
        Pin { piece: 1, sheet: 0, x: 40, y: 900, rot: true },
    ];
    let sheets = pack_with(&pieces, &o, &pins).unwrap();
    check(&pieces, &o, &sheets);
    let placed: Vec<_> = sheets[0].placed.iter().filter(|p| p.pin.is_some()).copied().collect();
    assert_eq!(placed.len(), 2);
    assert!(placed.iter().any(|p| p.pin == Some(0) && (p.x, p.y, p.rot, p.piece) == (600, 700, false, 0)));
    assert!(placed.iter().any(|p| p.pin == Some(1) && (p.x, p.y, p.rot, p.piece) == (40, 900, true, 1)));
    // the sheet reaches the lowest pin
    assert!(sheets[0].h >= 900 + 150 + o.margin);
    // no pins = the same as pack
    assert_eq!(format!("{:?}", pack_with(&pieces, &o, &[]).unwrap()), format!("{:?}", pack(&pieces, &o).unwrap()));
}

#[test]
fn unusable_pins_are_ignored() {
    use studiozea_engine::gang::{pack_with, Pin};
    let pieces = [Piece { w: 300, h: 200, count: 2 }];
    let o = Opts { width: 1000, max_len: Some(1500), gap: 10, margin: 20, rotate: false };
    let pins = [
        Pin { piece: 0, sheet: 0, x: 100, y: 100, rot: false },
        Pin { piece: 0, sheet: 0, x: 405, y: 100, rot: false }, // closer than the gap
        Pin { piece: 0, sheet: 0, x: 5, y: 600, rot: false },   // in the margin
        Pin { piece: 0, sheet: 0, x: 900, y: 600, rot: false }, // past the edge
        Pin { piece: 0, sheet: 0, x: 100, y: 1400, rot: false }, // past the max length
        Pin { piece: 0, sheet: 0, x: 100, y: 600, rot: true },  // turns not allowed
        Pin { piece: 7, sheet: 0, x: 100, y: 600, rot: false }, // no such design
        Pin { piece: 0, sheet: 999_999, x: 100, y: 600, rot: false },
    ];
    let sheets = pack_with(&pieces, &o, &pins).unwrap();
    check(&pieces, &o, &sheets);
    let pinned: Vec<_> = sheets.iter().flat_map(|s| &s.placed).filter_map(|p| p.pin).collect();
    assert_eq!(pinned, vec![0]);
    // more pins than copies: only as many as there are copies
    let many = [Pin { piece: 0, sheet: 0, x: 20, y: 20, rot: false }, Pin { piece: 0, sheet: 0, x: 500, y: 20, rot: false }, Pin { piece: 0, sheet: 0, x: 20, y: 500, rot: false }];
    let sheets = pack_with(&pieces, &o, &many).unwrap();
    check(&pieces, &o, &sheets);
    // a pin on a later sheet (max length): the sheets before it are filled first
    let o2 = Opts { max_len: Some(600), ..o };
    let pieces2 = [Piece { w: 300, h: 200, count: 8 }];
    let sheets = pack_with(&pieces2, &o2, &[Pin { piece: 0, sheet: 1, x: 600, y: 300, rot: false }]).unwrap();
    check(&pieces2, &o2, &sheets);
    assert!(sheets[1].placed.iter().any(|p| p.pin == Some(0) && (p.x, p.y) == (600, 300)));
}

#[test]
fn random_pins_never_overlap() {
    use studiozea_engine::gang::{pack_with, Pin};
    let mut seed = 777u64;
    let mut rnd = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % n as u64) as usize
    };
    for case in 0..300 {
        let n = 1 + rnd(5);
        let pieces: Vec<Piece> = (0..n).map(|_| Piece { w: 50 + rnd(500), h: 50 + rnd(700), count: 1 + rnd(5) }).collect();
        let o = Opts { width: 1200 + rnd(600), max_len: if case % 2 == 0 { Some(1800 + rnd(1500)) } else { None }, gap: rnd(30), margin: rnd(40), rotate: case % 3 != 0 };
        let pins: Vec<Pin> = (0..rnd(8)).map(|_| Pin { piece: rnd(n + 1), sheet: rnd(3), x: rnd(1900), y: rnd(3500), rot: rnd(2) == 1 }).collect();
        if let Ok(sheets) = pack_with(&pieces, &o, &pins) {
            check(&pieces, &o, &sheets);
            for p in sheets.iter().flat_map(|s| &s.placed) {
                if let Some(k) = p.pin {
                    assert_eq!((p.piece, p.x, p.y, p.rot), (pins[k].piece, pins[k].x, pins[k].y, pins[k].rot));
                }
            }
        }
    }
}
