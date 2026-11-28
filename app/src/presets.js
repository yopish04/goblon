import { L } from './i18n.js'
// Ready-made looks. Values map 1:1 onto the engine Settings (see engine/src/render.rs).

export const SHAPES = [
  ['bulat', 'Bulat (seperti referensi)', 'Round (classic)'],
  ['bulat-murni', 'Bulat murni', 'Pure round'],
  ['elips', 'Elips', 'Ellipse'],
  ['kotak', 'Kotak', 'Square'],
  ['berlian', 'Berlian', 'Diamond'],
  ['garis', 'Garis', 'Line'],
  ['silang', 'Silang', 'Cross'],
  ['gelombang', 'Garis gelombang', 'Wavy line'],
  ['arsir', 'Arsir silang', 'Crosshatch'],
  ['kasar', 'Titik kasar (analog)', 'Rough dot (analog)'],
  ['bintang', 'Bintang', 'Star'],
  ['segitiga', 'Segitiga', 'Triangle'],
  ['rantai', 'Rantai', 'Chain'],
]

// quick screen angles (deg); null = automatic (22.5, like the reference)
export const ANGLES = [0, 15, 45, 75, 90]
export const LPI_MIN = 3
export const LPI_MAX = 150

export const PATTERNS = [
  ['am', 'Halftone AM (rosette)', 'AM halftone (rosette)'],
  ['fm', 'FM / stokastik', 'FM / stochastic'],
  ['grain-halus', 'Grain halus', 'Fine grain'],
  ['grain-sedang', 'Grain sedang', 'Medium grain'],
  ['grain-kasar', 'Grain kasar', 'Coarse grain'],
  ['interlock', 'Interlock (paling rapi)', 'Interlock (cleanest)'],
  ['bayer', 'Dither Bayer', 'Bayer dither'],
  ['difusi', 'Dither difusi', 'Diffusion dither'],
]

// Vintage texture layer (engine Settings.distress): a texture from the built-in
// library (repository folder textures/, path "lib:<file>") or a file of the user.
// The texture covers the whole print; black = worn ink.
export function newTexture(path) {
  return { kind: 'gambar', path, mode: 'lubang', threshold: 0.5, amount: 0.5, zoom: 1, angle: 0, mirror: false, invert: false, offset_mm: [0, 0] }
}
/** label of a [value, Indonesian, English] option / [Indonesian, cm, English] size in
 *  the current language */
export const optLabel = (o) => L(o[1], o[2] ?? o[1])
export const sizeLabel = (o) => L(o[0], o[2] ?? o[0])

export const DPIS = [150, 200, 300, 360, 600]

// quick print widths (cm) for DTF transfers
export const SIZES = [
  ['Logo dada', 9, 'Chest logo'],
  ['Saku', 10, 'Pocket'],
  ['A5', 14.8],
  ['Anak', 20, 'Kids'],
  ['A4', 21],
  ['A3', 29.7],
  ['30 cm', 30],
  ['35 cm', 35],
  ['40 cm', 40],
]

const AUTO_ANGLE = { angle: null, rosette: null, halftoneOn: true }
// every preset says whether the true-colour dots look is on (only 'bandtee' turns it on),
// so choosing another preset after it gives that preset's own look
export const PRESETS = [
  { id: 'ref', name: 'Referensi (35 LPI rosette)', en: 'Classic (35 LPI rosette)', set: { pattern: 'am', lpi: 35, shape: 'bulat', grid: 'auto', k: 'off', edges: false, ...AUTO_ANGLE } },
  { id: 'tajam', name: 'Tajam (tepi huruf bersih)', en: 'Sharp (clean text edges)', set: { pattern: 'am', lpi: 35, shape: 'bulat', grid: 'auto', k: 'off', edges: true, ...AUTO_ANGLE } },
  { id: 'foto', name: 'Foto / detail halus (FM)', en: 'Photo / fine detail (FM)', set: { pattern: 'fm', edges: false, halftoneOn: true } },
  { id: 'vintage', name: 'Vintage grain', en: 'Vintage grain', set: { pattern: 'grain-sedang', edges: false, halftoneOn: true } },
  { id: 'netral', name: 'Kaos putih + hitam K (GCR)', en: 'White shirt + black K (GCR)', set: { pattern: 'am', lpi: 35, shape: 'bulat', grid: 'auto', k: 'gcr', edges: false, ...AUTO_ANGLE } },
  { id: 'popart', name: 'Pop art: titik besar (8 LPI, 45°)', en: 'Pop art: big dots (8 LPI, 45°)', set: { pattern: 'am', lpi: 8, shape: 'bulat', grid: 'auto', edges: false, angle: 45, rosette: null, halftoneOn: true } },
  { id: 'bintang', name: 'Pop art: bintang (6 LPI)', en: 'Pop art: stars (6 LPI)', set: { pattern: 'am', lpi: 6, shape: 'bintang', grid: 'auto', edges: false, angle: 0, rosette: null, halftoneOn: true } },
  { id: 'segitiga', name: 'Retro: segitiga (10 LPI)', en: 'Retro: triangles (10 LPI)', set: { pattern: 'am', lpi: 10, shape: 'segitiga', grid: 'auto', edges: false, angle: 0, rosette: null, halftoneOn: true } },
  { id: 'interlock', name: 'Interlock rapi (35 LPI, 45°)', en: 'Clean interlock (35 LPI, 45°)', set: { pattern: 'interlock', lpi: 35, angle: null, edges: false, halftoneOn: true } },
  { id: 'difusi', name: 'Dither difusi (detail halus)', en: 'Diffusion dither (fine detail)', set: { pattern: 'difusi', edges: false, halftoneOn: true } },
  { id: 'bandtee', name: 'Band tee: titik warna asli (12 LPI)', en: 'Band tee: true-colour dots (12 LPI)', set: { pattern: 'am', lpi: 12, shape: 'bulat-murni', grid: 'auto', edges: false, angle: 22.5, rosette: null, halftoneOn: true, fades: false, trueDots: true, dotSize: 100, solidFrom: 92, liftColour: 100 } },
  { id: 'garis', name: 'Retro: garis (15 LPI, 45°)', en: 'Retro: lines (15 LPI, 45°)', set: { pattern: 'am', lpi: 15, shape: 'garis', grid: 'auto', edges: false, angle: 45, rosette: null, halftoneOn: true } },
].map((p) => ({ ...p, set: { trueDots: false, ...p.set } }))

export const DEFAULTS = {
  shirtType: 'hitam', // hitam | putih | warna
  shirtColor: '#1c2541',
  autoShirt: true, // on opening an image, pick the shirt that matches its flat background
  bg: 'otomatis', // otomatis | warna | tidak
  bgTolerance: null, // null = automatic
  bgPockets: null,
  knockout: 0,
  autoTrim: true,
  crop: null, // [x0, y0, x1, y1] in source px
  rotate: 0, // turn clockwise 0 | 90 | 180 | 270 (after crop / trim, before halftone)
  mirror: false, // mirror left-right
  marginMm: 0, // transparent margin around the exported PNG
  erase: [], // brush strokes [{mode: 'hapus'|'kembalikan'|'teks'|'bukan-teks', r, pts: [[x, y], ...]}] in source px
  widthCm: 30,
  unit: 'cm', // cm | in | px (how the size is shown / typed; stored as cm)
  resample: 'lanczos', // lanczos (sharpest) | bicubic (softer, like Photoshop)
  dpi: 300,
  pattern: 'am',
  lpi: 35,
  shape: 'bulat',
  grid: 'auto',
  angle: null, // screen angle (deg); null = automatic 22.5
  rosette: null, // white/coloured shirt: true = rosette, false = dot-on-dot, null = automatic
  halftoneOn: true, // false = no halftone at all (true colours, edges flattened, texture-only looks)
  shirtCut: 12, // % : with halftone off, colours this close to the shirt colour are not printed
  fades: false, // halftone only on fades (true colours elsewhere)
  fadesArea: 50, // % : below this distance from the shirt colour the design becomes dots
  trueDots: false, // "titik warna asli": one screen over everything, dots in the design's own colours
  dotSize: 100, // % : true-colour dots: dot size where the design is fully there (smaller only where it fades into the shirt)
  solidFrom: 92, // % : true-colour dots: colours at least this light print solid (100 = never)
  liftColour: 100, // % : true-colour dots: colour lift (dots at full strength, smaller; 0 = design colour as is)
  k: 'off',
  edges: false,
  distress: [],
  batchFit: 'lebar', // export banyak: lebar = width above, kotak = fit inside batchW x batchH cm
  batchW: 30,
  batchH: 40,
  gangWidth: 58, // gang sheet film width (cm)
  gangMaxLen: 0, // cm; 0 = one sheet as long as needed
  gangGap: 5, // mm between designs
  gangMargin: 5, // mm at the film edges
  gangRotate: true,
  aiHalus: 'seimbang', // AI upscale: halus (cleans JPEG noise) | seimbang | tekstur (keeps fine texture)
  batchAi: false, // export banyak: AI upscale images below 150 DPI effective first
  aiDevice: 'otomatis', // otomatis = graphics card when it passes its self-test, else processor | cpu
  protectText: false, // "Lindungi teks": detected lettering prints solid (own colours), not screened
  textStyle: 'padat', // how protected lettering prints: 'padat' (solid) | 'halus' (finer dots, AM / interlock)
  // print type: 'dtf' | 'sablon' (screen printing: colour separation into films)
  printMode: 'dtf',
  // sablon: separation method, ink set ('auto' = from the design, a fixed set, 'saya' =
  // the user's own), number of inks (automatic), the inks when chosen by hand
  // ([{rgb, opacity, boost}], print order; [] = from the set) and how many of them print
  // before the underbase, underbase, choke, highlight white, film screen and dot style,
  // colour expand (px), drop films below this % of the design, colour boost (%), and
  // the film border with registration marks
  sepMethod: 'simulasi',
  sepSet: 'auto',
  sepKatalog: 'std', // with sepSet 'katalog': which ink catalogue ('std' = standard Pantone, 'x:<brand>' = imported)
  sepColors: 6,
  sepInks: [],
  sepBaseAfter: 0,
  sepBase: 'auto',
  sepChoke: 0.1,
  sepHighlight: true,
  sepLpi: 55,
  sepAngle: 25,
  sepShape: 'elips',
  sepDots: 'satu-sudut',
  sepExpand: 0,
  sepDrop: 0.3,
  sepBoost: 0,
  sepSaver: 0, // % : "hemat tinta": less underbase / ink where it changes the colour only a little
  sepWhiteAngle: null, // white films (underbase, highlight) on their own angle; null = the colour angle
  sepMaxDot: 100, // % : white films in gradients at most this (no flat white patches); 100 = off
  sepStrongInks: false, // dark shirts: strong automatic inks only (no pastel / cream ink)
  sepMargin: 12,
}

/** ready-made separation setups (Sablon). 'pro': learnt from hand-made film separations
 *  of band shirts (CorelDRAW): 35 LPI, colours dot on dot at 22.5 deg, the white films on
 *  their own screen 15 deg away (no moire, robust to a shifted white screen), at most 85 % white in gradients (smooth light tones keep their
 *  dots), strong inks only (no cream ink), round dots, lettering / logos solid */
export const SEP_PRESETS = [
  { id: 'pro', name: 'Film PRO (35 LPI, gradasi halus, putih beda sudut)', en: 'PRO films (35 LPI, smooth gradients, white on its own angle)',
    set: { sepMethod: 'simulasi', sepSet: 'auto', sepColors: 6, sepInks: [], sepBaseAfter: 0, sepBase: 'auto', sepHighlight: true, sepLpi: 35, sepAngle: 22.5, sepWhiteAngle: 7.5, sepSaver: 0, sepMaxDot: 85, sepStrongInks: true, sepShape: 'bulat', sepDots: 'satu-sudut', sepChoke: 0.1, protectText: true } },
  // Mode 1: the PRO screen with inks from stock (the chosen catalogue), Mode 2: one
  // catalogue ink per flat colour, solid, on a solid base, colours spread 1 px (trap)
  { id: 'stok', name: 'Simulasi dari stok (tinta katalog, 35 LPI)', en: 'Simulated from stock (catalogue inks, 35 LPI)',
    set: { sepMethod: 'simulasi', sepSet: 'katalog', sepColors: 6, sepInks: [], sepBaseAfter: 0, sepBase: 'auto', sepHighlight: true, sepLpi: 35, sepAngle: 22.5, sepWhiteAngle: 7.5, sepSaver: 0, sepMaxDot: 85, sepStrongInks: true, sepShape: 'bulat', sepDots: 'satu-sudut', sepChoke: 0.1, protectText: true } },
  { id: 'spot-pantone', name: 'Spot Pantone (warna rata, tinta katalog)', en: 'Spot Pantone (flat colours, catalogue inks)',
    set: { sepMethod: 'spot', sepSet: 'katalog', sepColors: 8, sepInks: [], sepBaseAfter: 0, sepBase: 'auto', sepHighlight: true, sepExpand: 1, sepChoke: 0.1, protectText: false } },
  { id: 'standar', name: 'Standar StudioZea (55 LPI)', en: 'StudioZea standard (55 LPI)',
    set: { sepMethod: 'simulasi', sepSet: 'auto', sepColors: 6, sepInks: [], sepBaseAfter: 0, sepBase: 'auto', sepHighlight: true, sepLpi: 55, sepAngle: 25, sepWhiteAngle: null, sepSaver: 0, sepMaxDot: 100, sepStrongInks: false, sepShape: 'elips', sepDots: 'satu-sudut', sepChoke: 0.1, protectText: false } },
]

/** AI smoothing choice -> Real-ESRGAN denoise strength (1 = smooth, 0 = keep noise) */
export const AI_DENOISE = { halus: 1, seimbang: 0.5, tekstur: 0.2 }
/** the app suggests the AI upscale below this effective resolution (same as the engine) */
export const LOW_DPI = 150
/** largest AI result (pixels), like studiozea_engine::upscale::MAX_OUT_PX */
export const AI_MAX_PX = 40e6

/** AI factor for an effective resolution: 2 when twice is enough for the print DPI
 *  (at most 300), else 4; like ai_factor in the app backend */
export function aiScaleFor(effDpi, dpi) {
  return effDpi && effDpi * 2 >= Math.min(dpi, 300) ? 2 : 4
}

function hexToRgb(h) {
  const v = h.replace('#', '')
  return [0, 2, 4].map((i) => parseInt(v.slice(i, i + 2), 16) / 255)
}

/** UI state -> engine Job (serde format of studiozea_engine::Job) */
export function toJob(s) {
  const shirt =
    s.shirtType === 'hitam' ? { type: 'hitam' } : s.shirtType === 'putih' ? { type: 'putih' } : { type: 'warna', rgb: hexToRgb(s.shirtColor) }
  const pattern = s.pattern.startsWith('grain') ? { grain: s.pattern.split('-')[1] } : s.pattern
  return {
    remove_bg: s.bg,
    bg_tolerance: s.bgTolerance == null ? null : Number(s.bgTolerance),
    bg_pockets: s.bgPockets == null ? null : !!s.bgPockets,
    crop: s.crop,
    erase: s.erase || [],
    rotate: [0, 90, 180, 270].includes(s.rotate) ? s.rotate : 0,
    mirror: !!s.mirror,
    auto_trim: s.autoTrim,
    width_cm: Number(s.widthCm) > 0 ? Number(s.widthCm) : null,
    settings: {
      shirt,
      lpi: Number(s.lpi),
      shape: s.shape,
      dpi: Number(s.dpi),
      grid: s.grid,
      angle: s.angle == null || !Number.isFinite(Number(s.angle)) ? null : Number(s.angle),
      rosette: s.rosette == null ? null : !!s.rosette,
      resample: s.resample === 'bicubic' ? 'bicubic' : 'lanczos',
      shirt_cut: (Number.isFinite(Number(s.shirtCut)) ? Math.min(100, Math.max(0, Number(s.shirtCut))) : 12) / 100,
      halftone_area: s.halftoneOn === false ? 0 : s.fades && !s.trueDots ? Math.min(100, Math.max(1, Number(s.fadesArea) || 50)) / 100 : null,
      true_dots: s.halftoneOn !== false && s.trueDots
        ? { size: Math.min(100, Math.max(1, Number(s.dotSize) || 100)) / 100, solid: Math.min(100, Math.max(1, Number(s.solidFrom) || 92)) / 100, lift: (Number.isFinite(Number(s.liftColour)) ? Math.min(100, Math.max(0, Number(s.liftColour))) : 100) / 100 }
        : null,
      protect_text: !!s.protectText && s.halftoneOn !== false,
      text_style: s.textStyle === 'halus' ? 'halus' : 'padat',
      pattern,
      k: s.shirtType === 'hitam' ? 'off' : s.k,
      knockout: s.shirtType === 'warna' ? Number(s.knockout) : 0,
      edges: !!s.edges,
      distress: (s.distress || [])
        .filter((l) => l.path)
        .map((l) => ({
          kind: 'gambar',
          path: l.path,
          mode: l.mode,
          threshold: Number(l.threshold),
          amount: Number(l.amount),
          zoom: Number(l.zoom) || 1,
          angle: Number(l.angle) || 0,
          mirror: !!l.mirror,
          invert: !!l.invert,
          tile: true,
          offset_mm: (Array.isArray(l.offset_mm) && l.offset_mm.length === 2 ? l.offset_mm : [0, 0]).map((v) => Number(v) || 0),
        })),
    },
  }
}

/** shirt for a flat background colour [r, g, b] 0..255 (same limits as the engine's
 *  is_black / is_white): black -> black shirt, white -> white shirt, else that colour */
export function shirtFor(bg) {
  if (bg.every((v) => v < 25.5)) return { shirtType: 'hitam' }
  if (bg.every((v) => v > 229.5)) return { shirtType: 'putih' }
  return { shirtType: 'warna', shirtColor: '#' + bg.map((v) => v.toString(16).padStart(2, '0')).join('') }
}

export function shirtCss(s) {
  return s.shirtType === 'hitam' ? '#000000' : s.shirtType === 'putih' ? '#ffffff' : s.shirtColor
}

// ------------------------------------------------------------------ saved settings
// What is remembered between starts and kept in "Preset saya": the look, not the
// image-specific crop / background tolerance. Projects keep everything.
export const IMAGE_KEYS = ['crop', 'erase', 'rotate', 'mirror', 'bgTolerance', 'bgPockets', 'sepInks', 'sepBaseAfter']
// the print type is not part of a look (a preset never switches DTF / sablon); projects keep it
export const LOOK_KEYS = Object.keys(DEFAULTS).filter((k) => !IMAGE_KEYS.includes(k) && k !== 'printMode')
export const PROJECT_KEYS = Object.keys(DEFAULTS)

const ENUMS = {
  shirtType: ['hitam', 'putih', 'warna'],
  bg: ['otomatis', 'warna', 'tidak'],
  unit: ['cm', 'in', 'px'],
  resample: ['lanczos', 'bicubic'],
  pattern: PATTERNS.map(([v]) => v),
  shape: SHAPES.map(([v]) => v),
  grid: ['auto', 'rapi', 'presisi'],
  k: ['off', 'garis', 'gcr'],
  batchFit: ['lebar', 'kotak'],
  rotate: [0, 90, 180, 270],
  aiHalus: Object.keys(AI_DENOISE),
  aiDevice: ['otomatis', 'cpu'],
  textStyle: ['padat', 'halus'],
  printMode: ['dtf', 'sablon'],
  sepMethod: ['simulasi', 'index', 'spot'],
  sepSet: ['auto', 'katalog', 'dasar', 'dasar-abu-dulu', 'hemat', 'gelap', 'saya'],
  sepDots: ['satu-sudut', 'roset', 'interlock'],
  sepBase: ['auto', 'ya', 'tidak'],
  sepShape: SHAPES.map(([v]) => v),
}

export const BRUSH_MODES = ['hapus', 'kembalikan', 'teks', 'bukan-teks']
/** the "Lindungi teks" brush modes (the others belong to the eraser) */
export const isTextMode = (m) => m === 'teks' || m === 'bukan-teks'
/** LPI of the lettering with "Titik halus" (as Settings::text_fine_lpi in the engine) */
export const textFineLpi = (lpi, dpi) => Math.max(lpi, Math.min(Math.max(lpi * 2, 45), Math.min(75, dpi / 4)))
const okStroke = (s) => s && BRUSH_MODES.includes(s.mode) && Number.isFinite(s.r) && s.r >= 0.5 && s.r <= 5000 &&
  Array.isArray(s.pts) && s.pts.length > 0 && s.pts.every((p) => Array.isArray(p) && p.length === 2 && Number.isFinite(p[0]) && Number.isFinite(p[1]))

/** a valid ink list ([{rgb, opacity, boost?, name?, catId?}], at most 12; name / catId = the catalogue ink and its catalogue) */
export const okInks = (v) => Array.isArray(v) && v.length <= 12 && v.every((i) => i && Array.isArray(i.rgb) && i.rgb.length === 3 && i.rgb.every((c) => Number.isInteger(c) && c >= 0 && c <= 255) &&
  Number.isFinite(i.opacity) && i.opacity >= 0.05 && i.opacity <= 1 && (i.boost === undefined || (Number.isFinite(i.boost) && i.boost >= 0 && i.boost <= 1)) &&
  (i.name === undefined || (typeof i.name === 'string' && i.name.length <= 200)) && (i.catId === undefined || (typeof i.catId === 'string' && i.catId.length <= 80)))

/** a saved value is used only when it has the right type (old or damaged files never break the app) */
function okValue(k, v) {
  const d = DEFAULTS[k]
  if (ENUMS[k]) return ENUMS[k].includes(v)
  if (k === 'sepKatalog') return typeof v === 'string' && v.length <= 80
  if (k === 'shirtColor') return typeof v === 'string' && /^#[0-9a-fA-F]{6}$/.test(v)
  if (k === 'crop') return v === null || (Array.isArray(v) && v.length === 4 && v.every((x) => Number.isFinite(x) && x >= 0))
  if (k === 'erase') return Array.isArray(v) && v.length <= 20000 && v.every(okStroke)
  if (k === 'sepInks') return okInks(v)
  if (k === 'distress') return Array.isArray(v) && v.length <= 4 && v.every((l) => l && typeof l === 'object' && (l.path === null || typeof l.path === 'string'))
  if (k === 'angle' || k === 'bgTolerance' || k === 'sepWhiteAngle') return v === null || Number.isFinite(v)
  if (k === 'rosette' || k === 'bgPockets') return v === null || typeof v === 'boolean'
  if (typeof d === 'number') return Number.isFinite(v)
  return typeof v === typeof d
}

/** `base` with the valid saved values of `keys` applied */
export function mergeSettings(base, saved, keys) {
  const out = { ...base }
  if (!saved || typeof saved !== 'object') return out
  for (const k of keys) if (k in saved && okValue(k, saved[k])) out[k] = saved[k]
  return out
}

export function pickSettings(s, keys) {
  return Object.fromEntries(keys.map((k) => [k, s[k]]))
}

// ------------------------------------------------------------------ sablon (screen printing)
/** the separation settings for the engine (sablon::SepSettings); `palette` = the colours
 * of the chosen ink catalogue ([[r, g, b]], for the ink set 'katalog') */
export function toSep(s, palette = []) {
  const num = (v, d, lo, hi) => (Number.isFinite(Number(v)) ? Math.min(hi, Math.max(lo, Number(v))) : d)
  return {
    method: s.sepMethod,
    colors: Math.round(num(s.sepColors, 6, 1, 12)),
    inks: (s.sepInks || []).map((i) => ({ rgb: i.rgb, opacity: i.opacity, boost: num(i.boost, 0, 0, 1) })),
    // inks given one by one: the set is not used (the same inks keep the same result)
    ink_set: s.sepSet === 'saya' || (s.sepInks || []).length ? 'auto' : s.sepSet,
    palette: s.sepSet === 'katalog' && !(s.sepInks || []).length ? palette : [],
    base_after: Math.round(num(s.sepBaseAfter, 0, 0, (s.sepInks || []).length)),
    underbase: s.sepBase === 'auto' ? null : s.sepBase === 'ya',
    choke_mm: num(s.sepChoke, 0.1, 0, 2),
    highlight: !!s.sepHighlight,
    lpi: num(s.sepLpi, 55, 10, 100),
    angle: num(s.sepAngle, 25, -360, 360),
    shape: s.sepShape,
    dot_style: s.sepDots,
    base_opacity: 0.85,
    expand_px: Math.round(num(s.sepExpand, 0, 0, 20)),
    drop_below: num(s.sepDrop, 0.3, 0, 10),
    boost: num(s.sepBoost, 0, 0, 50) / 100,
    ink_saver: num(s.sepSaver, 0, 0, 100) / 100,
    max_dot: num(s.sepMaxDot, 100, 50, 100) / 100,
    strong_inks: !!s.sepStrongInks,
    white_angle: s.sepWhiteAngle == null || !Number.isFinite(Number(s.sepWhiteAngle)) ? null : Math.min(360, Math.max(-360, Number(s.sepWhiteAngle))),
  }
}

/** the shirt for the ink sets: 'gelap' (dark: underbase, grey, no black), 'warna'
 * (coloured: underbase and black) or 'terang' (white / light: no underbase), as the
 * engine decides (OKLab lightness) */
export function shirtTone(s) {
  const [r, g, b] = hexToRgb(shirtCss(s)).map((v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
  const L = 0.2105 * Math.cbrt(0.4122 * r + 0.5363 * g + 0.0514 * b) + 0.7936 * Math.cbrt(0.2119 * r + 0.6807 * g + 0.1074 * b) - 0.0041 * Math.cbrt(0.0883 * r + 0.2817 * g + 0.63 * b)
  return L < 0.45 ? 'gelap' : L < 0.85 ? 'warna' : 'terang'
}

/** the ink sets that fit a shirt tone: [value, Indonesian, English] */
export function inkSets(tone) {
  const dark = tone === 'gelap'
  return [
    ['auto', 'Otomatis dari desain', 'Automatic from the design'],
    ['katalog', 'Katalog tinta (Pantone / merek)', 'Ink catalogue (Pantone / brand)'],
    ['dasar', dark ? '6 warna dasar + abu' : '6 warna dasar + hitam', dark ? '6 basic colours + grey' : '6 basic colours + black'],
    ...(dark ? [['dasar-abu-dulu', '6 warna dasar, abu dicetak dulu', '6 basic colours, grey printed first']] : []),
    ['hemat', dark ? 'Hemat: kuning, merah, biru' : 'Hemat: kuning, merah, biru, hitam', dark ? 'Cheap: yellow, red, blue' : 'Cheap: yellow, red, blue, black'],
    ...(tone !== 'terang' ? [['gelap', 'Gelap / api / vintage', 'Dark / fire / vintage']] : []),
  ]
}

// common plastisol ink colours [rgb, Indonesian, English]: the nearest one names an ink
const INK_NAMES = [
  [[255, 255, 255], 'Putih', 'White'], [[20, 20, 20], 'Hitam', 'Black'], [[128, 128, 128], 'Abu-abu', 'Grey'],
  [[190, 190, 190], 'Abu muda', 'Light grey'], [[70, 72, 70], 'Abu tua', 'Dark grey'],
  [[255, 230, 0], 'Kuning', 'Yellow'], [[250, 180, 30], 'Kuning emas', 'Gold'], [[245, 120, 20], 'Oranye', 'Orange'],
  [[220, 30, 30], 'Merah', 'Red'], [[150, 20, 10], 'Merah tua', 'Dark red'], [[110, 10, 30], 'Marun', 'Maroon'],
  [[240, 120, 160], 'Pink', 'Pink'], [[230, 40, 130], 'Magenta', 'Magenta'], [[120, 40, 150], 'Ungu', 'Purple'],
  [[30, 70, 190], 'Biru royal', 'Royal blue'], [[20, 30, 90], 'Navy', 'Navy'], [[90, 170, 230], 'Biru muda', 'Light blue'],
  [[0, 160, 170], 'Tosca', 'Teal'], [[30, 150, 60], 'Hijau', 'Green'], [[20, 80, 40], 'Hijau tua', 'Dark green'],
  [[150, 200, 60], 'Hijau muda', 'Light green'], [[120, 70, 30], 'Coklat', 'Brown'], [[240, 190, 150], 'Kulit', 'Skin'],
  [[230, 210, 170], 'Krem', 'Cream'], [[180, 130, 70], 'Coklat muda', 'Tan'],
]
/** [Indonesian, English] name of an ink colour; the underbase / highlight by their role */
export function inkName(rgb, role) {
  if (role === 'underbase') return ['Underbase putih', 'White underbase']
  if (role === 'highlight') return ['Highlight putih', 'Highlight white']
  const lin = (v) => ((v /= 255) <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4)
  const lab = (c) => {
    const [r, g, b] = c.map(lin)
    const l = Math.cbrt(0.4122 * r + 0.5363 * g + 0.0514 * b)
    const m = Math.cbrt(0.2119 * r + 0.6807 * g + 0.1074 * b)
    const s2 = Math.cbrt(0.0883 * r + 0.2817 * g + 0.63 * b)
    return [0.2105 * l + 0.7936 * m - 0.0041 * s2, 1.978 * l - 2.4286 * m + 0.4506 * s2, 0.0259 * l + 0.7828 * m - 0.8087 * s2]
  }
  const a = lab(rgb)
  let best = INK_NAMES[0]
  let bd = Infinity
  for (const n of INK_NAMES) {
    const b = lab(n[0])
    const d = (a[0] - b[0]) ** 2 + (a[1] - b[1]) ** 2 + (a[2] - b[2]) ** 2
    if (d < bd) {
      bd = d
      best = n
    }
  }
  return [best[1], best[2]]
}
export const rgbHex = (c) => '#' + c.map((v) => v.toString(16).padStart(2, '0')).join('')
export const hexRgb = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16))
