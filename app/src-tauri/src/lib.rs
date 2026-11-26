//! StudioZea desktop app: Tauri commands around studiozea-engine.
//!
//! Light-weight pipeline: the prepared image stays at source resolution, the print is
//! rendered band by band in a background thread into a 4-bit packed buffer (34 MB for
//! 30 x 40 cm at 600 dpi), the viewer pulls an overview and tiles at any zoom, and the
//! PNG is streamed from the packed buffer. A new job cancels the running one.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Instant;

use serde::Serialize;
use studiozea_engine::band::{tone_rgba, Overview, Packed, Plan};
use studiozea_engine::{dtf, io, screen_info, Job, Prepared, ScreenInfo, Shirt};
use tauri::{Emitter, Manager};

// ---------------------------------------------------------------- language
/// interface language of the messages: English (default) or Indonesian; set by the UI
static EN: AtomicBool = AtomicBool::new(true);

fn en() -> bool {
    EN.load(Ordering::Relaxed)
}

/// the message in the interface language: tr!("Indonesia {x}", "English {x}")
macro_rules! tr {
    ($id:literal, $en:literal $(, $a:expr)* $(,)?) => {
        if en() { format!($en $(, $a)*) } else { format!($id $(, $a)*) }
    };
}

#[tauri::command]
fn set_lang(lang: String) {
    EN.store(lang != "id", Ordering::Relaxed);
}

/// engine messages are Indonesian; in English the known phrases are translated
/// (longest first), the rest (numbers, names, OS errors) stays as it is
fn eng(msg: impl Into<String>) -> String {
    let mut m: String = msg.into();
    if !en() {
        return m;
    }
    const T: &[(&str, &str)] = &[
        ("tidak ada kartu grafis yang bisa dipakai", "no usable graphics card"),
        ("kartu grafis tidak bisa dipakai", "graphics card not usable"),
        ("hasil tes berbeda dari prosesor", "test result differs from the processor"),
        ("hasil tes tidak valid", "invalid test result"),
        ("terlalu terbatas untuk AI", "too limited for AI"),
        ("memori kartu grafis terlalu kecil", "not enough graphics memory"),
        ("memori kartu grafis penuh", "graphics memory full"),
        ("kartu grafis menolak perintah", "graphics card rejected a command"),
        ("kartu grafis tidak merespons", "graphics card not responding"),
        ("hasil kartu grafis tidak terbaca", "could not read the graphics card result"),
        ("hasil kartu grafis tidak valid", "invalid graphics card result"),
        ("kartu grafis terputus", "graphics card lost"),
        ("kesalahan di driver kartu grafis", "graphics driver error"),
        ("potongan gambar terlalu besar untuk kartu grafis", "tile too large for the graphics card"),
        ("shader ditolak", "shader rejected"),
        ("tidak bisa dibuka", "cannot be opened"),
        ("tanpa dukungan kartu grafis", "built without graphics card support"),
        ("terlalu besar (maks.", "too large (max."),
        ("Gambar ini sudah cukup besar; Perbesar AI untuk gambar kecil.", "This image is big enough already; AI Upscale is for small images."),
        ("Hasil ", "Result "), // whole word only: "Hasil 9000×9000 px terlalu besar" 
        ("gambar kosong", "empty image"),
        ("detektor teks tidak bisa dimuat", "text detector could not load"),
        ("deteksi teks gagal", "text detection failed"),
        ("belum memilih tekstur", "no texture chosen"),
        ("lapisan", "layer"),
        ("tekstur pustaka", "library texture"),
        ("tidak ada di versi ini", "is not in this version"),
        ("tidak bisa dibaca", "cannot be read"),
        ("format tidak dikenal (pakai PNG, JPG, TIFF, WebP atau BMP)", "unknown format (use PNG, JPG, TIFF, WebP or BMP)"),
        ("ukuran gambar tidak didukung", "image size not supported"),
        ("ukuran gambar 0", "image size 0"),
        ("file PNG terpotong", "PNG file is cut off"),
        ("tekstur", "texture"),
        ("ukuran kuas harus antara 1 dan", "brush size must be between 1 and"),
        ("coretan tidak valid", "invalid stroke"),
        ("terlalu banyak coretan", "too many strokes"),
        ("titik, maksimal", "points, max."),
        ("terlalu banyak salinan", "too many copies"),
        ("maksimal", "max."),
        ("maks.", "max."),
        ("ukuran lembar atau desain tidak masuk akal", "sheet or design size makes no sense"),
        ("jarak atau margin lebih besar dari lebar film", "gap or margin larger than the film width"),
        ("lebar film terlalu kecil untuk marginnya", "film too narrow for its margins"),
        ("panjang maksimal terlalu kecil untuk marginnya", "max. length too small for its margins"),
        ("lebih besar dari lembar", "larger than the sheet"),
        ("desain", "design"),
        ("tidak transparan: bukan file hasil export (background masih ada)", "not transparent: not an exported file (background still there)"),
        ("piksel setengah transparan: pakai file hasil export StudioZea", "semi-transparent pixels: use a file exported by StudioZea"),
        ("dibatalkan", "cancelled"),
        ("uji", "test"),
    ];
    for (a, b) in T {
        m = replace_words(&m, a, b);
    }
    m
}

/// part of a word or of a file path / name (never translated)
fn wordish(c: char) -> bool {
    c.is_alphanumeric() || "/\\_-.~".contains(c)
}

/// replace `a` by `b` where it stands on its own (not inside a word, path or file name;
/// a phrase ending in "(" or " " may touch what follows)
fn replace_words(m: &str, a: &str, b: &str) -> String {
    let mut out = String::with_capacity(m.len());
    let mut rest = m;
    while let Some(i) = rest.find(a) {
        let before = rest[..i].chars().next_back();
        let after = rest[i + a.len()..].chars().next();
        let open_end = a.ends_with(['(', ' ', '.']);
        let ok = !before.is_some_and(wordish) && (open_end || !after.is_some_and(|c| c.is_alphanumeric() || "/\\_-".contains(c)));
        out.push_str(&rest[..i]);
        out.push_str(if ok { b } else { a });
        rest = &rest[i + a.len()..];
    }
    out.push_str(rest);
    out
}

const OVERVIEW_MAX: usize = 2048;
const TILE: usize = 512;
/// pixels per render band (~45 bytes of working memory per pixel)
const BAND_PX: usize = 4 << 20;

/// one rendered (or rendering) print
struct RenderJob {
    gen: u32,
    plan: Plan,
    dpi: f64,
    packed: RwLock<Packed>,
    /// per band: finished?
    bands: Vec<AtomicBool>,
    band_rows: usize,
    overview: Mutex<Overview>,
    source_overview: Arc<(Vec<u8>, usize, usize)>,
    /// the prepared image (its "Lindungi teks" mask for the viewer's hatching)
    prep: Arc<studiozea_engine::Prepared>,
    /// overview of that mask, made when the viewer first asks
    text_overview: std::sync::OnceLock<(Vec<u8>, usize, usize)>,
    cancel: AtomicBool,
    done: Mutex<Option<io::Checks>>,
    done_cv: Condvar,
    rows_done: AtomicUsize,
    /// focus row (centre of the visible area) for band order
    focus: AtomicUsize,
}

impl RenderJob {
    fn complete(&self, y0: usize, y1: usize) -> bool {
        let b0 = y0 / self.band_rows;
        let b1 = (y1.max(y0 + 1) - 1) / self.band_rows;
        (b0..=b1.min(self.bands.len() - 1)).all(|b| self.bands[b].load(Ordering::Acquire))
    }
}

struct Source {
    /// increases with every opened image (part of the prepared-image cache key)
    id: u32,
    /// the opened image as 8-bit RGBA (4 bytes per pixel; floats only while preparing)
    img: io::Rgba8,
    name: String,
    overview: Vec<u8>,
    ov_wh: (usize, usize),
    /// "Lindungi teks": the text found in this image (detected on first use; None =
    /// the detector failed, then nothing is protected automatically)
    text: std::sync::OnceLock<Option<Arc<studiozea_engine::text::TextMap>>>,
}

impl Source {
    fn text(&self) -> Option<Arc<studiozea_engine::text::TextMap>> {
        self.text
            .get_or_init(|| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| studiozea_engine::text::detect(&self.img)))
                    .ok()
                    .and_then(|r| r.ok())
                    .map(Arc::new)
            })
            .clone()
    }
}

#[derive(Default)]
struct AppState {
    src: Option<Arc<Source>>,
    job: Option<Arc<RenderJob>>,
    /// sablon mode: the last separation
    sep: Option<Arc<SepJob>>,
}

/// prepared image (+ its overview) for one prep key
struct PrepEntry {
    key: String,
    prep: Arc<Prepared>,
    overview: Arc<(Vec<u8>, usize, usize)>,
}

/// last prepared image; prepare_source runs one at a time (PREP_LOCK) so overlapping
/// requests never prepare in parallel, and a request that is already outdated stops
static PREP: Mutex<Option<PrepEntry>> = Mutex::new(None);
static PREP_LOCK: Mutex<()> = Mutex::new(());
/// 'source' tiles at zoom < 50 % resize a large window; one at a time bounds memory
static SOURCE_TILE_LOCK: Mutex<()> = Mutex::new(());
/// largest source image accepted (pixels): preparing needs ~50-70 bytes per source pixel
const MAX_SOURCE_PX: usize = 120_000_000;

type State<'a> = tauri::State<'a, Mutex<AppState>>;

static GEN: AtomicU32 = AtomicU32::new(0);
static SRC_ID: AtomicU32 = AtomicU32::new(0);
/// only one band render at a time (a cancelled job finishes its current band first)
static RENDER_LOCK: Mutex<()> = Mutex::new(());

/// raw IPC image: [u32 w][u32 h][u32 gen][u32 rows complete] + RGBA8
fn raw(w: usize, h: usize, gen: u32, complete: bool, rgba: &[u8]) -> tauri::ipc::Response {
    let mut v = Vec::with_capacity(16 + rgba.len());
    for x in [w as u32, h as u32, gen, if complete { h as u32 } else { 0 }] {
        v.extend_from_slice(&x.to_le_bytes());
    }
    v.extend_from_slice(rgba);
    tauri::ipc::Response::new(v)
}

fn empty() -> tauri::ipc::Response {
    tauri::ipc::Response::new(vec![0u8; 16])
}

/// continuous-tone RGBA8 overview of a float image (box average, no big buffers)
fn tone_overview(rgb: &[f32], a: &[f32], w: usize, h: usize, max: usize) -> (Vec<u8>, usize, usize) {
    let f = w.max(h).div_ceil(max).max(1);
    if f == 1 {
        return (tone_rgba(rgb, a), w, h);
    }
    let (ow, oh) = (w.div_ceil(f), h.div_ceil(f));
    (box_tone(rgb, a, w, h, 0, 0, ow, oh, f), ow, oh)
}

#[derive(Serialize)]
struct ImageInfo {
    /// changes with every opened image (viewer cache key)
    id: u32,
    name: String,
    w: usize,
    h: usize,
    has_alpha: bool,
    /// fully opaque (no transparent pixel)
    opaque: bool,
    /// colour of a flat background (0..255), None for transparent / busy borders
    background: Option<[u8; 3]>,
}

#[tauri::command]
async fn open_image(path: String, state: State<'_>) -> Result<ImageInfo, String> {
    let pb = PathBuf::from(&path);
    let name = pb
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let src = tauri::async_runtime::spawn_blocking(move || -> Result<Source, String> {
        let img = io::load_rgba8(&pb).map_err(|e| tr!("Gagal membuka gambar: {e:#}", "Cannot open the image: {e:#}"))?;
        if img.w * img.h > MAX_SOURCE_PX {
            return Err(tr!(
                "Gambar terlalu besar: {}×{} px ({:.0} Mpx). Maksimal {} Mpx; kecilkan dulu (mis. ke resolusi cetak 300 DPI).",
                "Image too large: {}×{} px ({:.0} MP). Max. {} MP; make it smaller first (e.g. to 300 DPI print size).",
                img.w,
                img.h,
                (img.w * img.h) as f64 / 1e6,
                MAX_SOURCE_PX / 1_000_000
            ));
        }
        let f = ((img.w.max(img.h) + OVERVIEW_MAX - 1) / OVERVIEW_MAX).max(1);
        let (ow, oh) = (img.w.div_ceil(f), img.h.div_ceil(f));
        let overview = box_rgba8(&img, 0, 0, ow, oh, f);
        Ok(Source {
            id: SRC_ID.fetch_add(1, Ordering::AcqRel) + 1,
            img,
            name,
            overview,
            ov_wh: (ow, oh),
            text: std::sync::OnceLock::new(),
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    let info = ImageInfo {
        id: src.id,
        name: src.name.clone(),
        w: src.img.w,
        h: src.img.h,
        has_alpha: src.img.has_alpha,
        opaque: !src.img.has_alpha || src.img.px.chunks_exact(4).all(|p| p[3] == 255),
        background: studiozea_engine::bg::flat_background(&src.img).map(|c| c.map(|v| (v * 255.0).round() as u8)),
    };
    let mut st = state.lock().unwrap();
    if let Some(j) = &st.job {
        j.cancel.store(true, Ordering::Release);
    }
    st.src = Some(Arc::new(src));
    *PREP.lock().unwrap_or_else(|e| e.into_inner()) = None;
    st.job = None;
    st.sep = None;
    Ok(info)
}

#[derive(Serialize)]
struct Processed {
    gen: u32,
    w: usize,
    h: usize,
    width_cm: f64,
    height_cm: f64,
    dpi: f64,
    screen: ScreenInfo,
    bg: Option<[u8; 3]>,
    bg_used: Option<(f32, bool)>,
    /// part of the source image the print covers (x0, y0, x1, y1), after crop and trim
    src_box: [usize; 4],
    warnings: Vec<Warning>,
    effective_dpi: Option<f64>,
    prep_ms: u128,
    /// "Lindungi teks": words found (None = option off or the detector failed)
    text_words: Option<usize>,
}

/// a warning about the print: `code` (res | levels | dot-on-dot) and its numbers
#[derive(Serialize)]
struct Warning {
    code: &'static str,
    a: f64,
    b: f64,
    c: f64,
}

#[derive(Clone, Serialize)]
struct Progress {
    gen: u32,
    rows: usize,
    h: usize,
}

#[derive(Clone, Serialize)]
struct Done {
    gen: u32,
    h: usize,
    ms: u128,
    checks: io::Checks,
}

#[derive(Clone, Serialize)]
struct Failed {
    gen: u32,
    msg: String,
}

/// what `prepare_source` depends on: the image, background options, crop, trim and the
/// shirt TYPE (not its colour)
fn prep_key(src_id: u32, job: &Job) -> String {
    let shirt = match job.settings.shirt {
        Shirt::Hitam => "hitam",
        Shirt::Putih => "putih",
        Shirt::Warna(_) => "warna",
    };
    serde_json::to_string(&(
        src_id,
        &job.remove_bg,
        job.bg_tolerance,
        job.bg_pockets,
        &job.crop,
        job.auto_trim,
        shirt,
        &job.erase,
        job.rotate,
        job.mirror,
        job.settings.protect_text,
    ))
    .unwrap()
}


/// largest print the app accepts: 400 Mpx (e.g. 100 x 150 cm at 300 dpi = 209 Mpx,
/// 60 x 100 cm at 600 dpi = 335 Mpx); the packed result is 0.5 byte per pixel
const MAX_PRINT_PX: usize = 400_000_000;

/// settings the engine cannot work with (the UI should not send them; old projects might)
/// the Design tab settings for another image: automatic background, no crop / eraser /
/// turn (they belong to the open image)
fn any_image_job(mut job: Job) -> Result<Job, String> {
    job.crop = None;
    job.erase = Vec::new();
    job.bg_tolerance = None;
    job.bg_pockets = None;
    job.rotate = 0;
    job.mirror = false;
    check_job(&job)?;
    Ok(job)
}

fn check_job(job: &Job) -> Result<(), String> {
    let s = &job.settings;
    if !(s.dpi.is_finite() && (100.0..=720.0).contains(&s.dpi)) {
        return Err(tr!("DPI harus antara 100 dan 720", "DPI must be 100–720"));
    }
    if !(s.lpi.is_finite() && (3.0..=150.0).contains(&s.lpi)) {
        return Err(tr!("Ukuran pola (LPI) harus antara 3 dan 150", "LPI must be 3–150"));
    }
    if s.angle.is_some_and(|a| !a.is_finite()) {
        return Err(tr!("Sudut pola tidak valid", "Invalid screen angle"));
    }
    if !(s.shirt_cut.is_finite() && (0.0..=1.0).contains(&s.shirt_cut)) {
        return Err(tr!("Ambang warna kaos harus antara 0 dan 100 %", "Shirt colour threshold must be 0–100 %"));
    }
    if s.halftone_area.is_some_and(|a| !(a.is_finite() && (0.0..=1.0).contains(&a))) {
        return Err(tr!("Area halftone harus antara 1 dan 100 %", "Halftone area must be 1–100 %"));
    }
    if let Some([x0, y0, x1, y1]) = job.crop {
        if x1 <= x0 || y1 <= y0 {
            return Err(tr!("Kotak crop tidak valid", "Invalid crop box"));
        }
    }
    if let Some(cm) = job.width_cm {
        if !(cm.is_finite() && (0.5..=300.0).contains(&cm)) {
            return Err(tr!("Lebar harus antara 0,5 dan 300 cm", "Width must be 0.5–300 cm"));
        }
    }
    studiozea_engine::distress::validate(s).map_err(|e| tr!("Tekstur: {}", "Texture: {}", eng(e)))?;
    studiozea_engine::erase::validate(&job.erase).map_err(|e| tr!("Penghapus: {}", "Eraser: {}", eng(e)))?;
    if ![0, 90, 180, 270].contains(&job.rotate) {
        return Err(tr!("Putar harus 0, 90, 180 atau 270°", "Rotation must be 0, 90, 180 or 270°"));
    }
    Ok(())
}

/// the prepared image (source resolution) of `job` and its overview, from the cache
/// when nothing that matters changed; None when a newer request (`gen`) is waiting
fn prepared_for(src: &Arc<Source>, job: &Job, gen: u32) -> Option<(Arc<Prepared>, Arc<(Vec<u8>, usize, usize)>)> {
    let key = prep_key(src.id, job);
    let _one = PREP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(e) = PREP.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if e.key == key {
            return Some((e.prep.clone(), e.overview.clone()));
        }
    }
    // a newer request is waiting: do not spend memory and time on this one
    if GEN.load(Ordering::Acquire) != gen {
        return None;
    }
    // floats exist only while preparing; the prepared image is shared with the plan
    let text = if job.settings.protect_text { src.text() } else { None };
    let prep = Arc::new(studiozea_engine::prepare_source_text(&src.img.to_loaded(), job, text.as_deref()));
    let overview = Arc::new(tone_overview(&prep.rgb, &prep.alpha, prep.w, prep.h, OVERVIEW_MAX));
    *PREP.lock().unwrap_or_else(|e| e.into_inner()) = Some(PrepEntry {
        key,
        prep: prep.clone(),
        overview: overview.clone(),
    });
    Some((prep, overview))
}

#[tauri::command]
async fn process(job: Job, app: tauri::AppHandle, state: State<'_>) -> Result<Processed, String> {
    check_job(&job)?;
    let s = job.settings.clone();
    // generation of this request: newer requests always win
    let gen = GEN.fetch_add(1, Ordering::AcqRel) + 1;
    let src = {
        let st = state.lock().unwrap();
        // cancel the running render right away
        if let Some(j) = &st.job {
            j.cancel.store(true, Ordering::Release);
        }
        st.src.clone().ok_or_else(|| tr!("Belum ada gambar", "No image yet"))?
    };
    let t0 = Instant::now();
    let (job2, src2) = (job.clone(), src.clone());
    let prepared = tauri::async_runtime::spawn_blocking(move || prepared_for(&src2, &job2, gen))
        .await
        .map_err(|e| e.to_string())?;
    let Some((prep, source_ov)) = prepared else {
        return Err(tr!("Dibatalkan (pengaturan berubah)", "Cancelled (settings changed)"));
    };
    let (w, h) = studiozea_engine::print_size(&prep, &job);
    if w * h > MAX_PRINT_PX {
        return Err(tr!(
            "Ukuran cetak terlalu besar: {w}×{h} px ({:.0}×{:.0} cm). Kecilkan lebar, DPI, atau periksa crop.",
            "Print too large: {w}×{h} px ({:.0}×{:.0} cm). Lower the width or DPI, or check the crop.",
            w as f64 / s.dpi * 2.54,
            h as f64 / s.dpi * 2.54
        ));
    }
    let plan = Plan::from_arc(prep.clone(), w, h, s.clone());
    let band_rows = plan.band_rows(BAND_PX);
    let nb = h.div_ceil(band_rows);
    let rj = Arc::new(RenderJob {
        gen,
        dpi: s.dpi,
        packed: RwLock::new(Packed::for_settings(w, h, &s)),
        bands: (0..nb).map(|_| AtomicBool::new(false)).collect(),
        band_rows,
        overview: Mutex::new(Overview::new(w, h, OVERVIEW_MAX)),
        source_overview: source_ov,
        prep: prep.clone(),
        text_overview: std::sync::OnceLock::new(),
        cancel: AtomicBool::new(false),
        done: Mutex::new(None),
        done_cv: Condvar::new(),
        rows_done: AtomicUsize::new(0),
        focus: AtomicUsize::new(h / 2),
        plan,
    });
    let prep_ms = t0.elapsed().as_millis();
    {
        let mut st = state.lock().unwrap();
        // another image was opened while this one was being prepared
        if !st.src.as_ref().is_some_and(|x| Arc::ptr_eq(x, &src)) {
            return Err(tr!("Gambar sudah diganti", "The image was replaced"));
        }
        // an older process call finishing late must not replace a newer job
        if st.job.as_ref().is_some_and(|j| j.gen > gen) {
            return Err(tr!("Dibatalkan (pengaturan berubah)", "Cancelled (settings changed)"));
        }
        if let Some(j) = &st.job {
            j.cancel.store(true, Ordering::Release);
        }
        st.job = Some(rj.clone());
    }
    std::thread::spawn(move || run_job(rj, app, t0));

    // warnings as codes + numbers; the UI writes them in its language (so switching
    // the language needs no new render)
    let mut warnings = Vec::new();
    let effective_dpi = job.width_cm.map(|cm| prep.w as f64 / (cm / 2.54));
    if let Some(e) = effective_dpi {
        if e < 150.0 {
            warnings.push(Warning { code: "res", a: e, b: 0.0, c: 0.0 });
        }
    }
    let levels = (s.dpi / s.lpi).powi(2);
    if levels < 30.0 {
        warnings.push(Warning { code: "levels", a: s.lpi, b: s.dpi, c: levels });
    }
    if s.lpi >= 55.0 && s.shirt != Shirt::Hitam && s.angles.is_none() && s.rosette.is_none() {
        warnings.push(Warning { code: "dot-on-dot", a: s.lpi, b: 0.0, c: 0.0 });
    }
    Ok(Processed {
        gen,
        w,
        h,
        width_cm: w as f64 / s.dpi * 2.54,
        height_cm: h as f64 / s.dpi * 2.54,
        dpi: s.dpi,
        screen: screen_info(s.dpi, s.lpi, s.angles()[0], s.grid()),
        bg: prep.bg.map(|b| b.map(|v| (v * 255.0).round() as u8)),
        bg_used: prep.bg_used,
        src_box: prep.src_box,
        warnings,
        effective_dpi,
        prep_ms,
        text_words: if s.protect_text { src.text.get().and_then(|t| t.as_ref()).map(|t| t.words) } else { None },
    })
}

/// background render: bands nearest to the visible area first
fn run_job(rj: Arc<RenderJob>, app: tauri::AppHandle, t0: Instant) {
    let _guard = RENDER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (w, h) = (rj.plan.w, rj.plan.h);
    let nb = rj.bands.len();
    let mut checks = io::Checks {
        semi: 0,
        impure: 0,
        tiny: 0,
        hidden: 0,
    };
    for _ in 0..nb {
        if rj.cancel.load(Ordering::Acquire) {
            return;
        }
        let fb = (rj.focus.load(Ordering::Relaxed) / rj.band_rows).min(nb - 1);
        // nearest unfinished band to the focus
        let b = (0..nb)
            .filter(|&b| !rj.bands[b].load(Ordering::Acquire))
            .min_by_key(|&b| (b as isize - fb as isize).unsigned_abs())
            .unwrap();
        let y0 = b * rj.band_rows;
        let y1 = (y0 + rj.band_rows).min(h);
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rj.plan.render(0, y0, w, y1)));
        let o = match res {
            Ok(o) => o,
            Err(_) => {
                // wake an export waiting for this render (it then reports the failure)
                rj.cancel.store(true, Ordering::Release);
                rj.done_cv.notify_all();
                let _ = app.emit("render-error", Failed { gen: rj.gen, msg: "Render gagal (kesalahan internal)".into() });
                return;
            }
        };
        let c = studiozea_engine::band::pixel_checks_for(&o, rj.plan.settings.pure_colours());
        checks.semi += c.semi;
        checks.impure += c.impure;
        checks.hidden += c.hidden;
        rj.packed.write().unwrap().put_rows(y0, &o);
        rj.overview.lock().unwrap().add_rows(y0, &o);
        rj.bands[b].store(true, Ordering::Release);
        let rows = rj.rows_done.fetch_add(y1 - y0, Ordering::AcqRel) + (y1 - y0);
        let _ = app.emit("render-progress", Progress { gen: rj.gen, rows, h });
    }
    checks.tiny = rj.packed.read().unwrap().tiny_islands(rj.dpi);
    *rj.done.lock().unwrap() = Some(checks);
    rj.done_cv.notify_all();
    let _ = app.emit(
        "render-done",
        Done {
            gen: rj.gen,
            h,
            ms: t0.elapsed().as_millis(),
            checks,
        },
    );
}

fn current(state: &State<'_>) -> Result<Arc<RenderJob>, String> {
    state.lock().unwrap().job.clone().ok_or_else(|| tr!("Belum diproses", "Not processed yet"))
}

/// "Lindungi teks" mask as white with alpha = share of protected pixels (f x f mask
/// pixels each), for the viewer's hatching
fn box_mask(m: &[f32], w: usize, h: usize, tw: usize, th: usize, f: usize) -> Vec<u8> {
    let mut out = vec![0u8; tw * th * 4];
    out.chunks_mut(tw * 4).enumerate().for_each(|(ty, row)| {
        for tx in 0..tw {
            let (mut n, mut k) = (0u32, 0u32);
            for y in ty * f..((ty + 1) * f).min(h) {
                for x in tx * f..((tx + 1) * f).min(w) {
                    n += 1;
                    k += (m[y * w + x] >= 0.5) as u32;
                }
            }
            if k > 0 {
                row[tx * 4..tx * 4 + 4].copy_from_slice(&[255, 255, 255, ((k * 255 + n / 2) / n) as u8]);
            }
        }
    });
    out
}

/// overview image: kind = orig | source | result | text (the "Lindungi teks" areas)
#[tauri::command]
fn overview(kind: String, state: State<'_>) -> Result<tauri::ipc::Response, String> {
    if kind == "orig" {
        let src = state.lock().unwrap().src.clone().ok_or_else(|| tr!("Belum ada gambar", "No image yet"))?;
        let (w, h) = src.ov_wh;
        return Ok(raw(w, h, 0, true, &src.overview));
    }
    let Some(rj) = state.lock().unwrap().job.clone() else {
        return Ok(empty());
    };
    if kind == "source" {
        let (ref px, w, h) = *rj.source_overview;
        return Ok(raw(w, h, rj.gen, true, px));
    }
    if kind == "text" {
        let Some(m) = rj.prep.solid.as_ref().filter(|_| rj.plan.settings.protect_text) else {
            return Ok(empty());
        };
        let (ref px, w, h) = *rj.text_overview.get_or_init(|| {
            let (w, h) = (rj.prep.w, rj.prep.h);
            let f = w.max(h).div_ceil(OVERVIEW_MAX).max(1);
            let (ow, oh) = (w.div_ceil(f), h.div_ceil(f));
            (box_mask(m, w, h, ow, oh, f), ow, oh)
        });
        return Ok(raw(w, h, rj.gen, true, px));
    }
    let ov = rj.overview.lock().unwrap();
    let complete = rj.done.lock().unwrap().is_some();
    Ok(raw(ov.w, ov.h, rj.gen, complete, &ov.rgba()))
}

/// tile (TILE x TILE output pixels, each = 2^level x 2^level print / source pixels)
#[tauri::command]
async fn tile(kind: String, level: u32, tx: usize, ty: usize, state: State<'_>) -> Result<tauri::ipc::Response, String> {
    let level = level.min(8);
    let f = 1usize << level;
    if kind == "orig" {
        let src = state.lock().unwrap().src.clone().ok_or_else(|| tr!("Belum ada gambar", "No image yet"))?;
        return tauri::async_runtime::spawn_blocking(move || {
            let l = &src.img;
            let (x0, y0) = (tx * TILE * f, ty * TILE * f);
            if x0 >= l.w || y0 >= l.h {
                return empty();
            }
            let tw = (l.w - x0).div_ceil(f).min(TILE);
            let th = (l.h - y0).div_ceil(f).min(TILE);
            let rgba = box_rgba8(l, x0, y0, tw, th, f);
            raw(tw, th, 0, true, &rgba)
        })
        .await
        .map_err(|e| e.to_string());
    }
    let rj = current(&state)?;
    let (pw, ph) = (rj.plan.w, rj.plan.h);
    let (x0, y0) = (tx * TILE * f, ty * TILE * f);
    if x0 >= pw || y0 >= ph {
        return Ok(empty());
    }
    let tw = (pw - x0).div_ceil(f).min(TILE);
    let th = (ph - y0).div_ceil(f).min(TILE);
    let (x1, y1) = ((x0 + tw * f).min(pw), (y0 + th * f).min(ph));
    // the viewer is looking here: render these bands first
    rj.focus.store((y0 + y1) / 2, Ordering::Relaxed);
    tauri::async_runtime::spawn_blocking(move || {
        if kind == "source" {
            // zoomed-out source tiles resize (512 * 2^level)^2 print pixels: one at a time
            let _one = (level > 0).then(|| SOURCE_TILE_LOCK.lock().unwrap_or_else(|e| e.into_inner()));
            let (c, a) = rj.plan.prepared(x0, y0, x1, y1);
            let rgba = box_tone(&c, &a, x1 - x0, y1 - y0, 0, 0, tw, th, f);
            return raw(tw, th, rj.gen, true, &rgba);
        }
        if kind == "text" {
            let _one = (level > 0).then(|| SOURCE_TILE_LOCK.lock().unwrap_or_else(|e| e.into_inner()));
            let Some(m) = rj.plan.solid(x0, y0, x1, y1) else { return empty() };
            return raw(tw, th, rj.gen, true, &box_mask(&m, x1 - x0, y1 - y0, tw, th, f));
        }
        if !rj.complete(y0, y1) {
            return raw(tw, th, rj.gen, false, &vec![0u8; tw * th * 4]);
        }
        let rgba = rj.packed.read().unwrap().box_rgba(x0, y0, tw, th, level);
        raw(tw, th, rj.gen, true, &rgba)
    })
    .await
    .map_err(|e| e.to_string())
}

/// box-averaged straight-alpha RGBA8 of an 8-bit image region (f x f source pixels each)
fn box_rgba8(img: &io::Rgba8, x0: usize, y0: usize, tw: usize, th: usize, f: usize) -> Vec<u8> {
    let mut out = vec![0u8; tw * th * 4];
    out.chunks_mut(tw * 4).enumerate().for_each(|(ty, row)| {
        for tx in 0..tw {
            let (mut n, mut sa, mut s) = (0u64, 0u64, [0u64; 3]);
            for dy in 0..f {
                let y = y0 + ty * f + dy;
                if y >= img.h {
                    break;
                }
                for dx in 0..f {
                    let x = x0 + tx * f + dx;
                    if x >= img.w {
                        break;
                    }
                    let p = &img.px[(y * img.w + x) * 4..(y * img.w + x) * 4 + 4];
                    let a = if img.has_alpha { p[3] as u64 } else { 255 };
                    n += 1;
                    sa += a;
                    for c in 0..3 {
                        s[c] += p[c] as u64 * a;
                    }
                }
            }
            let o = &mut row[tx * 4..tx * 4 + 4];
            if n > 0 && sa > 0 {
                for c in 0..3 {
                    o[c] = ((s[c] + sa / 2) / sa) as u8;
                }
                o[3] = ((sa + n / 2) / n) as u8;
            }
        }
    });
    out
}

/// box-averaged straight-alpha RGBA8 of a float image region
fn box_tone(
    rgb: &[f32],
    a: &[f32],
    w: usize,
    h: usize,
    x0: usize,
    y0: usize,
    tw: usize,
    th: usize,
    f: usize,
) -> Vec<u8> {
    let mut out = vec![0u8; tw * th * 4];
    for ty in 0..th {
        for tx in 0..tw {
            let (mut n, mut sa, mut s) = (0f32, 0f32, [0f32; 3]);
            for dy in 0..f {
                let y = y0 + ty * f + dy;
                if y >= h {
                    break;
                }
                for dx in 0..f {
                    let x = x0 + tx * f + dx;
                    if x >= w {
                        break;
                    }
                    let q = y * w + x;
                    n += 1.0;
                    sa += a[q];
                    for c in 0..3 {
                        s[c] += rgb[q * 3 + c] * a[q];
                    }
                }
            }
            let o = &mut out[(ty * tw + tx) * 4..(ty * tw + tx) * 4 + 4];
            if n > 0.0 && sa > 0.0 {
                for c in 0..3 {
                    o[c] = (s[c] / sa * 255.0).round().clamp(0.0, 255.0) as u8;
                }
                o[3] = (sa / n * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

#[derive(Serialize)]
struct Exported {
    path: String,
    bytes: u64,
    w: usize,
    h: usize,
    checks: io::Checks,
}

#[tauri::command]
async fn export_png(path: String, margin_mm: Option<f64>, state: State<'_>) -> Result<Exported, String> {
    let margin_mm = margin_mm.unwrap_or(0.0);
    if !(margin_mm.is_finite() && (0.0..=100.0).contains(&margin_mm)) {
        return Err(tr!("Margin harus 0 sampai 100 mm", "Margin must be 0–100 mm"));
    }
    let rj = current(&state)?;
    let pb = PathBuf::from(&path);
    let pb2 = pb.clone();
    let rj2 = rj.clone();
    let checks = tauri::async_runtime::spawn_blocking(move || -> Result<io::Checks, String> {
        // wait for the render to finish
        let mut g = rj2.done.lock().unwrap();
        while g.is_none() {
            if rj2.cancel.load(Ordering::Acquire) {
                return Err(tr!("Render dibatalkan (pengaturan berubah). Coba export lagi.", "Render cancelled (settings changed). Try the export again."));
            }
            g = rj2.done_cv.wait_timeout(g, std::time::Duration::from_millis(200)).unwrap().0;
        }
        let checks = g.unwrap();
        drop(g);
        let p = rj2.packed.read().unwrap();
        io::save_png_packed_margin(&p, &checks, rj2.dpi, &pb2, io::margin_px(margin_mm, rj2.dpi)).map_err(|e| tr!("Gagal menyimpan: {e}", "Cannot save: {e}"))?;
        Ok(checks)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(Exported {
        bytes: std::fs::metadata(&pb).map(|m| m.len()).unwrap_or(0),
        path,
        w: rj.plan.w + 2 * io::margin_px(margin_mm, rj.dpi),
        h: rj.plan.h + 2 * io::margin_px(margin_mm, rj.dpi),
        checks,
    })
}

// ---------------------------------------------------------------- batch export
/// a batch is running / should stop
static BATCH_RUNNING: AtomicBool = AtomicBool::new(false);
static BATCH_CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Serialize)]
struct BatchItem {
    /// 0-based index, total
    i: usize,
    n: usize,
    name: String,
    /// "mulai" | "ai" (frac = part upscaled) | "render" (frac = part rendered) | "ok" | "gagal"
    state: &'static str,
    frac: f64,
    msg: String,
}

#[derive(Clone, Serialize)]
struct BatchDone {
    ok: usize,
    failed: usize,
    cancelled: bool,
}

/// `{dir}/{name}.png`, or `{name}-2.png`, `-3`... when that file exists already
fn free_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let mut p = dir.join(format!("{name}.png"));
    let mut k = 2;
    while p.exists() {
        p = dir.join(format!("{name}-{k}.png"));
        k += 1;
    }
    p
}

/// one image of a batch: open, prepare (automatic background, no crop / eraser), size
/// (fixed width or fit in a box), render band by band, save. Returns the output path.
/// AI factor for a design `src_w` px wide printed `width_cm` wide at `dpi`: None when the
/// source is sharp enough (>= LOW_DPI effective), else 2 or 4 (4 when 2 is not enough)
fn ai_factor(src_w: usize, width_cm: f64, dpi: f64) -> Option<upscale::Scale> {
    let eff = src_w as f64 / (width_cm / 2.54);
    if !(eff.is_finite() && eff > 0.0) || eff >= LOW_DPI {
        return None;
    }
    Some(if eff * 2.0 >= dpi.min(300.0) { upscale::Scale::X2 } else { upscale::Scale::X4 })
}

/// one image of a batch: open, prepare (automatic background, no crop / eraser), size
/// (fixed width or fit in a box), optionally AI upscale a low-resolution source first,
/// render band by band, save. Returns the output path.
fn batch_one(
    path: &std::path::Path,
    out_dir: &std::path::Path,
    job: &Job,
    fit_cm: Option<[f64; 2]>,
    margin_mm: f64,
    suffix: &str,
    ai: Option<(f32, &str)>,
    app: &tauri::AppHandle,
    cancel: &AtomicBool,
    mut on_frac: impl FnMut(&'static str, f64) + Send,
) -> Result<PathBuf, String> {
    let img = io::load_rgba8(path).map_err(|e| tr!("file rusak atau bukan gambar ({e:#})", "damaged file or not an image ({e:#})"))?;
    if img.w * img.h > MAX_SOURCE_PX {
        return Err(tr!("gambar terlalu besar ({:.0} Mpx, maksimal {})", "image too large ({:.0} MP, max. {})", (img.w * img.h) as f64 / 1e6, MAX_SOURCE_PX / 1_000_000));
    }
    // one prepare at a time with the interactive view (it needs ~50-70 bytes per source px);
    // the 8-bit image is kept only when the AI may need it again
    let prepare = |img: io::Rgba8, keep: bool| {
        // "Lindungi teks": the text of this image (a failed detection protects nothing)
        let text = if job.settings.protect_text { studiozea_engine::text::detect(&img).ok() } else { None };
        let _one = PREP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let src = img.to_loaded();
        let img = keep.then_some(img);
        (studiozea_engine::prepare_source_text(&src, job, text.as_ref()), img)
    };
    let width_for = |prep: &Prepared| match fit_cm {
        // as large as possible inside bw x bh cm, same aspect ratio
        Some([bw, bh]) => bw.min(bh * prep.w as f64 / prep.h as f64),
        None => job.width_cm.unwrap_or(0.0),
    };
    let (mut prep, img) = prepare(img, ai.is_some());
    if let (Some((dn, device)), Some(img)) = (ai, img) {
        // the design as printed (after trim) decides: too few pixels -> AI first, again
        if let Some(scale) = ai_factor(prep.w, width_for(&prep), job.settings.dpi) {
            let (ow, oh) = upscale::out_size(img.w, img.h, scale);
            if ow * oh <= upscale::MAX_OUT_PX {
                drop(prep);
                let report = Mutex::new(&mut on_frac);
                let (up, _) = ai_run(&img, scale, dn, device, app, |d, t| {
                    (report.lock().unwrap_or_else(|e| e.into_inner()))("ai", d as f64 / t.max(1) as f64);
                    !cancel.load(Ordering::Acquire)
                })
                .map_err(|e| if cancel.load(Ordering::Acquire) { "dibatalkan".into() } else { tr!("Perbesar AI: {}", "AI Upscale: {}", eng(format!("{e:#}"))) })?;
                drop(img);
                prep = prepare(up, false).0;
            }
        }
    }
    let mut job = job.clone();
    job.width_cm = Some(width_for(&prep));
    let (w, h) = studiozea_engine::print_size(&prep, &job);
    if w * h > MAX_PRINT_PX {
        return Err(tr!("ukuran cetak terlalu besar ({w}×{h} px)", "print too large ({w}×{h} px)"));
    }
    let s = &job.settings;
    let plan = Plan::from_arc(Arc::new(prep), w, h, s.clone());
    let res = studiozea_engine::band::render_packed(&plan, BAND_PX, |_, y1, _| {
        on_frac("render", y1 as f64 / h as f64);
        !cancel.load(Ordering::Acquire)
    });
    let Some((packed, checks)) = res else {
        return Err("dibatalkan".into());
    };
    let base: String = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "desain".into()).chars().take(100).collect();
    let name = format!("{base}_{:.0}x{:.0}cm{suffix}", w as f64 / s.dpi * 2.54, h as f64 / s.dpi * 2.54);
    let out = free_path(out_dir, &name);
    io::save_png_packed_margin(&packed, &checks, s.dpi, &out, io::margin_px(margin_mm, s.dpi)).map_err(|e| tr!("gagal menyimpan: {e}", "cannot save: {e}"))?;
    Ok(out)
}

/// export many images with the same settings, one after the other (events
/// "batch-item" per step and "batch-done" at the end; batch_cancel stops it)
#[tauri::command]
fn batch_export(
    paths: Vec<String>,
    out_dir: String,
    job: Job,
    fit_cm: Option<[f64; 2]>,
    margin_mm: Option<f64>,
    suffix: String,
    ai: Option<f32>,
    ai_device: Option<String>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let ai_device = ai_device.unwrap_or_else(|| "otomatis".into());
    let margin_mm = margin_mm.unwrap_or(0.0);
    if ai.is_some_and(|d| !(d.is_finite() && (0.0..=1.0).contains(&d))) {
        return Err(tr!("Kehalusan AI harus 0..1", "AI smoothing must be 0..1"));
    }
    if !(margin_mm.is_finite() && (0.0..=100.0).contains(&margin_mm)) {
        return Err(tr!("Margin harus 0 sampai 100 mm", "Margin must be 0–100 mm"));
    }
    let job = any_image_job(job)?;
    if let Some([bw, bh]) = fit_cm {
        if !(bw.is_finite() && bh.is_finite() && (0.5..=300.0).contains(&bw) && (0.5..=300.0).contains(&bh)) {
            return Err(tr!("Kotak harus antara 0,5 dan 300 cm", "Box must be 0.5–300 cm"));
        }
    } else if job.width_cm.is_none() {
        return Err(tr!("Isi lebar cetak dulu", "Enter a print width first"));
    }
    if paths.is_empty() {
        return Err(tr!("Belum ada gambar dipilih", "No images chosen"));
    }
    let dir = PathBuf::from(&out_dir);
    if !dir.is_dir() {
        return Err(tr!("Folder hasil tidak ada: {out_dir}", "Output folder not found: {out_dir}"));
    }
    // keep the name safe for Windows file names
    let suffix: String = suffix.chars().filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c)).take(80).collect();
    if BATCH_RUNNING.swap(true, Ordering::AcqRel) {
        return Err(tr!("Export banyak masih berjalan", "Batch export is still running"));
    }
    BATCH_CANCEL.store(false, Ordering::Release);
    std::thread::spawn(move || {
        let n = paths.len();
        let (mut ok, mut failed) = (0, 0);
        for (i, p) in paths.iter().enumerate() {
            if BATCH_CANCEL.load(Ordering::Acquire) {
                break;
            }
            let pb = PathBuf::from(p);
            let name = pb.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let item = |state: &'static str, frac: f64, msg: String| BatchItem { i, n, name: name.clone(), state, frac, msg };
            let _ = app.emit("batch-item", item("mulai", 0.0, String::new()));
            let mut last = 0.0;
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut phase = "";
                batch_one(&pb, &dir, &job, fit_cm, margin_mm, &suffix, ai.map(|d| (d, ai_device.as_str())), &app, &BATCH_CANCEL, |st, f| {
                    if st != phase || f - last >= 0.05 || f >= 1.0 {
                        (phase, last) = (st, f);
                        let _ = app.emit("batch-item", item(st, f, String::new()));
                    }
                })
            }))
            .unwrap_or_else(|_| Err(tr!("kesalahan internal", "internal error")));
            match r {
                Ok(out) => {
                    ok += 1;
                    let _ = app.emit("batch-item", item("ok", 1.0, out.to_string_lossy().into_owned()));
                }
                Err(e) => {
                    if BATCH_CANCEL.load(Ordering::Acquire) {
                        break;
                    }
                    failed += 1;
                    let _ = app.emit("batch-item", item("gagal", 0.0, e));
                }
            }
        }
        let cancelled = BATCH_CANCEL.load(Ordering::Acquire);
        BATCH_RUNNING.store(false, Ordering::Release);
        let _ = app.emit("batch-done", BatchDone { ok, failed, cancelled });
    });
    Ok(())
}

#[tauri::command]
fn batch_cancel() {
    BATCH_CANCEL.store(true, Ordering::Release);
}

// ---------------------------------------------------------------- AI upscale
use studiozea_engine::upscale;

/// below this effective resolution the app suggests the AI upscale
const LOW_DPI: f64 = 150.0;
static AI_RUNNING: AtomicBool = AtomicBool::new(false);
static AI_CANCEL: AtomicBool = AtomicBool::new(false);

// The graphics card is chosen once per start, on the first AI use ("otomatis"): open
// it, compare it with the processor on a small image, and keep it only when both agree.
// A card that fails the test or breaks off while working is remembered in
// ai_gpu.json (app data folder) and not tried again until "coba lagi".
enum GpuState {
    Untested,
    Ready(Arc<upscale::Gpu>),
    Off(String),
}

static GPU: Mutex<GpuState> = Mutex::new(GpuState::Untested);

fn gpu_file(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("ai_gpu.json"))
}

/// remember a card that misbehaved (name + reason)
fn gpu_disable(app: &tauri::AppHandle, name: &str, reason: &str) {
    *GPU.lock().unwrap_or_else(|e| e.into_inner()) = GpuState::Off(format!("{name}: {reason}"));
    if let Some(f) = gpu_file(app) {
        let _ = std::fs::create_dir_all(f.parent().unwrap_or(std::path::Path::new(".")));
        let _ = write_atomic(&f, &serde_json::json!({ "name": name, "reason": reason }).to_string());
    }
}

/// the graphics card for an AI run, None = processor ("cpu", no card, or a card that
/// failed before)
fn gpu_for(device: &str, app: &tauri::AppHandle) -> Option<Arc<upscale::Gpu>> {
    if device != "otomatis" {
        return None;
    }
    let mut st = GPU.lock().unwrap_or_else(|e| e.into_inner());
    if let GpuState::Untested = *st {
        let off: Option<(String, String)> = gpu_file(app)
            .and_then(|f| read_small(&f).ok())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| Some((v["name"].as_str()?.to_string(), v["reason"].as_str()?.to_string())));
        // STZ_GPU_SOFTWARE=1 also accepts software renderers (testing without a card)
        let soft = std::env::var("STZ_GPU_SOFTWARE").is_ok_and(|v| v == "1");
        let opened = std::panic::catch_unwind(|| upscale::Gpu::open(soft))
            .unwrap_or_else(|_| Err(anyhow::anyhow!("kesalahan di driver kartu grafis")));
        *st = match opened {
            Err(e) => GpuState::Off(format!("{e:#}")),
            Ok(g) => match off {
                Some((name, reason)) if name == g.name() => GpuState::Off(format!("{name}: {reason}")),
                _ => {
                    let test = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.self_test()))
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("kesalahan di driver kartu grafis")));
                    match test {
                        Ok(()) => GpuState::Ready(Arc::new(g)),
                        Err(e) => {
                            let name = g.name().to_string();
                            drop(st);
                            gpu_disable(app, &name, &format!("{e:#}"));
                            return None;
                        }
                    }
                }
            },
        };
    }
    match &*st {
        GpuState::Ready(g) => Some(g.clone()),
        _ => None,
    }
}

/// AI upscale with the chosen device; a card that breaks off is remembered
fn ai_run(
    img: &io::Rgba8,
    scale: upscale::Scale,
    denoise: f32,
    device: &str,
    app: &tauri::AppHandle,
    progress: impl Fn(usize, usize) -> bool + Sync,
) -> anyhow::Result<(io::Rgba8, upscale::Report)> {
    let gpu = gpu_for(device, app);
    let r = upscale::upscale_on(img, scale, denoise, gpu.as_deref(), progress)?;
    if let (Some(name), Some(why)) = (&r.1.gpu, &r.1.fallback) {
        gpu_disable(app, name, why);
    }
    Ok(r)
}

#[derive(Serialize)]
struct AiDevice {
    /// "belum" (not tried yet) | "gpu" | "cpu"
    state: &'static str,
    name: String,
    reason: String,
}

/// what the AI runs on (no detection here: that happens on the first AI use)
#[tauri::command]
fn ai_device() -> AiDevice {
    // never wait (this runs on the UI thread) while a card is being opened and tested
    let st = match GPU.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return AiDevice { state: "belum", name: String::new(), reason: String::new() },
    };
    match &*st {
        GpuState::Untested => AiDevice { state: "belum", name: String::new(), reason: String::new() },
        GpuState::Ready(g) => AiDevice { state: "gpu", name: g.name().to_string(), reason: String::new() },
        GpuState::Off(r) => AiDevice { state: "cpu", name: String::new(), reason: eng(r.clone()) },
    }
}

/// forget a failed card: the next AI use tests it again (async: never waits on the UI
/// thread while a card is being tested)
#[tauri::command]
async fn ai_device_retry(app: tauri::AppHandle) {
    if let Some(f) = gpu_file(&app) {
        let _ = std::fs::remove_file(f);
    }
    let mut st = GPU.lock().unwrap_or_else(|e| e.into_inner());
    if !matches!(*st, GpuState::Ready(_)) {
        *st = GpuState::Untested;
    }
}

/// "kartu grafis X" / "prosesor" (+ why the card stopped)
fn ai_where(rep: &upscale::Report) -> String {
    match (&rep.gpu, &rep.fallback) {
        (Some(n), None) => tr!("kartu grafis {n}", "graphics card {n}"),
        (Some(n), Some(why)) => tr!("prosesor (kartu grafis {n} bermasalah: {})", "processor (graphics card {n} failed: {})", eng(why.clone())),
        (None, _) => tr!("prosesor", "processor"),
    }
}

#[derive(Clone, Serialize)]
struct AiProgress {
    done: usize,
    total: usize,
}

#[derive(Serialize)]
struct AiResult {
    path: String,
    w: usize,
    h: usize,
    ms: u128,
    /// where it ran, for the message
    device: String,
    /// taken from the cache (made before with the same image, scale and smoothing)
    cached: bool,
}

/// results kept, newest first (older ones are deleted); a result is a few to 160 MB
const AI_CACHE_KEEP: usize = 24;
const AI_CACHE_MAX_BYTES: u64 = 3 << 30;

/// the app's AI result cache: `{cache}/ai/{key}/{name}_ai4x.png`. Results stay here
/// (no new file next to the original on every click); saving a project copies the
/// result next to the project file (keep_ai_image)
fn ai_cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_cache_dir().map(|d| d.join("ai")).map_err(|e| e.to_string())
}

/// FNV-1a 64: a stable key across builds (DefaultHasher may change between Rust versions)
fn fnv64(parts: &[&[u8]]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for &b in p.iter().chain(&[0xff]) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

/// cache entry of `original` (identified by its full path, size and modified time, so an
/// edited file is upscaled again) at `scale` and `denoise`; the device does not matter
/// (the graphics card gives the same result as the processor)
fn ai_cache_path(cache: &std::path::Path, original: &std::path::Path, scale: upscale::Scale, denoise: f32) -> Result<PathBuf, String> {
    let meta = std::fs::metadata(original).map_err(|e| tr!("Gambar asli tidak bisa dibaca: {}", "Cannot read the original image: {}", e))?;
    let full = std::fs::canonicalize(original).unwrap_or_else(|_| original.to_path_buf());
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    let key = fnv64(&[
        b"realesrgan-compact-1",
        full.to_string_lossy().as_bytes(),
        &meta.len().to_le_bytes(),
        &mtime.to_le_bytes(),
        &[scale.factor() as u8],
        &denoise.to_bits().to_le_bytes(),
    ]);
    let stem: String = original.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "gambar".into()).chars().take(100).collect();
    Ok(cache.join(format!("{key:016x}")).join(format!("{stem}_ai{}x.png", scale.factor())))
}

/// width and height of a PNG written by io::save_rgba8 (from its IHDR)
fn png_size(path: &std::path::Path) -> Option<(usize, usize)> {
    use std::io::Read;
    let mut b = [0u8; 24];
    std::fs::File::open(path).ok()?.read_exact(&mut b).ok()?;
    (b[..8] == *b"\x89PNG\r\n\x1a\n" && b[12..16] == *b"IHDR").then(|| {
        (u32::from_be_bytes(b[16..20].try_into().unwrap()) as usize, u32::from_be_bytes(b[20..24].try_into().unwrap()) as usize)
    })
}

/// keep the newest AI_CACHE_KEEP entries within AI_CACHE_MAX_BYTES; `keep` (the result
/// just made and the open image) is never deleted
fn prune_ai_cache(cache: &std::path::Path, keep: &[&std::path::Path]) {
    let Ok(rd) = std::fs::read_dir(cache) else { return };
    let mut entries: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|e| {
            let dir = e.path();
            let (mut newest, mut bytes) = (std::time::UNIX_EPOCH, 0);
            for f in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                if let Ok(m) = f.metadata() {
                    bytes += m.len();
                    newest = newest.max(m.modified().unwrap_or(std::time::UNIX_EPOCH));
                }
            }
            (newest, bytes, dir)
        })
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0));
    let mut total = 0;
    for (i, (_, bytes, dir)) in entries.iter().enumerate() {
        total += bytes;
        let kept = keep.iter().any(|k| k.starts_with(dir));
        if !kept && (i >= AI_CACHE_KEEP || total > AI_CACHE_MAX_BYTES) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// AI upscale (Real-ESRGAN Compact) of the open image `id` (read from `path`) into the
/// app's cache (the same image, scale and smoothing again: the cached result at once);
/// events "ai-progress"; ai_cancel stops it
#[tauri::command]
async fn ai_upscale(id: u32, path: String, scale: u32, denoise: f32, device: String, app: tauri::AppHandle, state: State<'_>) -> Result<AiResult, String> {
    let scale = match scale {
        2 => upscale::Scale::X2,
        4 => upscale::Scale::X4,
        _ => return Err(tr!("Skala AI harus 2 atau 4", "AI scale must be 2 or 4")),
    };
    if !(denoise.is_finite() && (0.0..=1.0).contains(&denoise)) {
        return Err(tr!("Kehalusan AI harus 0..1", "AI smoothing must be 0..1"));
    }
    if device != "otomatis" && device != "cpu" {
        return Err(tr!("Perangkat AI harus otomatis atau cpu", "AI device must be auto or cpu"));
    }
    let src = state.lock().unwrap().src.clone().ok_or_else(|| tr!("Buka gambar dulu", "Open an image first"))?;
    if src.id != id {
        return Err(tr!("Gambar sudah berganti; coba lagi", "The image changed; try again"));
    }
    let cache = ai_cache_dir(&app)?;
    let out = ai_cache_path(&cache, std::path::Path::new(&path), scale, denoise)?;
    if let Some((w, h)) = png_size(&out) {
        // used again: now the newest entry
        let _ = std::fs::File::options().write(true).open(&out).and_then(|f| f.set_modified(std::time::SystemTime::now()));
        return Ok(AiResult { path: out.to_string_lossy().into_owned(), w, h, ms: 0, device: String::new(), cached: true });
    }
    if AI_RUNNING.swap(true, Ordering::AcqRel) {
        return Err(tr!("Perbesar AI masih berjalan", "AI Upscale is still running"));
    }
    AI_CANCEL.store(false, Ordering::Release);
    let t0 = Instant::now();
    let r = tauri::async_runtime::spawn_blocking(move || -> Result<AiResult, String> {
        let _ = app.emit("ai-progress", AiProgress { done: 0, total: 0 });
        let (up, rep) = ai_run(&src.img, scale, denoise, &device, &app, |done, total| {
            let _ = app.emit("ai-progress", AiProgress { done, total });
            !AI_CANCEL.load(Ordering::Acquire)
        })
        .map_err(|e| if AI_CANCEL.load(Ordering::Acquire) { "dibatalkan".to_string() } else { eng(format!("{e:#}")) })?;
        if AI_CANCEL.load(Ordering::Acquire) {
            return Err("dibatalkan".into());
        }
        let dir = out.parent().unwrap_or(&cache);
        std::fs::create_dir_all(dir)
            .map_err(|e| e.to_string())
            .and_then(|_| io::save_rgba8(&up, &out).map_err(|e| format!("{e:#}")))
            .map_err(|e| tr!("Gagal menyimpan hasil AI: {}", "Cannot save the AI result: {}", e))?;
        prune_ai_cache(&cache, &[&out, std::path::Path::new(&path)]);
        Ok(AiResult { path: out.to_string_lossy().into_owned(), w: up.w, h: up.h, ms: t0.elapsed().as_millis(), device: ai_where(&rep), cached: false })
    })
    .await;
    AI_RUNNING.store(false, Ordering::Release);
    r.map_err(|_| tr!("Perbesar AI gagal (kesalahan internal)", "AI Upscale failed (internal error)"))?
}

/// an image from the AI cache becomes a real file when its project is saved: copied next
/// to the project file (`{name}_ai4x.png`; the same picture there already is reused,
/// another file of that name gets `-2`...). Any other image path comes back as it is.
#[tauri::command]
fn keep_ai_image(path: String, project: String, app: tauri::AppHandle) -> Result<String, String> {
    let src = PathBuf::from(&path);
    let cache = ai_cache_dir(&app)?;
    let (Ok(full), Ok(cache)) = (std::fs::canonicalize(&src), std::fs::canonicalize(&cache)) else { return Ok(path) };
    if !full.starts_with(&cache) {
        return Ok(path);
    }
    let dir = PathBuf::from(&project).parent().map(|d| d.to_path_buf()).ok_or_else(|| tr!("Folder proyek tidak valid", "Invalid project folder"))?;
    let stem = full.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "gambar_ai".into());
    let same = |p: &std::path::Path| -> bool {
        match (std::fs::metadata(p), std::fs::metadata(&full)) {
            (Ok(a), Ok(b)) if a.len() == b.len() => std::fs::read(p).ok() == std::fs::read(&full).ok(),
            _ => false,
        }
    };
    let mut out = dir.join(format!("{stem}.png"));
    let mut k = 2;
    while out.exists() {
        if same(&out) {
            return Ok(out.to_string_lossy().into_owned());
        }
        out = dir.join(format!("{stem}-{k}.png"));
        k += 1;
    }
    let mut part = out.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    std::fs::copy(&full, &part)
        .and_then(|_| std::fs::rename(&part, &out))
        .map_err(|e| {
            let _ = std::fs::remove_file(&part);
            tr!("Gagal menyalin hasil AI ke folder proyek: {}", "Cannot copy the AI result to the project folder: {}", e)
        })?;
    Ok(out.to_string_lossy().into_owned())
}

#[tauri::command]
fn ai_cancel() {
    AI_CANCEL.store(true, Ordering::Release);
}


// ---------------------------------------------------------------- sablon (screen printing)
use studiozea_engine::sablon;

/// largest print separated at once (all films and the simulated print stay in memory).
/// The separation peaks at about 48 bytes per pixel, so 125 Mpx is about 6 GB: e.g.
/// 40 x 53 cm at 600 dpi (118 Mpx) or 60 x 80 cm at 400 dpi (117 Mpx)
const MAX_SEP_PX: usize = 125_000_000;
/// one separation at a time
static SEP_LOCK: Mutex<()> = Mutex::new(());

/// the last separation: films, simulated print and the design at print size
struct SepJob {
    gen: u32,
    dpi: f64,
    plan: Plan,
    source_overview: Arc<(Vec<u8>, usize, usize)>,
    films: Vec<sablon::Film>,
    preview: io::Rgba8,
    preview_overview: (Vec<u8>, usize, usize),
    lpi: f64,
    angle: f64,
}

#[derive(Serialize)]
struct FilmInfo {
    role: sablon::Role,
    rgb: [u8; 3],
    opacity: f32,
    /// share of the print area that this film prints (0..1)
    coverage: f32,
    /// index into `SepResult::inks` (None = underbase)
    src: Option<usize>,
    /// halftone screen angle (simulated process)
    angle: Option<f64>,
    /// interlock: dots on the inverted screen
    inverted: bool,
}

#[derive(Serialize)]
struct SepResult {
    gen: u32,
    w: usize,
    h: usize,
    width_cm: f64,
    height_cm: f64,
    dpi: f64,
    films: Vec<FilmInfo>,
    /// the ink set in print order, dropped inks included
    inks: Vec<sablon::Ink>,
    /// how many of `inks` print before the underbase
    base_after: usize,
    /// colour difference design vs simulated print (about Delta E)
    delta_e: f32,
    ms: u128,
    bg: Option<[u8; 3]>,
    bg_used: Option<(f32, bool)>,
    src_box: [usize; 4],
    effective_dpi: Option<f64>,
    text_words: Option<usize>,
}

/// separate the open image into sablon films (Design settings `job` + separation `sep`)
#[tauri::command]
async fn sep_process(job: Job, sep: sablon::SepSettings, state: State<'_>) -> Result<SepResult, String> {
    check_job(&job)?;
    sep.check().map_err(eng)?;
    let gen = GEN.fetch_add(1, Ordering::AcqRel) + 1;
    let src = {
        let mut st = state.lock().unwrap();
        // the DTF render is not needed in this mode
        if let Some(j) = st.job.take() {
            j.cancel.store(true, Ordering::Release);
        }
        st.src.clone().ok_or_else(|| tr!("Belum ada gambar", "No image yet"))?
    };
    let t0 = Instant::now();
    let src2 = src.clone();
    let r = tauri::async_runtime::spawn_blocking(move || -> Result<Option<(SepJob, Vec<FilmInfo>, Vec<sablon::Ink>, usize, f32, Arc<Prepared>)>, String> {
        let Some((prep, source_overview)) = prepared_for(&src2, &job, gen) else { return Ok(None) };
        let (w, h) = studiozea_engine::print_size(&prep, &job);
        let dpi = job.settings.dpi;
        if w * h > MAX_SEP_PX {
            return Err(tr!(
                "Ukuran cetak terlalu besar untuk separasi: {:.0}×{:.0} cm. Kecilkan lebar atau DPI.",
                "Print too large to separate: {:.0}×{:.0} cm. Lower the width or DPI.",
                w as f64 / dpi * 2.54,
                h as f64 / dpi * 2.54
            ));
        }
        let _one = SEP_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if GEN.load(Ordering::Acquire) != gen {
            return Ok(None);
        }
        let plan = Plan::from_arc(prep.clone(), w, h, job.settings.clone());
        let (rgb, alpha) = plan.prepared(0, 0, w, h);
        let full = Prepared { w, h, pre_w: prep.pre_w, src_box: prep.src_box, rgb, alpha, bg: prep.bg, bg_used: prep.bg_used, solid: plan.solid(0, 0, w, h) };
        let r = sablon::separate(&full, &plan.settings, &sep);
        drop(full);
        let preview = io::Rgba8 { w, h, px: r.preview, has_alpha: true };
        let f = w.max(h).div_ceil(OVERVIEW_MAX).max(1);
        let (ow, oh) = (w.div_ceil(f), h.div_ceil(f));
        let preview_overview = (box_rgba8(&preview, 0, 0, ow, oh, f), ow, oh);
        let infos = r
            .films
            .iter()
            .map(|fm| FilmInfo { role: fm.role, rgb: fm.ink.rgb, opacity: fm.ink.opacity, coverage: fm.bits.coverage(), src: fm.src, angle: fm.angle, inverted: fm.inverted })
            .collect();
        let job = SepJob { gen, dpi, plan, source_overview, films: r.films, preview, preview_overview, lpi: sep.lpi, angle: sep.angle };
        Ok(Some((job, infos, r.inks, r.base_after, r.delta_e, prep)))
    })
    .await
    .map_err(|e| e.to_string())??;
    let Some((sj, films, inks, base_after, delta_e, prep)) = r else {
        return Err(tr!("Dibatalkan (pengaturan berubah)", "Cancelled (settings changed)"));
    };
    let (w, h, dpi) = (sj.plan.w, sj.plan.h, sj.dpi);
    {
        let mut st = state.lock().unwrap();
        if !st.src.as_ref().is_some_and(|x| Arc::ptr_eq(x, &src)) {
            return Err(tr!("Gambar sudah diganti", "The image was replaced"));
        }
        if st.sep.as_ref().is_some_and(|j| j.gen > gen) {
            return Err(tr!("Dibatalkan (pengaturan berubah)", "Cancelled (settings changed)"));
        }
        st.sep = Some(Arc::new(sj));
    }
    let width_cm = w as f64 / dpi * 2.54;
    Ok(SepResult {
        gen,
        w,
        h,
        width_cm,
        height_cm: h as f64 / dpi * 2.54,
        dpi,
        films,
        inks,
        base_after,
        delta_e,
        ms: t0.elapsed().as_millis(),
        bg: prep.bg.map(|b| b.map(|v| (v * 255.0).round() as u8)),
        bg_used: prep.bg_used,
        src_box: prep.src_box,
        effective_dpi: Some(prep.w as f64 / (width_cm / 2.54)),
        text_words: src.text.get().and_then(|t| t.as_ref()).map(|t| t.words),
    })
}

fn sep_job(state: &State<'_>) -> Result<Arc<SepJob>, String> {
    state.lock().unwrap().sep.clone().ok_or_else(|| tr!("Belum diproses", "Not processed yet"))
}

/// film `i` of a separation as an RGBA "film positive": black where the ink prints,
/// white elsewhere, box-averaged f x f
fn box_film(bits: &sablon::Bits, x0: usize, y0: usize, tw: usize, th: usize, f: usize) -> Vec<u8> {
    let mut out = vec![255u8; tw * th * 4];
    out.chunks_mut(tw * 4).enumerate().for_each(|(ty, row)| {
        for tx in 0..tw {
            let (mut n, mut k) = (0u32, 0u32);
            for y in y0 + ty * f..(y0 + (ty + 1) * f).min(bits.h) {
                for x in x0 + tx * f..(x0 + (tx + 1) * f).min(bits.w) {
                    n += 1;
                    k += bits.get(y * bits.w + x) as u32;
                }
            }
            let v = if n == 0 { 255 } else { 255 - ((k * 255 + n / 2) / n) as u8 };
            row[tx * 4..tx * 4 + 3].fill(v);
        }
    });
    out
}

/// overview of a separation: kind = sep (simulated print) | source | film0, film1, ...
#[tauri::command]
fn sep_overview(kind: String, state: State<'_>) -> Result<tauri::ipc::Response, String> {
    let Some(sj) = state.lock().unwrap().sep.clone() else { return Ok(empty()) };
    match kind.as_str() {
        "sep" => {
            let (ref px, w, h) = sj.preview_overview;
            Ok(raw(w, h, sj.gen, true, px))
        }
        "source" => {
            let (ref px, w, h) = *sj.source_overview;
            Ok(raw(w, h, sj.gen, true, px))
        }
        k => {
            let i: usize = k.strip_prefix("film").and_then(|v| v.parse().ok()).ok_or("kind")?;
            let film = sj.films.get(i).ok_or("film")?;
            let (w, h) = (sj.plan.w, sj.plan.h);
            let f = w.max(h).div_ceil(OVERVIEW_MAX).max(1);
            let (ow, oh) = (w.div_ceil(f), h.div_ceil(f));
            Ok(raw(ow, oh, sj.gen, true, &box_film(&film.bits, 0, 0, ow, oh, f)))
        }
    }
}

/// tile of a separation (see `tile`): sep | source | filmN
#[tauri::command]
async fn sep_tile(kind: String, level: u32, tx: usize, ty: usize, state: State<'_>) -> Result<tauri::ipc::Response, String> {
    let level = level.min(8);
    let f = 1usize << level;
    let sj = sep_job(&state)?;
    let (pw, ph) = (sj.plan.w, sj.plan.h);
    let (x0, y0) = (tx * TILE * f, ty * TILE * f);
    if x0 >= pw || y0 >= ph {
        return Ok(empty());
    }
    let tw = (pw - x0).div_ceil(f).min(TILE);
    let th = (ph - y0).div_ceil(f).min(TILE);
    let (x1, y1) = ((x0 + tw * f).min(pw), (y0 + th * f).min(ph));
    tauri::async_runtime::spawn_blocking(move || match kind.as_str() {
        "sep" => raw(tw, th, sj.gen, true, &box_rgba8(&sj.preview, x0, y0, tw, th, f)),
        "source" => {
            let _one = (level > 0).then(|| SOURCE_TILE_LOCK.lock().unwrap_or_else(|e| e.into_inner()));
            let (c, a) = sj.plan.prepared(x0, y0, x1, y1);
            raw(tw, th, sj.gen, true, &box_tone(&c, &a, x1 - x0, y1 - y0, 0, 0, tw, th, f))
        }
        k => match k.strip_prefix("film").and_then(|v| v.parse::<usize>().ok()).and_then(|i| sj.films.get(i)) {
            Some(film) => raw(tw, th, sj.gen, true, &box_film(&film.bits, x0, y0, tw, th, f)),
            None => empty(),
        },
    })
    .await
    .map_err(|e| e.to_string())
}

/// file name part of a film label: letters, digits and - only
fn safe_name(s: &str) -> String {
    let t: String = s.chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    t.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-").chars().take(40).collect()
}

/// write the separation: `path` = the simulated print (PNG), next to it one film per
/// ink (`{stem}_{nn}_{name}.png`, black on white with registration marks in a
/// `margin_mm` border) and the print order (`{stem}_urutan-cetak.txt`). `names` = the
/// ink names in the user's language, film order.
#[tauri::command]
async fn sep_export(path: String, names: Vec<String>, margin_mm: f64, state: State<'_>) -> Result<Vec<String>, String> {
    let sj = sep_job(&state)?;
    if !(margin_mm.is_finite() && (0.0..=50.0).contains(&margin_mm)) {
        return Err(tr!("Margin film harus 0 sampai 50 mm", "Film margin must be 0–50 mm"));
    }
    tauri::async_runtime::spawn_blocking(move || -> Result<Vec<String>, String> {
        let out = PathBuf::from(if path.to_lowercase().ends_with(".png") { path } else { format!("{path}.png") });
        let stem = out.with_extension("");
        let mut written = Vec::new();
        let (w, h) = (sj.plan.w, sj.plan.h);
        io::write_png_rows(w, h, sj.dpi, &out, |y0, y1| Ok(sj.preview.px[y0 * w * 4..y1 * w * 4].to_vec()))
            .map_err(|e| tr!("Gagal menyimpan: {}", "Cannot save: {}", e))?;
        written.push(out.to_string_lossy().into_owned());
        let mut order = format!(
            "{}\r\n{} × {} cm @ {:.0} DPI · film {:.0} LPI {:.1}°\r\n\r\n",
            tr!("Urutan cetak (sablon)", "Print order (screen printing)"),
            fmt_cm(w as f64 / sj.dpi * 2.54),
            fmt_cm(h as f64 / sj.dpi * 2.54),
            sj.dpi,
            sj.lpi,
            sj.angle
        );
        for (i, film) in sj.films.iter().enumerate() {
            let name = names.get(i).cloned().unwrap_or_else(|| format!("{:?}", film.role));
            let hex = format!("{:02X}{:02X}{:02X}", film.ink.rgb[0], film.ink.rgb[1], film.ink.rgb[2]);
            let p = PathBuf::from(format!("{}_{:02}_{}.png", stem.display(), i + 1, safe_name(&name)));
            let label: String = format!("{} {} #{hex}", i + 1, name).chars().filter(|c| c.is_ascii()).collect();
            sablon::save_film(film, sj.dpi, margin_mm, &label, &p).map_err(|e| tr!("Gagal menyimpan film: {}", "Cannot save the film: {}", e))?;
            let screen = match film.angle {
                Some(a) if film.inverted => format!(" · {a:.1}° {}", tr!("titik dibalik", "inverted dots")),
                Some(a) => format!(" · {a:.1}°"),
                None => String::new(),
            };
            order.push_str(&format!("{:2}. {name} #{hex} ({:.0} %){screen}\r\n", i + 1, film.bits.coverage() * 100.0));
            written.push(p.to_string_lossy().into_owned());
        }
        let txt = PathBuf::from(format!("{}_urutan-cetak.txt", stem.display()));
        std::fs::write(&txt, order).map_err(|e| e.to_string())?;
        written.push(txt.to_string_lossy().into_owned());
        Ok(written)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn fmt_cm(v: f64) -> String {
    let s = format!("{v:.1}");
    if en() {
        s
    } else {
        s.replace('.', ",")
    }
}

// ---------------------------------------------------------------- gang sheet
use studiozea_engine::gang;

static GANG_RUNNING: AtomicBool = AtomicBool::new(false);
static GANG_CANCEL: AtomicBool = AtomicBool::new(false);
/// thumbnails of added designs (path -> w, h, rgba)
fn gang_thumbs() -> &'static Mutex<std::collections::HashMap<String, (usize, usize, Vec<u8>)>> {
    static C: std::sync::OnceLock<Mutex<std::collections::HashMap<String, (usize, usize, Vec<u8>)>>> = std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}
const GANG_THUMB: usize = 384;
/// all designs of one gang sheet together in memory (8-bit RGBA)
const GANG_MAX_BYTES: usize = 3 << 30;

#[derive(Serialize)]
struct GangInfo {
    path: String,
    name: String,
    w: usize,
    h: usize,
    dpi: Option<f64>,
    /// the raw image this print file was made from (None = added as it is)
    from: Option<String>,
}

/// add a design to the gang sheet and remember its thumbnail. A print file (transparent
/// PNG, alpha only 0 / 255) is used as it is; any other image is first made print-ready
/// with the Design tab settings (`job`; its print width) into the app cache, like one
/// image of a batch export
#[tauri::command]
async fn gang_add(path: String, job: Option<Job>, app: tauri::AppHandle) -> Result<GangInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let src = PathBuf::from(&path);
        let name = src.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let (pb, d, from) = match gang::load_design(&src) {
            Ok(d) => (src, d, None),
            Err(e) => {
                // only now the Design tab settings matter (a missing texture must not stop
                // print files from being added)
                let Some(job) = job else { return Err(eng(format!("{e:#}"))) };
                if job.width_cm.is_none() {
                    return Err(tr!("gambar mentah: isi dulu lebar cetak di tab Desain (Ukuran)", "raw image: enter a print width in the Design tab (Size) first"));
                }
                let job = &any_image_job(job)?;
                let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("gang");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let none = AtomicBool::new(false);
                let out = batch_one(&src, &dir, job, None, 0.0, &format!("_{:.0}dpi", job.settings.dpi), None, &app, &none, |_, _| {})?;
                let d = gang::load_design(&out).map_err(|e| eng(format!("{e:#}")))?;
                (out, d, Some(path.clone()))
            }
        };
        let dpi = gang::png_dpi(&pb);
        let f = d.w.max(d.h).div_ceil(GANG_THUMB).max(1);
        let img = io::Rgba8 { w: d.w, h: d.h, px: d.px, has_alpha: true };
        let (tw, th) = (d.w.div_ceil(f), d.h.div_ceil(f));
        let thumb = box_rgba8(&img, 0, 0, tw, th, f);
        let path = pb.to_string_lossy().into_owned();
        gang_thumbs().lock().unwrap().insert(path.clone(), (tw, th, thumb));
        Ok(GangInfo { name, path, w: img.w, h: img.h, dpi, from })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn gang_thumb(path: String) -> Result<tauri::ipc::Response, String> {
    let c = gang_thumbs().lock().unwrap();
    let (w, h, px) = c.get(&path).ok_or_else(|| tr!("belum ditambahkan", "not added yet"))?;
    Ok(raw(*w, *h, 0, true, px))
}

/// drop the thumbnail of a design removed from the gang sheet
#[tauri::command]
fn gang_forget(path: String) {
    gang_thumbs().lock().unwrap_or_else(|e| e.into_inner()).remove(&path);
}

#[tauri::command]
fn gang_layout(pieces: Vec<gang::Piece>, opts: gang::Opts, pins: Option<Vec<gang::Pin>>) -> Result<Vec<gang::Sheet>, String> {
    gang::pack_with(&pieces, &opts, &pins.unwrap_or_default()).map_err(eng)
}

#[derive(Clone, Serialize, serde::Deserialize)]
struct GangItem {
    path: String,
    count: usize,
}

#[derive(Clone, Serialize)]
struct GangProgress {
    sheet: usize,
    sheets: usize,
    frac: f64,
}

#[derive(Clone, Serialize)]
struct GangDone {
    paths: Vec<String>,
    error: Option<String>,
    cancelled: bool,
}

/// the chosen file for one sheet; with more sheets `{stem}_1.png`, `{stem}_2.png`… next
/// to it, never replacing an existing file (only the chosen name was confirmed)
fn sheet_path(out: &std::path::Path, i: usize, n: usize) -> PathBuf {
    if n == 1 {
        return out.to_path_buf();
    }
    let stem = out.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "gangsheet".into());
    let dir = out.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    free_path(&dir, &format!("{stem}_{}", i + 1))
}

fn same_file(a: &std::path::Path, b: &std::path::Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// writes the sheets; `paths` = files written so far (also when it fails later)
fn gang_run(items: &[GangItem], opts: &gang::Opts, pins: &[gang::Pin], dpi: f64, out: &std::path::Path, app: &tauri::AppHandle, paths: &mut Vec<String>) -> Result<(), String> {
    let mut designs = Vec::new();
    let mut bytes = 0usize;
    for it in items {
        let pb = PathBuf::from(&it.path);
        let name = pb.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let d = gang::load_design(&pb).map_err(|e| eng(format!("{name}: {e:#}")))?;
        if let Some(f) = gang::png_dpi(&pb) {
            if (f - dpi).abs() > dpi * 0.005 {
                return Err(tr!("{name}: {f:.0} DPI, lembar {dpi:.0} DPI (export ulang dengan DPI yang sama)", "{name}: {f:.0} DPI, sheet {dpi:.0} DPI (export again at the same DPI)"));
            }
        }
        bytes += d.px.len();
        if bytes > GANG_MAX_BYTES {
            return Err(tr!("desain terlalu banyak/besar untuk satu gang sheet (maksimal 3 GB gambar); bagi jadi beberapa", "too many/large designs for one gang sheet (max. 3 GB of images); split it up"));
        }
        if GANG_CANCEL.load(Ordering::Acquire) {
            return Err("dibatalkan".into());
        }
        designs.push(d);
    }
    let pieces: Vec<gang::Piece> = designs.iter().zip(items).map(|(d, it)| gang::Piece { w: d.w, h: d.h, count: it.count }).collect();
    let sheets = gang::pack_with(&pieces, opts, pins).map_err(eng)?;
    if sheets.iter().any(|sh| sh.h as f64 > 5000.0 / 2.54 * dpi) {
        return Err(tr!("lembar lebih panjang dari 50 m: pakai panjang maksimal", "sheet longer than 50 m: set a max. length"));
    }
    let refs: Vec<&gang::Design> = designs.iter().collect();
    for (i, sh) in sheets.iter().enumerate() {
        let p = sheet_path(out, i, sheets.len());
        if items.iter().any(|it| same_file(&p, std::path::Path::new(&it.path))) {
            return Err(tr!("{} adalah salah satu file desain; pilih nama lain", "{} is one of the design files; choose another name", p.display()));
        }
        let mut last = 0.0;
        gang::save_sheet(sh, &refs, dpi, &p, |rows| {
            let f = rows as f64 / sh.h as f64;
            if f - last >= 0.02 || rows >= sh.h {
                last = f;
                let _ = app.emit("gang-progress", GangProgress { sheet: i, sheets: sheets.len(), frac: f });
            }
            !GANG_CANCEL.load(Ordering::Acquire)
        })
        .map_err(|e| eng(format!("{e:#}")))?;
        paths.push(p.to_string_lossy().into_owned());
    }
    Ok(())
}

/// write the gang sheet(s) in a background thread ("gang-progress", "gang-done")
#[tauri::command]
fn gang_export(items: Vec<GangItem>, opts: gang::Opts, pins: Option<Vec<gang::Pin>>, dpi: f64, path: String, app: tauri::AppHandle) -> Result<(), String> {
    let pins = pins.unwrap_or_default();
    if !(dpi.is_finite() && (100.0..=720.0).contains(&dpi)) {
        return Err(tr!("DPI lembar tidak valid", "Invalid sheet DPI"));
    }
    if items.iter().all(|i| i.count == 0) {
        return Err(tr!("Belum ada desain", "No designs yet"));
    }
    // a 300 cm wide or 50 m long sheet is surely a typo
    if opts.width as f64 > 300.0 / 2.54 * dpi || opts.max_len.is_some_and(|l| l as f64 > 5000.0 / 2.54 * dpi) {
        return Err(tr!("Ukuran lembar tidak masuk akal", "Sheet size makes no sense"));
    }
    if GANG_RUNNING.swap(true, Ordering::AcqRel) {
        return Err(tr!("Gang sheet masih dibuat", "The gang sheet is still being made"));
    }
    GANG_CANCEL.store(false, Ordering::Release);
    std::thread::spawn(move || {
        let out = PathBuf::from(if path.to_lowercase().ends_with(".png") { path } else { format!("{path}.png") });
        let mut paths = Vec::new();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| gang_run(&items, &opts, &pins, dpi, &out, &app, &mut paths)))
            .unwrap_or_else(|_| Err(tr!("kesalahan internal", "internal error")));
        let cancelled = GANG_CANCEL.load(Ordering::Acquire);
        GANG_RUNNING.store(false, Ordering::Release);
        let done = match r {
            Ok(()) => GangDone { paths, error: None, cancelled: false },
            Err(e) => GangDone { paths, error: (!cancelled).then_some(e), cancelled },
        };
        let _ = app.emit("gang-done", done);
    });
    Ok(())
}

#[tauri::command]
fn gang_cancel() {
    GANG_CANCEL.store(true, Ordering::Release);
}

/// a file in the app cache for "add the current design" (exported there first)
#[tauri::command]
fn gang_temp_path(name: String, app: tauri::AppHandle) -> Result<String, String> {
    let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("gang");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c)).take(60).collect();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    Ok(dir.join(format!("{}_{stamp}.png", if safe.is_empty() { "desain".into() } else { safe })).to_string_lossy().into_owned())
}

/// print calibration sheet (220 x 300 mm) for a black or white shirt
#[tauri::command]
async fn test_sheet(kaos: String, dpi: f64, path: String) -> Result<Exported, String> {
    let shirt = if kaos == "putih" {
        Shirt::Putih
    } else {
        Shirt::Hitam
    };
    let pb = PathBuf::from(&path);
    let pb2 = pb.clone();
    let (w, h, checks) = tauri::async_runtime::spawn_blocking(move || {
        let (w, h, px) = studiozea_engine::testsheet::test_sheet(shirt, dpi);
        io::save_png(&px, w, h, dpi, &pb2).map(|_| (w, h, io::checks(&px, w, h, dpi)))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| tr!("Gagal membuat lembar uji: {e}", "Cannot make the test sheet: {e}"))?;
    Ok(Exported {
        bytes: std::fs::metadata(&pb).map(|m| m.len()).unwrap_or(0),
        path,
        w,
        h,
        checks,
    })
}

#[derive(Serialize)]
struct TextureInfo {
    /// value for the layer path (lib:<file> or the file path)
    path: String,
    name: String,
    w: u32,
    h: u32,
    /// small grey preview (worn = black) as a PNG data URL
    thumb: String,
    /// in the user's own texture library (can be removed)
    own: bool,
}

fn texture_info(path: String, name: String) -> Option<TextureInfo> {
    let (w, h) = studiozea_engine::distress::texture_size(&path).ok()?;
    let (tw, th, px) = studiozea_engine::distress::thumbnail(&path, 160)?;
    let png = io::encode_png(&px, tw, th, 72.0).ok()?;
    use base64::Engine as _;
    Some(TextureInfo {
        thumb: format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)),
        path,
        name,
        w,
        h,
        own: false,
    })
}

// ---------------------------------------------------------------- own texture library
const TEX_EXT: [&str; 7] = ["png", "jpg", "jpeg", "tif", "tiff", "webp", "bmp"];
/// largest texture file accepted for the library
const TEX_MAX_BYTES: u64 = 300 << 20;

/// the user's texture library: <app data>/tekstur (files are copied there, so the
/// originals can be moved or deleted)
fn own_tex_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let d = app.path().app_data_dir().map_err(|e| e.to_string())?.join("tekstur");
    std::fs::create_dir_all(&d).map_err(|e| tr!("Folder tekstur tidak bisa dibuat: {e}", "Cannot create the texture folder: {e}"))?;
    Ok(d)
}

fn is_texture_file(p: &std::path::Path) -> bool {
    p.is_file() && p.extension().is_some_and(|e| TEX_EXT.contains(&e.to_string_lossy().to_lowercase().as_str()))
}

/// thumbnail of an own texture, cached next to it in .thumbs (keyed by size + mtime)
fn own_texture_info(p: &std::path::Path) -> Option<TextureInfo> {
    let meta = std::fs::metadata(p).ok()?;
    let stamp = format!(
        "{}-{}",
        meta.len(),
        meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
    );
    let file = p.file_name()?.to_string_lossy().into_owned();
    let cache = p.parent()?.join(".thumbs").join(format!("{file}.json"));
    let path = p.to_string_lossy().into_owned();
    let name = studiozea_engine::library::title(&file);
    if let Ok(text) = std::fs::read_to_string(&cache) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if v["stamp"] == stamp.as_str() {
                return Some(TextureInfo {
                    path,
                    name,
                    w: v["w"].as_u64()? as u32,
                    h: v["h"].as_u64()? as u32,
                    thumb: v["thumb"].as_str()?.to_string(),
                    own: true,
                });
            }
        }
    }
    let mut info = texture_info(path, name)?;
    info.own = true;
    let _ = std::fs::create_dir_all(cache.parent()?);
    let _ = std::fs::write(&cache, serde_json::json!({ "stamp": stamp, "w": info.w, "h": info.h, "thumb": info.thumb }).to_string());
    Some(info)
}

fn own_textures(app: &tauri::AppHandle) -> Vec<TextureInfo> {
    use rayon::prelude::*;
    let Ok(dir) = own_tex_dir(app) else { return Vec::new() };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| is_texture_file(p)).collect())
        .unwrap_or_default();
    files.sort_by_key(|p| p.file_name().map(|s| s.to_string_lossy().to_lowercase()));
    files.par_iter().filter_map(|p| own_texture_info(p)).collect()
}

#[derive(Serialize)]
struct AddedTextures {
    added: Vec<TextureInfo>,
    failed: Vec<String>,
}

/// copy texture files into the own library (several at once); the same file (name and
/// size) is not copied twice
#[tauri::command]
async fn add_textures(paths: Vec<String>, app: tauri::AppHandle) -> Result<AddedTextures, String> {
    let dir = own_tex_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut added = Vec::new();
        let mut failed = Vec::new();
        for p in paths {
            let src = PathBuf::from(&p);
            let file = src.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let res = (|| -> Result<TextureInfo, String> {
                if !is_texture_file(&src) {
                    return Err(tr!("bukan file gambar (PNG/JPG/TIFF/WebP/BMP)", "not an image file (PNG/JPG/TIFF/WebP/BMP)"));
                }
                let len = std::fs::metadata(&src).map_err(|e| e.to_string())?.len();
                if len > TEX_MAX_BYTES {
                    return Err(tr!("terlalu besar ({} MB, maksimal {} MB)", "too large ({} MB, max. {} MB)", len >> 20, TEX_MAX_BYTES >> 20));
                }
                studiozea_engine::distress::texture_size(&src.to_string_lossy())?;
                let (stem, ext) = match file.rsplit_once('.') {
                    Some((s, e)) => (s.to_string(), e.to_lowercase()),
                    None => (file.clone(), "png".into()),
                };
                // same name and size already in the library: use it
                let mut dst = dir.join(format!("{stem}.{ext}"));
                let mut k = 2;
                while dst.exists() {
                    if std::fs::metadata(&dst).map(|m| m.len()).ok() == Some(len) {
                        return own_texture_info(&dst).ok_or_else(|| tr!("tidak bisa dibaca", "cannot be read"));
                    }
                    dst = dir.join(format!("{stem}-{k}.{ext}"));
                    k += 1;
                }
                std::fs::copy(&src, &dst).map_err(|e| tr!("tidak bisa disalin: {e}", "cannot be copied: {e}"))?;
                own_texture_info(&dst).ok_or_else(|| {
                    let _ = std::fs::remove_file(&dst);
                    "tidak bisa dibaca".to_string()
                })
            })();
            match res {
                Ok(i) => added.push(i),
                Err(e) => failed.push(format!("{file}: {e}")),
            }
        }
        AddedTextures { added, failed }
    })
    .await
    .map_err(|e| e.to_string())
}

/// remove a texture from the own library (only files inside it)
#[tauri::command]
fn remove_texture(path: String, app: tauri::AppHandle) -> Result<(), String> {
    let dir = std::fs::canonicalize(own_tex_dir(&app)?).map_err(|e| e.to_string())?;
    let p = std::fs::canonicalize(&path).map_err(|_| tr!("Tekstur tidak ditemukan", "Texture not found"))?;
    if p.parent() != Some(dir.as_path()) || !is_texture_file(&p) {
        return Err(tr!("Hanya tekstur di pustaka sendiri yang bisa dihapus", "Only textures in your own library can be removed"));
    }
    std::fs::remove_file(&p).map_err(|e| tr!("Tidak bisa dihapus: {e}", "Cannot remove: {e}"))?;
    if let Some(f) = p.file_name() {
        let _ = std::fs::remove_file(dir.join(".thumbs").join(format!("{}.json", f.to_string_lossy())));
    }
    Ok(())
}

/// built-in vintage textures (repository folder textures/), then the own library
#[tauri::command]
async fn textures(app: tauri::AppHandle) -> Result<Vec<TextureInfo>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        use rayon::prelude::*;
        // decoding large textures for the thumbnails: in parallel, order kept
        let mut all: Vec<TextureInfo> = studiozea_engine::library::names()
            .into_par_iter()
            .filter_map(|n| {
                texture_info(
                    format!("{}{n}", studiozea_engine::distress::LIB_PREFIX),
                    studiozea_engine::library::title(n),
                )
            })
            .collect();
        all.extend(own_textures(&app));
        all
    })
    .await
    .map_err(|e| e.to_string())
}

/// preview + size of a texture file the user picked
#[tauri::command]
async fn texture_file(path: String) -> Result<TextureInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let name = PathBuf::from(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        studiozea_engine::distress::texture_size(&path).map_err(|e| tr!("Tekstur tidak bisa dibuka: {}", "Cannot open the texture: {}", eng(e)))?;
        texture_info(path, name).ok_or_else(|| tr!("Tekstur tidak bisa dibaca", "Cannot read the texture"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize, Clone)]
struct Swatch {
    shape: &'static str,
    /// PNG data URL: white ink (transparent elsewhere), 10-90 % tone ramp, ~6 cells wide
    thumb: String,
}

fn swatch_cache() -> &'static Mutex<Vec<(u64, Vec<Swatch>)>> {
    static C: std::sync::OnceLock<Mutex<Vec<(u64, Vec<Swatch>)>>> = std::sync::OnceLock::new();
    C.get_or_init(|| Mutex::new(Vec::new()))
}

/// previews of every halftone pattern at the given screen angle (None = automatic),
/// rendered by the real engine; the pattern size is fixed so the shape stays readable
#[tauri::command]
async fn pattern_swatches(angle: Option<f64>) -> Result<Vec<Swatch>, String> {
    let key = angle.filter(|a| a.is_finite()).map_or(u64::MAX, f64::to_bits);
    if let Some((_, v)) = swatch_cache().lock().unwrap().iter().find(|(k, _)| *k == key) {
        return Ok(v.clone());
    }
    let v = tauri::async_runtime::spawn_blocking(move || {
        use rayon::prelude::*;
        use studiozea_engine::{render, Settings, Shape};
        let (w, h) = (168usize, 84usize);
        let mut rgb = vec![0f32; w * h * 3];
        for (p, px) in rgb.chunks_exact_mut(3).enumerate() {
            let t = 0.1 + 0.8 * (p % w) as f32 / (w - 1) as f32;
            px.fill(t);
        }
        let alpha = vec![1f32; w * h];
        Shape::ALL
            .par_iter()
            .map(|&shape| {
                let s = Settings {
                    shirt: Shirt::Hitam,
                    dpi: 300.0,
                    lpi: 300.0 / 28.0,
                    shape,
                    angle,
                    ..Settings::default()
                };
                let o = render(&rgb, &alpha, w, h, &s);
                let png = io::encode_png(&o, w, h, 72.0).map_err(|e| e.to_string())?;
                use base64::Engine as _;
                Ok(Swatch {
                    shape: shape.name(),
                    thumb: format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)),
                })
            })
            .collect::<Result<Vec<_>, String>>()
    })
    .await
    .map_err(|e| e.to_string())??;
    let mut c = swatch_cache().lock().unwrap();
    if c.len() >= 16 {
        c.remove(0);
    }
    c.push((key, v.clone()));
    Ok(v)
}

// ------------------------------------------------------------------ settings, presets, projects

fn config_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| tr!("Folder pengaturan tidak bisa dibuat: {e}", "Cannot create the settings folder: {e}"))?;
    Ok(dir.join("studiozea.json"))
}

/// write via a temporary file + rename, so a crash never leaves a half-written file
fn write_atomic(path: &std::path::Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text).map_err(|e| tr!("Gagal menyimpan: {e}", "Cannot save: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| tr!("Gagal menyimpan: {e}", "Cannot save: {e}"))
}

const TEXT_MAX: u64 = 8 << 20;

fn read_small(path: &std::path::Path) -> Result<String, String> {
    let len = std::fs::metadata(path).map_err(|e| tr!("Tidak bisa dibuka: {e}", "Cannot open: {e}"))?.len();
    if len > TEXT_MAX {
        return Err(tr!("File terlalu besar", "File too large"));
    }
    std::fs::read_to_string(path).map_err(|e| tr!("Tidak bisa dibaca: {e}", "Cannot read: {e}"))
}

/// saved settings and own presets (JSON text, None on the first start)
#[tauri::command]
fn load_config(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let p = config_file(&app)?;
    if !p.exists() {
        return Ok(None);
    }
    let text = read_small(&p)?;
    if serde_json::from_str::<serde_json::Value>(&text).is_err() {
        // damaged: keep a copy (own presets may be recovered by hand), start with defaults
        let _ = std::fs::rename(&p, p.with_extension("json.rusak"));
        return Ok(None);
    }
    Ok(Some(text))
}

#[tauri::command]
fn save_config(app: tauri::AppHandle, text: String) -> Result<(), String> {
    if text.len() as u64 > TEXT_MAX {
        return Err(tr!("Pengaturan terlalu besar", "Settings too large"));
    }
    write_atomic(&config_file(&app)?, &text)
}

fn project_path(path: &str) -> Result<PathBuf, String> {
    let p = PathBuf::from(path);
    match p.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case("szproj") => Ok(p),
        _ => Err(tr!("File proyek harus berakhiran .szproj", "The project file must end in .szproj")),
    }
}

#[tauri::command]
fn save_project(path: String, text: String) -> Result<(), String> {
    if text.len() as u64 > TEXT_MAX {
        return Err(tr!("Proyek terlalu besar", "Project too large"));
    }
    write_atomic(&project_path(&path)?, &text)
}

#[tauri::command]
fn load_project(path: String) -> Result<String, String> {
    read_small(&project_path(&path)?)
}

/// the image of a project: the saved path, else a file of the same name next to the project
#[tauri::command]
fn find_project_image(project: String, image: String) -> Option<String> {
    let p = PathBuf::from(&image);
    if p.is_file() {
        return Some(image);
    }
    let name = p.file_name()?;
    let near = PathBuf::from(&project).parent()?.join(name);
    near.is_file().then(|| near.to_string_lossy().into_owned())
}

#[tauri::command]
fn source_name(state: State<'_>) -> String {
    state
        .lock()
        .unwrap()
        .src
        .as_ref()
        .map(|s| s.name.clone())
        .unwrap_or_default()
}

#[tauri::command]
fn min_dot_info(dpi: f64) -> (usize, u32) {
    (dtf::mindot_px(dpi), dtf::minarea_px(dpi))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());
    // update check + install (signed updates from the public studiozea-update releases)
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());
    builder
        .setup(|app| {
            // designs exported for the gang sheet ("Desain sekarang") only live for one run
            if let Ok(d) = app.path().app_cache_dir() {
                let _ = std::fs::remove_dir_all(d.join("gang"));
            }
            Ok(())
        })
        .manage(Mutex::new(AppState::default()))
        .invoke_handler(tauri::generate_handler![
            open_image,
            process,
            sep_process,
            sep_overview,
            sep_tile,
            sep_export,
            overview,
            tile,
            export_png,
            test_sheet,
            source_name,
            textures,
            texture_file,
            add_textures,
            remove_texture,
            pattern_swatches,
            load_config,
            save_config,
            save_project,
            load_project,
            find_project_image,
            batch_export,
            batch_cancel,
            ai_upscale,
            keep_ai_image,
            ai_cancel,
            ai_device,
            ai_device_retry,
            set_lang,
            gang_add,
            gang_thumb,
            gang_forget,
            gang_layout,
            gang_export,
            gang_cancel,
            gang_temp_path,
            min_dot_info
        ])
        .run(tauri::generate_context!())
        .expect("error while running StudioZea");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_messages_translate_words_not_paths() {
        EN.store(true, Ordering::Relaxed);
        assert_eq!(
            eng("lapisan 1: tekstur C:\\Users\\a\\tekstur\\desain_uji.png: tidak bisa dibaca (os error 2)"),
            "layer 1: texture C:\\Users\\a\\tekstur\\desain_uji.png: cannot be read (os error 2)"
        );
        assert_eq!(eng("desain 2 (900 × 400 px) lebih besar dari lembar"), "design 2 (900 × 400 px) larger than the sheet");
        assert_eq!(eng("/home/Hasil cetak/desain.png: dibatalkan"), "/home/Hasil cetak/desain.png: cancelled");
        assert!(eng("Hasil 9000×9000 px terlalu besar (maks. 40 Mpx). Gambar ini sudah cukup besar; Perbesar AI untuk gambar kecil.").starts_with("Result 9000×9000 px too large (max. 40"));
        assert_eq!(eng("RTX 4070 (Dx12): hasil tes berbeda dari prosesor (maks. 3.0/255)"), "RTX 4070 (Dx12): test result differs from the processor (max. 3.0/255)");
        EN.store(false, Ordering::Relaxed);
        assert_eq!(eng("desain 2"), "desain 2");
        EN.store(true, Ordering::Relaxed);
    }

    #[test]
    fn ai_cache_key_follows_file_scale_and_smoothing() {
        let dir = std::env::temp_dir().join(format!("sz_aicache_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("logo kecil.png");
        let img = io::Rgba8 { w: 3, h: 2, px: vec![200; 24], has_alpha: true };
        io::save_rgba8(&img, &src).unwrap();
        assert_eq!(png_size(&src), Some((3, 2)));
        let cache = dir.join("cache");
        let a = ai_cache_path(&cache, &src, upscale::Scale::X4, 0.5).unwrap();
        assert_eq!(a, ai_cache_path(&cache, &src, upscale::Scale::X4, 0.5).unwrap());
        assert_eq!(a.file_name().unwrap(), "logo kecil_ai4x.png");
        assert!(a.starts_with(&cache));
        assert_ne!(a, ai_cache_path(&cache, &src, upscale::Scale::X2, 0.5).unwrap());
        assert_ne!(a, ai_cache_path(&cache, &src, upscale::Scale::X4, 0.2).unwrap());
        // the file changed (other length): a new entry
        let len = std::fs::metadata(&src).unwrap().len() as usize;
        std::fs::write(&src, vec![7u8; len + 10]).unwrap();
        assert_ne!(a, ai_cache_path(&cache, &src, upscale::Scale::X4, 0.5).unwrap());
        assert!(png_size(&dir.join("tidak ada.png")).is_none());

        // pruning keeps the newest AI_CACHE_KEEP entries and the protected one
        let t0 = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        let mut files = Vec::new();
        for i in 0..AI_CACHE_KEEP + 3 {
            let d = cache.join(format!("{i:016x}"));
            std::fs::create_dir_all(&d).unwrap();
            let f = d.join("x_ai2x.png");
            std::fs::write(&f, b"x").unwrap();
            std::fs::File::options().write(true).open(&f).unwrap().set_modified(t0 + std::time::Duration::from_secs(i as u64)).unwrap();
            files.push(f);
        }
        prune_ai_cache(&cache, &[&files[0]]);
        assert!(files[0].exists(), "protected entry kept");
        assert!(!files[1].exists() && !files[2].exists(), "oldest deleted");
        assert!(files[3..].iter().all(|f| f.exists()), "newest kept");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
