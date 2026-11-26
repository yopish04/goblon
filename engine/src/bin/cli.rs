//! studiozea-cli: image -> print-ready DTF PNG (same options as prototype/render.py)

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::Parser;
use studiozea_engine::{
    distress, io, noise::Grain, BgMode, GridChoice, Job, KMode, Pattern, Settings, Shape, Shirt,
};

#[derive(Parser)]
#[command(version, about = "StudioZea DTF engine")]
struct Args {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    /// hitam | putih | #RRGGBB
    #[arg(long, default_value = "hitam")]
    kaos: String,
    /// screen ruling, 3-150 (low = big decorative dots)
    #[arg(long, default_value_t = 35.0)]
    lpi: f64,
    /// screen angle in degrees (default 22.5; negative values allowed, e.g. -30)
    #[arg(long, allow_negative_numbers = true)]
    sudut: Option<f64>,
    /// auto | ya (C/Y sudut, M sudut+45) | tidak (dot-on-dot)
    #[arg(long, default_value = "auto")]
    rosette: String,
    /// halftone only on fades, 1-100 %: the design keeps its true colours and only the
    /// parts fading into the shirt colour (below this %) become dots
    #[arg(long)]
    gradasi: Option<f32>,
    /// "titik warna asli": the whole design through one screen, every dot in the design's
    /// own colour (not split into pure inks), very light colours solid (the look of
    /// colour-halftone band shirts). Use one angle (e.g. --sudut 22.5) and a low LPI
    #[arg(long)]
    titik_warna: bool,
    /// with --titik-warna: largest dot size, 1-100 % (100 = colours do not fade)
    #[arg(long, default_value_t = 100.0)]
    ukuran_titik: f32,
    /// with --titik-warna: colours at least this light (luminance, % away from the shirt
    /// colour: white, pale highlights) print solid; 100 = never solid
    #[arg(long, default_value_t = 92.0)]
    padat_mulai: f32,
    /// with --titik-warna: colour lift 0-100 %: dots in the colour at full strength
    /// (hue / saturation kept), smaller by the same factor; 0 = the design colour as is
    #[arg(long, default_value_t = 100.0)]
    naik_warna: f32,
    /// no halftone at all: true colours, soft edges flattened onto the shirt colour,
    /// specks < 0.15 mm cleaned (combine with --tekstur for a texture-only vintage look)
    #[arg(long = "tanpa-halftone")]
    tanpa_halftone: bool,
    /// with --tanpa-halftone: colours closer to the shirt colour than this % are not
    /// printed (default 12: near-black shadows / JPEG noise on a black shirt)
    #[arg(long = "ambang-kaos", default_value_t = 12.0)]
    ambang_kaos: f32,
    /// resize filter to the print size: lanczos (sharpest) | bicubic (like Photoshop)
    #[arg(long, default_value = "lanczos")]
    resize: String,
    /// bulat | bulat-murni | elips | kotak | berlian | garis | silang |
    /// gelombang (garis gelombang) | arsir (arsir silang) | kasar (titik kasar, analog) |
    /// bintang | segitiga | rantai
    #[arg(long, default_value = "bulat")]
    bentuk: String,
    /// am | fm | grain-halus | grain-sedang | grain-kasar
    #[arg(long, default_value = "am")]
    pola: String,
    /// auto | rapi | presisi
    #[arg(long, default_value = "auto")]
    grid: String,
    #[arg(long, default_value_t = 300.0)]
    dpi: f64,
    #[arg(long = "lebar-cm")]
    lebar_cm: Option<f64>,
    /// otomatis | warna | tidak
    #[arg(long = "hapus-bg", default_value = "otomatis")]
    hapus_bg: String,
    /// off | garis | gcr
    #[arg(long, default_value = "off")]
    k: String,
    #[arg(long = "tepi-tajam")]
    tepi_tajam: bool,
    #[arg(long, default_value_t = 0.0)]
    knockout: f32,
    #[arg(long)]
    trim: bool,
    /// turn the design clockwise: 0, 90, 180 or 270 degrees (after crop / trim)
    #[arg(long, default_value_t = 0)]
    putar: u16,
    /// mirror the design left-right
    #[arg(long)]
    cermin: bool,
    /// transparent margin around the exported print (mm)
    #[arg(long = "margin-mm", default_value_t = 0.0)]
    margin_mm: f64,
    /// "Lindungi teks": detected lettering prints solid in its own colours, not screened
    #[arg(long = "lindungi-teks")]
    lindungi_teks: bool,
    /// with --lindungi-teks: the lettering in finer dots of the same screen (AM /
    /// interlock) instead of solid
    #[arg(long = "teks-halus")]
    teks_halus: bool,
    /// AI upscale the source first (Real-ESRGAN Compact): 2 or 4 times
    #[arg(long = "ai")]
    ai: Option<usize>,
    /// AI smoothing 0..1: 1 cleans JPEG noise, 0 keeps fine texture
    #[arg(long = "ai-halus", default_value_t = 0.5)]
    ai_halus: f32,
    /// where the AI runs: auto (graphics card if it passes its self-test, else
    /// processor) | cpu | gpu (graphics card or fail; STZ_GPU_SOFTWARE=1 also accepts
    /// software renderers)
    #[arg(long = "ai-perangkat", default_value = "auto")]
    ai_perangkat: String,
    /// with --ai: only write the upscaled image to OUTPUT (no print processing)
    #[arg(long = "ai-saja")]
    ai_saja: bool,
    /// sablon (screen printing): separate into this many inks; OUTPUT = the simulated
    /// print, the films go next to it as OUTPUT_film1_RRGGBB.png ...
    #[arg(long = "sablon")]
    sablon: Option<usize>,
    /// with --sablon: simulasi (halftone) | index | spot
    #[arg(long = "metode", default_value = "simulasi")]
    metode: String,
    /// with --sablon: film LPI (all films one angle, --sudut, default 25)
    #[arg(long = "lpi-film", default_value_t = 55.0)]
    lpi_film: f64,
    /// with --sablon: ink set auto | dasar | dasar-abu-dulu | hemat | gelap
    #[arg(long = "set-tinta", default_value = "auto")]
    set_tinta: String,
    /// with --sablon: dot style satu-sudut | roset | interlock
    #[arg(long = "gaya-titik", default_value = "satu-sudut")]
    gaya_titik: String,
    /// with --sablon: colour films grow this many pixels
    #[arg(long = "expand", default_value_t = 0)]
    expand: usize,
    /// with --sablon: drop inks printing on less than this percent of the design
    #[arg(long = "buang", default_value_t = 0.3)]
    buang: f32,
    /// with --sablon: target chroma boost in percent
    #[arg(long = "boost", default_value_t = 0.0)]
    boost: f32,
    /// with --sablon: "hemat tinta" 0-100 %: less underbase / ink where it changes the
    /// colour only a little
    #[arg(long, default_value_t = 0.0)]
    hemat_tinta: f32,
    /// with --sablon: angle of the white films (underbase, highlight) on their own
    /// screen, e.g. 7.5 with colours at 22.5 (default: the colour angle)
    #[arg(long, allow_hyphen_values = true)]
    sudut_putih: Option<f64>,
    /// eraser strokes (JSON list of {"mode":"hapus"|"kembalikan","r":px,"pts":[[x,y],...]}
    /// in source pixels), applied after background removal, before crop
    #[arg(long = "coretan", value_name = "FILE.json")]
    coretan: Option<PathBuf>,
    /// crop in source pixels: x0,y0,x1,y1 (applied before trim)
    #[arg(long, value_delimiter = ',')]
    crop: Option<Vec<usize>>,
    /// background flood tolerance (default: automatic)
    #[arg(long)]
    toleransi: Option<f32>,
    /// keep enclosed areas of the background colour inside the design
    #[arg(long = "tanpa-kantong")]
    tanpa_kantong: bool,
    /// old full-image render (needs ~65 bytes per print pixel); default renders in bands
    #[arg(long)]
    penuh: bool,
    /// vintage texture layer, repeatable: gambar:key=val,... with keys path
    /// (lib:<name> for a built-in texture, or a file), mode (lubang|campur|pudar),
    /// threshold (hole strength 0..1), amount (fade strength 0..1), zoom, angle,
    /// mirror, invert, tile, scale_mm (texture width; default: cover the print),
    /// offset_mm (x;y from the centre). e.g.
    /// --tekstur gambar:path=lib:vintage-01.jpg,threshold=0.5
    #[arg(long = "tekstur", value_name = "gambar:key=val,...")]
    tekstur: Vec<String>,
    /// list the built-in textures and exit
    #[arg(long = "daftar-tekstur")]
    daftar_tekstur: bool,
}

/// "retak:amount=0.3,scale_mm=4" -> distress::Layer (same lenient parsing as the UI JSON)
fn parse_layer(spec: &str) -> Result<distress::Layer> {
    let (kind, rest) = spec.split_once(':').unwrap_or((spec, ""));
    let kind = distress::Kind::parse(kind)
        .ok_or_else(|| anyhow::anyhow!("unknown tekstur kind '{kind}' (use gambar:path=...)"))?;
    let mut obj = serde_json::Map::new();
    obj.insert("kind".into(), serde_json::Value::String(kind.name().into()));
    let mut last: Option<String> = None;
    for part in rest.split(',').filter(|p| !p.is_empty()) {
        match part.split_once('=') {
            Some((k, v)) => {
                let k = k.trim().to_ascii_lowercase();
                let k = match k.as_str() {
                    "kekuatan" => "threshold".to_string(),
                    "pudar" | "jumlah" => "amount".to_string(),
                    "lebar" | "lebar_mm" | "size" | "scale" => "scale_mm".to_string(),
                    "cermin" => "mirror".to_string(),
                    "ukuran" => "zoom".to_string(),
                    "sudut" => "angle".to_string(),
                    "file" => "path".to_string(),
                    "ambang" => "threshold".to_string(),
                    "balik" => "invert".to_string(),
                    "ulang" => "tile".to_string(),
                    "cara" => "mode".to_string(),
                    "geser" | "geser_mm" | "offset" => "offset_mm".to_string(),
                    _ => k,
                };
                const KEYS: [&str; 11] = [
                    "mode", "amount", "scale_mm", "zoom", "mirror", "angle", "path", "invert",
                    "threshold", "tile", "offset_mm",
                ];
                if !KEYS.contains(&k.as_str()) {
                    bail!("unknown tekstur key '{k}' in '{spec}'");
                }
                obj.insert(k.clone(), serde_json::Value::String(v.to_string()));
                last = Some(k);
            }
            // a ',' inside a value (e.g. a path): glue it back to the previous value
            None => match &last {
                Some(k) => {
                    let prev = obj[k].as_str().unwrap_or_default().to_string();
                    obj.insert(k.clone(), serde_json::Value::String(format!("{prev},{part}")));
                }
                None => bail!("bad tekstur option '{part}' in '{spec}' (use key=val)"),
            },
        }
    }
    Ok(serde_json::from_value(serde_json::Value::Object(obj))?)
}

fn parse_shirt(s: &str) -> Result<Shirt> {
    Ok(match s {
        "hitam" => Shirt::Hitam,
        "putih" => Shirt::Putih,
        _ => {
            let h = s.trim_start_matches('#');
            if h.len() != 6 {
                bail!("kaos must be hitam, putih or #RRGGBB");
            }
            let v = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map(|x| x as f32 / 255.0);
            Shirt::Warna([v(0)?, v(2)?, v(4)?])
        }
    })
}

fn main() -> Result<()> {
    let a = Args::parse();
    if a.daftar_tekstur {
        for n in studiozea_engine::library::names() {
            let (w, h) = distress::texture_size(&format!("{}{n}", distress::LIB_PREFIX)).unwrap_or((0, 0));
            println!("{}{n}  ({w}x{h} px)  {}", distress::LIB_PREFIX, studiozea_engine::library::title(n));
        }
        return Ok(());
    }
    let (Some(input), Some(output)) = (a.input.clone(), a.output.clone()) else {
        bail!("usage: studiozea-cli <INPUT> <OUTPUT> [options] (or --daftar-tekstur)");
    };
    let pattern = match a.pola.as_str() {
        "am" => Pattern::Am,
        "fm" => Pattern::Fm,
        "grain" | "grain-sedang" => Pattern::Grain(Grain::Sedang),
        "grain-halus" => Pattern::Grain(Grain::Halus),
        "grain-kasar" => Pattern::Grain(Grain::Kasar),
        "bayer" => Pattern::Bayer,
        "difusi" => Pattern::Difusi,
        "interlock" => Pattern::Interlock,
        x => bail!("unknown pola {x}"),
    };
    if !(a.lpi.is_finite() && (3.0..=150.0).contains(&a.lpi)) {
        bail!("--lpi must be 3-150 (got {})", a.lpi);
    }
    if a.gradasi.is_some_and(|g| !(g.is_finite() && (1.0..=100.0).contains(&g))) {
        bail!("--gradasi must be 1-100 (%)");
    }
    if a.titik_warna && (a.gradasi.is_some() || a.tanpa_halftone) {
        bail!("--titik-warna cannot be combined with --gradasi or --tanpa-halftone");
    }
    if !(a.ukuran_titik.is_finite() && (1.0..=100.0).contains(&a.ukuran_titik)) || !(a.padat_mulai.is_finite() && (1.0..=100.0).contains(&a.padat_mulai)) {
        bail!("--ukuran-titik and --padat-mulai must be 1-100 (%)");
    }
    if !(a.naik_warna.is_finite() && (0.0..=100.0).contains(&a.naik_warna)) {
        bail!("--naik-warna must be 0-100 (%)");
    }
    if a.sudut.is_some_and(|d| !d.is_finite()) {
        bail!("--sudut must be a number of degrees");
    }
    let settings = Settings {
        shirt: parse_shirt(&a.kaos)?,
        lpi: a.lpi,
        shape: Shape::parse(&a.bentuk)
            .ok_or_else(|| anyhow::anyhow!("unknown bentuk {}", a.bentuk))?,
        dpi: a.dpi,
        grid: match a.grid.as_str() {
            "auto" => GridChoice::Auto,
            "rapi" => GridChoice::Rapi,
            "presisi" => GridChoice::Presisi,
            x => bail!("unknown grid {x}"),
        },
        pattern,
        k: match a.k.as_str() {
            "off" => KMode::Off,
            "garis" => KMode::Garis,
            "gcr" => KMode::Gcr,
            x => bail!("unknown k {x}"),
        },
        knockout: a.knockout,
        edges: a.tepi_tajam,
        angle: a.sudut,
        shirt_cut: (a.ambang_kaos / 100.0).clamp(0.0, 1.0),
        resample: match a.resize.as_str() {
            "lanczos" => studiozea_engine::resize::Resample::Lanczos,
            "bicubic" => studiozea_engine::resize::Resample::Bicubic,
            x => bail!("unknown resize {x} (lanczos | bicubic)"),
        },
        halftone_area: if a.tanpa_halftone { Some(0.0) } else { a.gradasi.map(|g| g / 100.0) },
        rosette: match a.rosette.as_str() {
            "auto" => None,
            "ya" => Some(true),
            "tidak" => Some(false),
            x => bail!("unknown rosette {x} (auto | ya | tidak)"),
        },
        distress: a
            .tekstur
            .iter()
            .map(|t| parse_layer(t))
            .collect::<Result<Vec<_>>>()?,
        true_dots: a.titik_warna.then(|| studiozea_engine::TrueDots { size: a.ukuran_titik / 100.0, solid: a.padat_mulai / 100.0, lift: a.naik_warna / 100.0 }),
        protect_text: a.lindungi_teks,
        text_style: if a.teks_halus { studiozea_engine::TextStyle::Halus } else { studiozea_engine::TextStyle::Padat },
        ..Settings::default()
    };
    if a.crop.as_ref().is_some_and(|c| c.len() != 4) {
        bail!("--crop needs 4 numbers: x0,y0,x1,y1");
    }
    if let Err(e) = distress::validate(&settings) {
        bail!("tekstur: {e}");
    }
    for (i, d) in distress::describe(&settings).iter().enumerate() {
        println!("tekstur {}: {d}", i + 1);
    }
    let erase: Vec<studiozea_engine::erase::Stroke> = match &a.coretan {
        Some(f) => serde_json::from_str(&std::fs::read_to_string(f)?)?,
        None => Vec::new(),
    };
    if let Err(e) = studiozea_engine::erase::validate(&erase) {
        bail!("coretan: {e}");
    }
    if ![0, 90, 180, 270].contains(&a.putar) {
        bail!("--putar must be 0, 90, 180 or 270");
    }
    if !(a.margin_mm.is_finite() && (0.0..=100.0).contains(&a.margin_mm)) {
        bail!("--margin-mm must be 0..100");
    }
    if a.margin_mm > 0.0 && a.penuh {
        bail!("--margin-mm works without --penuh");
    }
    let job = Job {
        rotate: a.putar,
        mirror: a.cermin,
        erase,
        remove_bg: match a.hapus_bg.as_str() {
            "otomatis" => BgMode::Otomatis,
            "warna" => BgMode::Warna,
            "tidak" => BgMode::Tidak,
            x => bail!("unknown hapus-bg {x}"),
        },
        crop: a.crop.as_ref().map(|c| [c[0], c[1], c[2], c[3]]),
        auto_trim: a.trim,
        bg_tolerance: a.toleransi,
        bg_pockets: if a.tanpa_kantong { Some(false) } else { None },
        width_cm: a.lebar_cm,
        settings,
    };
    let src = match a.ai {
        None if a.ai_saja => bail!("--ai-saja needs --ai 2|4"),
        None => io::load(&input)?,
        Some(k) => {
            let scale = match k {
                2 => studiozea_engine::upscale::Scale::X2,
                4 => studiozea_engine::upscale::Scale::X4,
                _ => bail!("--ai must be 2 or 4"),
            };
            if !(a.ai_halus.is_finite() && (0.0..=1.0).contains(&a.ai_halus)) {
                bail!("--ai-halus must be 0..1");
            }
            use studiozea_engine::upscale::{upscale_on, Gpu};
            let img = io::load_rgba8(&input)?;
            let soft = std::env::var("STZ_GPU_SOFTWARE").is_ok_and(|v| v == "1");
            let gpu = match a.ai_perangkat.as_str() {
                "cpu" => None,
                "gpu" => {
                    let g = Gpu::open(soft)?;
                    g.self_test()?;
                    Some(g)
                }
                "auto" => match Gpu::open(soft).and_then(|g| g.self_test().map(|_| g)) {
                    Ok(g) => Some(g),
                    Err(e) => {
                        println!("AI: processor ({e:#})");
                        None
                    }
                },
                x => bail!("unknown ai-perangkat {x} (auto | cpu | gpu)"),
            };
            let t = std::time::Instant::now();
            let (up, rep) = upscale_on(&img, scale, a.ai_halus, gpu.as_ref(), |_, _| true)?;
            let on = match &rep.gpu {
                Some(n) if rep.cpu_tiles == 0 => n.clone(),
                Some(n) => format!("{n} + processor: {}", rep.fallback.clone().unwrap_or_default()),
                None => "processor".into(),
            };
            println!("AI {}x{} -> {}x{} px in {:.2}s on {on}", img.w, img.h, up.w, up.h, t.elapsed().as_secs_f64());
            if a.ai_saja {
                io::save_rgba8(&up, &output)?;
                return Ok(());
            }
            up.to_loaded()
        }
    };
    let t0 = std::time::Instant::now();
    let d = job.settings.dpi;
    // same limit as the app: 400 Mpx (e.g. 100 x 150 cm at 300 dpi)
    let text = if job.settings.protect_text {
        let t = std::time::Instant::now();
        let m = studiozea_engine::text::detect(&src.to_rgba8())?;
        println!("teks: {} tulisan ditemukan ({:.2}s)", m.words, t.elapsed().as_secs_f64());
        Some(m)
    } else {
        None
    };
    let prep = studiozea_engine::prepare_source_text(&src, &job, text.as_ref());
    // maintainers: STZ_TEXT_DEBUG=file.png writes the detected word regions (colours)
    // and the solid mask (white) over the darkened source
    if let (Ok(dbg), Some(m)) = (std::env::var("STZ_TEXT_DEBUG"), text.as_ref()) {
        let solid = studiozea_engine::text::solid_mask(m, &src.rgb, &src.alpha, src.w, src.h);
        let mut px = Vec::with_capacity(src.w * src.h * 4);
        for y in 0..src.h {
            for x in 0..src.w {
                let i = y * src.w + x;
                let l = m.at(x, y) as usize;
                let base = [src.rgb[i * 3], src.rgb[i * 3 + 1], src.rgb[i * 3 + 2]].map(|v| v * 0.35);
                let tint = [(l * 97 % 255) as f32 / 255.0, (l * 57 % 255) as f32 / 255.0, (l * 151 % 255) as f32 / 255.0];
                let c = if solid[i] > 0.0 { [1.0; 3] } else if l > 0 { [0, 1, 2].map(|k| base[k] * 0.4 + tint[k] * 0.6) } else { base };
                px.extend(c.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8).chain([255]));
            }
        }
        io::save_rgba8(&io::Rgba8 { w: src.w, h: src.h, px, has_alpha: true }, std::path::Path::new(&dbg))?;
    }
    let (pw, ph) = studiozea_engine::print_size(&prep, &job);
    if pw * ph > 400_000_000 {
        bail!(
            "print too large: {pw} x {ph} px ({:.0} x {:.0} cm); lower the width, DPI or check the crop",
            pw as f64 / d * 2.54,
            ph as f64 / d * 2.54
        );
    }
    if let Some(n) = a.sablon {
        let sep = studiozea_engine::sablon::SepSettings {
            method: match a.metode.as_str() {
                "simulasi" => studiozea_engine::sablon::Method::Simulasi,
                "index" => studiozea_engine::sablon::Method::Index,
                "spot" => studiozea_engine::sablon::Method::Spot,
                x => bail!("unknown --metode {x} (simulasi | index | spot)"),
            },
            colors: n,
            ink_set: serde_json::from_value(serde_json::Value::String(a.set_tinta.clone())).map_err(|_| anyhow::anyhow!("unknown --set-tinta {}", a.set_tinta))?,
            dot_style: serde_json::from_value(serde_json::Value::String(a.gaya_titik.clone())).map_err(|_| anyhow::anyhow!("unknown --gaya-titik {}", a.gaya_titik))?,
            expand_px: a.expand,
            drop_below: a.buang,
            boost: a.boost / 100.0,
            ink_saver: a.hemat_tinta / 100.0,
            white_angle: a.sudut_putih,
            lpi: a.lpi_film,
            angle: a.sudut.unwrap_or(25.0),
            ..Default::default()
        };
        sep.check().map_err(|e| anyhow::anyhow!(e))?;
        let full = studiozea_engine::prepare_text(&src, &job, text.as_ref());
        let r = studiozea_engine::sablon::separate(&full, &job.settings, &sep);
        io::write_png_rows(r.w, r.h, d, &output, |y0, y1| Ok(r.preview[y0 * r.w * 4..y1 * r.w * 4].to_vec()))?;
        let stem = output.with_extension("");
        for (i, f) in r.films.iter().enumerate() {
            let hex = format!("{:02x}{:02x}{:02x}", f.ink.rgb[0], f.ink.rgb[1], f.ink.rgb[2]);
            let name = format!("{}_film{}_{:?}_{hex}.png", stem.display(), i + 1, f.role).to_lowercase();
            studiozea_engine::sablon::save_film(f, d, 12.0, &format!("{} {:?} #{hex}", i + 1, f.role), std::path::Path::new(&name))?;
            let ang = f.angle.map(|v| format!(" {v:.1}°")).unwrap_or_default();
            println!("film {}: {:?} #{hex} {:.1} %{ang}{}", i + 1, f.role, f.bits.coverage() * 100.0, if f.inverted { " inverted" } else { "" });
        }
        let dropped: Vec<String> = (0..r.inks.len()).filter(|i| !r.films.iter().any(|f| f.src == Some(*i))).map(|i| format!("#{:02x}{:02x}{:02x}", r.inks[i].rgb[0], r.inks[i].rgb[1], r.inks[i].rgb[2])).collect();
        if !dropped.is_empty() {
            println!("dropped: {}", dropped.join(" "));
        }
        println!("{}x{} px; {} films; delta E {:.2}; {:.2}s", r.w, r.h, r.films.len(), r.delta_e, t0.elapsed().as_secs_f64());
        return Ok(());
    }
    let (w, h, checks, bg) = if a.penuh {
        let out = studiozea_engine::run_text(&src, &job, text.as_ref());
        io::save_png_for(&out.rgba, out.w, out.h, d, &output, job.settings.pure_colours())?;
        (out.w, out.h, out.checks, out.bg)
    } else {
        let plan = studiozea_engine::band::Plan::new(prep, pw, ph, job.settings.clone());
        let (packed, checks) = studiozea_engine::band::render_packed(&plan, 4 << 20, |_, _, _| true)
            .expect("not cancelled");
        io::save_png_packed_margin(&packed, &checks, d, &output, io::margin_px(a.margin_mm, d))?;
        (plan.w, plan.h, checks, plan.source().bg)
    };
    let dt = t0.elapsed();
    if let Some(bg) = bg {
        println!("background {:?}", bg.map(|v| (v * 255.0).round() as u8));
    }
    let screen = studiozea_engine::screen_info(d, job.settings.lpi, job.settings.angles()[0], job.settings.grid());
    println!(
        "{}x{} px = {:.1} x {:.1} cm @ {} dpi; screen {:.2} LPI {:.2} deg ({} levels); checks {:?}; {:.2}s",
        w,
        h,
        w as f64 / d * 2.54,
        h as f64 / d * 2.54,
        d,
        screen.lpi,
        screen.angle,
        screen.levels,
        checks,
        dt.as_secs_f64()
    );
    Ok(())
}
