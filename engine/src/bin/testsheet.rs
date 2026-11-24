//! studiozea-testsheet <hitam|putih> <out.png> [dpi]: print calibration sheet

use anyhow::{bail, Result};
use studiozea_engine::{io, testsheet::test_sheet, Shirt};

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        bail!("usage: studiozea-testsheet <hitam|putih> <out.png> [dpi]");
    }
    let shirt = match a[1].as_str() {
        "hitam" => Shirt::Hitam,
        "putih" => Shirt::Putih,
        x => bail!("kaos must be hitam or putih, got {x}"),
    };
    let dpi: f64 = a.get(3).map(|v| v.parse()).transpose()?.unwrap_or(300.0);
    let t0 = std::time::Instant::now();
    let (w, h, px) = test_sheet(shirt, dpi);
    io::save_png(&px, w, h, dpi, std::path::Path::new(&a[2]))?;
    println!(
        "{w}x{h} px ({:.0} x {:.0} mm) @ {dpi} dpi in {:.1}s",
        w as f64 / dpi * 25.4,
        h as f64 / dpi * 25.4,
        t0.elapsed().as_secs_f64()
    );
    Ok(())
}
