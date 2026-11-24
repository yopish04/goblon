//! The chosen rational screen must follow the requested angle over the whole circle
//! (angles above 180 deg once produced a negative angle error and a wrong screen).

use studiozea_engine::{choose_grid, screen_info};

#[test]
fn screen_angle_follows_the_request_all_around() {
    let mut bad = Vec::new();
    for dpi in [150.0, 300.0, 600.0] {
        for lpi in [3.0, 8.0, 20.0, 35.0, 45.0, 65.0, 85.0] {
            // fewer than 4 px per cell cannot hold an angle (the app warns below 30 levels)
            if dpi / lpi < 4.0 {
                continue;
            }
            for k in 0..48 {
                let deg = k as f64 * 7.5;
                let grid = choose_grid(dpi, lpi, deg);
                let s = screen_info(dpi, lpi, deg, grid);
                let d = (s.angle - deg).rem_euclid(360.0);
                let d = d.min(360.0 - d);
                // rapi accepts 3 % error (incl. half the angle error in radians)
                if d > 3.5 || (s.lpi - lpi).abs() / lpi > 0.06 {
                    bad.push(format!("{dpi} dpi {lpi} LPI {deg} deg -> {:.2} deg {:.2} LPI ({grid:?})", s.angle, s.lpi));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{bad:#?}");
}
