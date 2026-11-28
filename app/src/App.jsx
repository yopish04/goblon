import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { open, save } from '@tauri-apps/plugin-dialog'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import logoMark from './logo-mark.svg'
import { SEP_PRESETS, AI_DENOISE, AI_MAX_PX, LOW_DPI, aiScaleFor, ANGLES, DEFAULTS, IMAGE_KEYS, isTextMode, textFineLpi, shirtFor, DPIS, SIZES, LOOK_KEYS, PROJECT_KEYS, mergeSettings, pickSettings, LPI_MAX, LPI_MIN, PATTERNS, PRESETS, SHAPES, newTexture, optLabel, sizeLabel, shirtCss, toJob, toSep, inkName, rgbHex, hexRgb, okInks, shirtTone, inkSets } from './presets.js'
import { STD, STD_ID, inkKind, catalogPalette, inkTitle, inkRef, inkLine, findInk, okCatalogs, readXlsx, catalogsFromSheets, hexToRgb8 } from './katalog.js'
import CanvasViewer from './CanvasViewer.jsx'
import { Help, Menu, NumField, Row, Seg, Slider, fmt, unpack } from './ui.jsx'
import Icon from './icons.jsx'
import GangSheet from './GangSheet.jsx'
import { UpdateBar, useUpdater } from './Updater.jsx'
import { THEMES, applyTheme, onSystemTheme } from './theme.js'
import { L, LANGS, getLang, setLang } from './i18n.js'

const PROJ_FILTERS = () => [{ name: L('Proyek StudioZea', 'StudioZea project'), extensions: ['szproj'] }]
const IMG_FILTERS = () => [{ name: L('Gambar', 'Images'), extensions: ['png', 'jpg', 'jpeg', 'webp', 'tif', 'tiff', 'bmp'] }]

// pattern size slider: logarithmic, so 3-10 LPI (big dots) gets as much room as 35-150
const LPI_SPAN = Math.log(LPI_MAX / LPI_MIN)
const lpiToSlider = (v) => Math.round((1000 * Math.log(Math.min(LPI_MAX, Math.max(LPI_MIN, Number(v) || 35)) / LPI_MIN)) / LPI_SPAN)
const sliderToLpi = (t) => {
  const v = LPI_MIN * Math.exp((t / 1000) * LPI_SPAN)
  return v < 10 ? Math.round(v * 2) / 2 : Math.round(v)
}

/** visual picker of the halftone shapes; previews rendered by the engine at the current angle */
function PatternPicker({ shape, angle, onChange }) {
  const [sw, setSw] = useState({}) // angle key -> [{shape, thumb}]
  const key = angle == null ? 'auto' : String(angle)
  useEffect(() => {
    if (sw[key]) return undefined
    const t = setTimeout(() => {
      invoke('pattern_swatches', { angle: angle == null ? null : Number(angle) })
        .then((v) => setSw((m) => ({ ...m, [key]: v })))
        .catch(() => {})
    }, 150)
    return () => clearTimeout(t)
  }, [key])
  const list = sw[key] || Object.values(sw).at(-1) || []
  const thumb = Object.fromEntries(list.map((x) => [x.shape, x.thumb]))
  return (
    <div className="patgrid">
      {SHAPES.map((o) => [o[0], optLabel(o)]).map(([v, l]) => (
        <button key={v} type="button" className={v === shape ? 'pat on' : 'pat'} onClick={() => onChange(v)} title={l}>
          {thumb[v] ? <img src={thumb[v]} alt={l} draggable={false} /> : <span className="patph" />}
          <span className="texname">{l}</span>
        </button>
      ))}
    </div>
  )
}

// settings that are not undo steps (they belong to the batch / gang sheet panels)
const NO_HISTORY = (k) => k.startsWith('batch') || k.startsWith('gang') || k.startsWith('ai')
/** a and b equal on every key except the ones `skip` accepts */
function sameExcept(a, b, skip) {
  const keys = new Set([...Object.keys(a), ...Object.keys(b)])
  for (const k of keys) if (!skip(k) && a[k] !== b[k]) return false
  return true
}

const PATTERN_HINT = () => ({
  interlock: L('Titik bulat seragam di grid yang sama untuk semua warna (45°), terang dan gelap saling mengunci seperti papan catur di nada 50 %. Tanpa bintik acak: paling rapi.',
    'Even round dots on one grid for all colours (45°); light and dark lock like a chessboard at 50 %. No random specks: the cleanest.'),
  bayer: L('Dither teratur (Bayer): pola kotak-kotak kecil yang rapi, gaya retro / pixel. Titik minimal 0,15 mm.', 'Ordered (Bayer) dither: small neat checks, retro / pixel style. Dots at least 0.15 mm.'),
  difusi: L('Dither difusi (Floyd–Steinberg): titik menyebar mengikuti detail gambar, tajam di tepi. Titik minimal 0,15 mm.', 'Diffusion (Floyd–Steinberg) dither: dots follow the detail, sharp at edges. Dots at least 0.15 mm.'),
})

/** a print warning from the backend ({code, a, b, c}) in the interface language */
function warningText(w) {
  const n = (v) => Math.round(v)
  switch (w.code) {
    case 'res':
      return L(`Resolusi sumber rendah: ${n(w.a)} DPI efektif (min. 150), detail bisa blur.`, `Low source resolution: ${n(w.a)} effective DPI (min. 150), details may look blurry.`)
    case 'levels':
      return L(`${n(w.a)} LPI pada ${n(w.b)} DPI hanya punya ${n(w.c)} tingkat nada. Sarankan 600 DPI atau LPI lebih rendah.`, `${n(w.a)} LPI at ${n(w.b)} DPI has only ${n(w.c)} tone levels. Use 600 DPI or a lower LPI.`)
    case 'dot-on-dot':
      return L('LPI ≥ 55 di kaos putih/berwarna: otomatis memakai dot-on-dot (tanpa rosette) supaya nada tetap akurat.', 'LPI ≥ 55 on a white/coloured shirt: dot-on-dot is used (no rosette) to keep the tones accurate.')
    default:
      return String(w.code || w)
  }
}

/** crop box of the same design at `f` times the size, inside the new image */
function scaleCrop(c, f, info) {
  if (!c) return null
  const x0 = Math.min(Math.round(c[0] * f), info.w - 1)
  const y0 = Math.min(Math.round(c[1] * f), info.h - 1)
  return [x0, y0, Math.max(x0 + 1, Math.min(Math.round(c[2] * f), info.w)), Math.max(y0 + 1, Math.min(Math.round(c[3] * f), info.h))]
}

/** eraser strokes of the same design at `f` times the size */
function scaleStrokes(strokes, f) {
  const r2 = (v) => Math.round(v * 100) / 100
  return (strokes || []).map((st) => ({ ...st, r: Math.min(5000, Math.max(0.5, r2(st.r * f))), pts: st.pts.map(([x, y]) => [r2(x * f), r2(y * f)]) }))
}

export default function App() {
  const [img, setImg] = useState(null) // {name, w, h, has_alpha, preview}
  const [s, setS] = useState(DEFAULTS)
  const [res, setRes] = useState(null)
  const [busy, setBusy] = useState(false)
  const [err, setErr] = useState('')
  const [toast, setToast] = useState('')
  const [mode, setMode] = useState('hasil') // asli | latar | hasil | split
  const [backdrop, setBackdrop] = useState('shirt') // grid = transparent (checkerboard) | shirt
  const [cropping, setCropping] = useState(false)
  // eraser tool: size = brush diameter in source image pixels
  const [tab, setTab] = useState('desain') // desain | batch | gang
  // the start page (DTF or sablon); skipped when the choice is remembered (config)
  const [home, setHome] = useState(true)
  const [rememberMode, setRememberMode] = useState(false)
  const [filmView, setFilmView] = useState(null) // sablon: film index shown, null = simulated print
  const [sec, setSec] = useState('gambar') // side panel section of the Desain tab
  const tabRef = useRef(tab)
  tabRef.current = tab
  const upd = useUpdater()
  // the result of "cek update" (the toolbar button has no room for text)
  useEffect(() => {
    if (upd.st.phase === 'terbaru') flash(L(`StudioZea v${upd.version} sudah versi terbaru.`, `StudioZea v${upd.version} is up to date.`))
    else if (upd.st.phase === 'gagal' && !upd.st.version) flash(upd.st.msg)
  }, [upd.st.phase])
  const [gangItems, setGangItems] = useState([])
  const [gangPins, setGangPins] = useState([]) // designs placed by hand: {path, sheet, x, y, rot}
  const gangAddRef = useRef(null) // GangSheet's "add these PNG paths"
  // gang sheet export progress lives here so switching tabs does not lose it
  const [gangRun, setGangRun] = useState(null) // {sheet, sheets, frac} while writing
  const [gangErr, setGangErr] = useState('')
  useEffect(() => {
    const uns = []
    let live = true
    const on = (name, f) => listen(name, f).then((u) => (live ? uns.push(u) : u()))
    on('gang-progress', (e) => setGangRun(e.payload))
    on('gang-done', (e) => {
      const d = e.payload
      setGangRun(null)
      const saved = d.paths.length ? L(` Tersimpan: ${d.paths.join(', ')}`, ` Saved: ${d.paths.join(', ')}`) : ''
      if (d.error) {
        setGangErr(d.error)
        flash(L(`Gang sheet gagal: ${d.error}.${saved}`, `Gang sheet failed: ${d.error}.${saved}`))
      } else if (d.cancelled) flash(L(`Gang sheet dihentikan.${saved}`, `Gang sheet stopped.${saved}`))
      else {
        setGangErr('')
        flash(L(`Gang sheet tersimpan: ${d.paths.join(', ')}`, `Gang sheet saved: ${d.paths.join(', ')}`))
      }
    })
    return () => {
      live = false
      uns.forEach((u) => u())
    }
  }, [])
  const [brushOn, setBrushOn] = useState(false)
  const [brush, setBrush] = useState({ mode: 'hapus', size: 40 })
  const [showText, setShowText] = useState(false) // hatch the "Lindungi teks" areas in the preview
  const hatchRef = useRef(false)
  const [prog, setProg] = useState(null) // {gen, rows, h}
  const [done, setDone] = useState(null) // {gen, ms, checks}
  const [ov, setOv] = useState({}) // kind -> canvas
  const [view, setView] = useState({ zoom: 1, x: null, y: null })
  const viewer = useRef(null)
  const reqId = useRef(0)
  const genRef = useRef(0)
  const toastT = useRef(0)
  // a message under the panel; `act` = {label, run}: a button in it (e.g. Undo)
  const [toastAct, setToastAct] = useState(null)
  const flash = (msg, act = null) => {
    clearTimeout(toastT.current)
    setToast(msg)
    setToastAct(act)
    toastT.current = setTimeout(() => (setToast(''), setToastAct(null)), act ? 15000 : 9000)
  }
  const set = (patch) => setS((o) => ({ ...o, ...patch }))
  const toastEl = toast ? (
    <p className="toast">
      {toast}
      {toastAct && (
        <> <button className="link" type="button" onClick={() => { toastAct.run(); setToast(''); setToastAct(null) }}>{toastAct.label}</button></>
      )}
    </p>
  ) : null
  const [imgPath, setImgPath] = useState(null)
  const imgRef = useRef(img)
  imgRef.current = img
  // AI upscale: progress while running; the original of the open AI result (to go back)
  const [ai, setAi] = useState(null) // {k, done, total}
  const [aiFrom, setAiFrom] = useState(null) // {path, k, result}
  const [aiDev, setAiDev] = useState(null) // {state: belum | gpu | cpu, name, reason}
  const refreshAiDev = () => invoke('ai_device').then(setAiDev).catch(() => {})
  useEffect(() => {
    refreshAiDev()
  }, [])
  useEffect(() => {
    let u = null
    let live = true
    listen('ai-progress', (e) => setAi((a) => (a ? { ...a, ...e.payload } : a))).then((f) => (live ? (u = f) : f()))
    return () => {
      live = false
      if (u) u()
    }
  }, [])
  const [presets, setPresets] = useState([]) // [{name, settings}]
  const [myInks, setMyInks] = useState(null) // sablon "Tinta saya": {inks, baseAfter}
  // sablon ink catalogues imported from .xlsx (brand colour charts; the standard Pantone
  // one is built in), kept in the local settings file only
  const [catalogs, setCatalogs] = useState([])
  const allCats = useMemo(() => [STD, ...catalogs], [catalogs])
  const activeCat = allCats.find((c) => c.id === s.sepKatalog) || STD
  const catPalette = useMemo(() => catalogPalette(activeCat), [activeCat])
  const xlsxRef = useRef(null)
  const [inkPick, setInkPick] = useState(null) // ink row with the catalogue picker open
  const [pickCatId, setPickCatId] = useState(null) // the picker's catalogue ('*' = all)
  const [pickQ, setPickQ] = useState('')
  const myInksRef = useRef(null)
  myInksRef.current = myInks
  // window colour theme (interface only; kept in the config and in localStorage)
  const [theme, setTheme] = useState(() => {
    try {
      const t = localStorage.getItem('sz-theme')
      return THEMES.some(([v]) => v === t) ? t : 'otomatis'
    } catch {
      return 'otomatis'
    }
  })
  // interface language (English by default); setLang re-renders through this state
  const [lang, setLangState] = useState(getLang)
  const chooseLang = (v) => {
    setLang(v)
    setLangState(v)
  }
  useEffect(() => {
    applyTheme(theme)
    if (theme !== 'otomatis') return undefined
    return onSystemTheme(() => applyTheme('otomatis'))
  }, [theme])
  const loaded = useRef(false)

  // ---------------------------------------------------------------- undo / redo of the settings
  // every change of `s` is a step; changes within 400 ms (dragging, typing) are one step
  const hist = useRef({ past: [], future: [], last: DEFAULTS, t: 0, fresh: false })
  const [histN, setHistN] = useState([0, 0])
  const freshHistory = () => (hist.current.fresh = true) // the next settings are a new start (new image, project, saved config)
  useEffect(() => {
    const h = hist.current
    if (h.fresh) {
      Object.assign(h, { past: [], future: [], last: s, t: 0, fresh: false })
      setHistN([0, 0])
      return
    }
    if (s === h.last) return
    // nothing changed, or only the batch / gang sheet options: not an undo step
    if (sameExcept(s, h.last, NO_HISTORY)) {
      h.last = s
      return
    }
    const now = Date.now()
    if (now - h.t > 400 || !h.past.length) {
      h.past.push(h.last)
      if (h.past.length > 100) h.past.shift()
    }
    h.future = []
    h.last = s
    h.t = now
    setHistN([h.past.length, 0])
  }, [s])
  // a restored step keeps the current batch / gang sheet options
  const keepOptions = (snap, now) => ({ ...snap, ...Object.fromEntries(Object.keys(now).filter(NO_HISTORY).map((k) => [k, now[k]])) })
  const undo = () => {
    const h = hist.current
    if (!h.past.length) return
    h.future.push(h.last)
    h.last = keepOptions(h.past.pop(), h.last)
    h.t = 0
    setS(h.last)
    setHistN([h.past.length, h.future.length])
  }
  const redo = () => {
    const h = hist.current
    if (!h.future.length) return
    h.past.push(h.last)
    h.last = keepOptions(h.future.pop(), h.last)
    h.t = 0
    setS(h.last)
    setHistN([h.past.length, h.future.length])
  }

  // ---------------------------------------------------------------- remembered settings + own presets
  useEffect(() => {
    invoke('load_config')
      .then((text) => {
        if (!text) return
        const cfg = JSON.parse(text)
        freshHistory()
        setS((o) => mergeSettings(o, cfg.settings, LOOK_KEYS))
        if (THEMES.some(([v]) => v === cfg.theme)) setTheme(cfg.theme)
        if (cfg.lang === 'id' || cfg.lang === 'en') chooseLang(cfg.lang)
        if (cfg.startMode === 'dtf' || cfg.startMode === 'sablon') {
          setS((o) => ({ ...o, printMode: cfg.startMode }))
          setRememberMode(true)
          setHome(false)
        }
        const mine = cfg.myInks && okInks(cfg.myInks.inks) && cfg.myInks.inks.length && Number.isInteger(cfg.myInks.baseAfter)
          ? { inks: cfg.myInks.inks, baseAfter: Math.min(Math.max(cfg.myInks.baseAfter, 0), cfg.myInks.inks.length) } : null
        setMyInks(mine)
        setCatalogs(okCatalogs(cfg.katalog))
        // "Tinta saya" remembered: its inks (they are not part of the remembered look)
        setS((o) => (o.sepSet !== 'saya' ? o : mine ? { ...o, sepInks: mine.inks, sepBaseAfter: mine.baseAfter } : { ...o, sepSet: 'auto' }))
        if (Array.isArray(cfg.presets)) {
          setPresets(cfg.presets.filter((p) => p && typeof p.name === 'string' && p.name.trim() && p.settings && typeof p.settings === 'object').slice(0, 100))
        }
      })
      .catch(() => {}) // damaged file: start with the defaults
      .finally(() => (loaded.current = true))
  }, [])
  useEffect(() => {
    if (!loaded.current) return undefined
    const t = setTimeout(() => {
      const text = JSON.stringify({ app: 'StudioZea', version: 1, settings: pickSettings(s, LOOK_KEYS), presets, theme, lang, startMode: rememberMode ? s.printMode : null, myInks, katalog: catalogs }, null, 1)
      invoke('save_config', { text }).catch(() => {})
    }, 600)
    return () => clearTimeout(t)
  }, [s, presets, theme, lang, rememberMode, myInks, catalogs])

  // ---------------------------------------------------------------- open image
  const sRef = useRef(s)
  sRef.current = s
  // the image file that is open now; `s._src` is the one the settings belong to
  // ({path, ai}), so undo / redo across an AI upscale opens the other file again
  const srcPath = useRef(null)
  const [lostSrc, setLostSrc] = useState(null)
  // opts.autoShirt === false: keep the shirt (a project brings its own)
  // opts.scale: the same design at another size (AI result / back to the original):
  // keep every setting, crop and eraser strokes scaled along, as one undo step
  // opts.ai: the original of an AI result {path, k, result}
  // opts.restore: undo / redo brings this file back; the settings are already right
  const openPath = useCallback(async (path, opts = {}) => {
    setErr('')
    try {
      const info = await invoke('open_image', { path })
      srcPath.current = path
      setHome(false)
      setImg(info)
      setImgPath(path)
      setRes(null)
      setDone(null)
      setProg(null)
      const o = unpack(await invoke('overview', { kind: 'orig' }))
      setOv(o ? { orig: o.canvas } : {})
      // the shirt that suits the design: the colour of its flat background
      const f = opts.scale
      const _src = { path, ai: opts.ai || null }
      setAiFrom(_src.ai)
      if (opts.restore) return true
      setLostSrc(null)
      if (f) {
        hist.current.t = 0 // its own step
        setS((o) => ({ ...o, crop: scaleCrop(o.crop, f, info), erase: scaleStrokes(o.erase, f), _src }))
        return true
      }
      const before = { shirtType: sRef.current.shirtType, shirtColor: sRef.current.shirtColor }
      let shirt = opts.autoShirt !== false && sRef.current.autoShirt && info.background ? shirtFor(info.background) : null
      // already that shirt: nothing changes, no message
      if (shirt && shirt.shirtType === before.shirtType && (shirt.shirtType !== 'warna' || shirt.shirtColor === before.shirtColor)) shirt = null
      freshHistory()
      setS((o) => ({ ...o, ...(shirt || {}), crop: null, erase: [], rotate: 0, mirror: false, bgTolerance: null, bgPockets: null, ...(o.sepSet === 'saya' && myInksRef.current ? { sepInks: myInksRef.current.inks, sepBaseAfter: myInksRef.current.baseAfter } : { sepInks: [], sepBaseAfter: 0, ...(o.sepSet === 'saya' ? { sepSet: 'auto' } : {}) }), _src }))
      setMode('hasil')
      if (shirt) {
        const name = shirt.shirtType === 'hitam' ? L('Hitam', 'Black') : shirt.shirtType === 'putih' ? L('Putih', 'White') : L(`Warna ${shirt.shirtColor}`, `Colour ${shirt.shirtColor}`)
        const was = before.shirtType === 'hitam' ? L('hitam', 'black') : before.shirtType === 'putih' ? L('putih', 'white') : before.shirtColor
        flash(L(`Kaos otomatis: ${name} (sama dengan warna latar desain). Bisa diganti di tab Kaos, atau dimatikan di sana.`, `Auto shirt: ${name} (matches the design background). Change it in the Shirt tab, or turn it off there.`),
          { label: L(`Batalkan (kembali ke kaos ${was})`, `Undo (back to the ${was} shirt)`), run: () => set(before) })
      }
      return true
    } catch (e) {
      setErr(String(e))
      flash(String(e))
      return false
    }
  }, [])

  // ---------------------------------------------------------------- AI upscale
  const effDpi = img && res?.effective_dpi ? res.effective_dpi : null
  const lowRes = !!effDpi && effDpi < LOW_DPI
  const aiFits = (k) => !!img && img.w * k * img.h * k <= AI_MAX_PX
  const aiK = [aiScaleFor(effDpi, s.dpi), 2].find(aiFits) || null
  const runAi = async (k = aiK) => {
    const from = { id: img?.id, path: imgPath }
    if (!img || !imgPath || ai || !k) return
    setErr('')
    setAi({ k, done: 0, total: 0 })
    try {
      const r = await invoke('ai_upscale', { id: from.id, path: from.path, scale: k, denoise: AI_DENOISE[s.aiHalus] ?? 0.5, device: s.aiDevice })
      setAi(null)
      refreshAiDev()
      if (imgRef.current?.id !== from.id) {
        flash(L('Hasil AI siap, tapi gambar sudah berganti. Klik Perbesar lagi pada gambar itu untuk memakainya.', 'The AI result is ready, but another image is open now. Click Upscale on that image again to use it.'))
        return
      }
      // a second upscale of a result still goes back to the very first original
      const prev = aiFrom && aiFrom.result === from.path ? aiFrom : null
      const orig = prev ? { path: prev.path, k: prev.k * k, result: r.path } : { path: from.path, k, result: r.path }
      if (await openPath(r.path, { scale: k, ai: orig })) {
        const how = r.cached ? L('dari cache', 'from the cache') : L(`dalam ${fmt(r.ms / 1000, 1)} detik (${r.device})`, `in ${fmt(r.ms / 1000, 1)} s (${r.device})`)
        flash(L(`Diperbesar ${k}× dengan AI ${how}: ${r.w}×${r.h} px. Ctrl Z kembali ke gambar asli; file baru dibuat saat proyek disimpan.`, `AI upscaled ${k}× ${how}: ${r.w}×${r.h} px. Ctrl Z goes back to the original; a file is written when you save the project.`))
      }
    } catch (e) {
      setAi(null)
      refreshAiDev()
      const m = String(e)
      if (m === 'dibatalkan') flash(L('Perbesar AI dibatalkan.', 'AI Upscale cancelled.'))
      else {
        setErr(`${L('Perbesar AI', 'AI Upscale')}: ${m}`)
        flash(`${L('Perbesar AI', 'AI Upscale')}: ${m}`)
      }
    }
  }
  const backToOriginal = async () => {
    if (!aiFrom || ai) return
    const { path, k } = aiFrom
    if (await openPath(path, { scale: 1 / k })) flash(L(`Kembali ke gambar asli: ${path}`, `Back to the original: ${path}`))
  }
  // undo / redo crossed an AI step: open the image those settings belong to
  useEffect(() => {
    const want = s._src
    if (!want || want.path === srcPath.current) return
    openPath(want.path, { restore: true, ai: want.ai }).then((ok) => {
      if (!ok) setLostSrc(want.path) // moved / deleted: the message is shown, keep working on the open one
      // quick undo + redo: the last step wins
      const now = sRef.current._src
      if (now && now.path !== srcPath.current) openPath(now.path, { restore: true, ai: now.ai })
    })
  }, [s._src, openPath])

  // ---------------------------------------------------------------- projects (.szproj)
  const saveProject = async () => {
    if (!imgPath) return
    const base = (img?.name || 'desain').replace(/\.[^.]+$/, '')
    const p = await save({ defaultPath: `${base}.szproj`, filters: PROJ_FILTERS() })
    if (!p) return
    const path = /\.szproj$/i.test(p) ? p : `${p}.szproj`
    try {
      // an AI result lives in the app cache: now it becomes a file next to the project
      const image = await invoke('keep_ai_image', { path: imgPath, project: path })
      const original = aiFrom ? { from: aiFrom.path, k: aiFrom.k } : undefined
      const text = JSON.stringify({ app: 'StudioZea', version: 1, image, ai: original, settings: pickSettings(s, PROJECT_KEYS) }, null, 1)
      await invoke('save_project', { path, text })
      flash(image === imgPath ? L(`Proyek tersimpan: ${path}`, `Project saved: ${path}`) : L(`Proyek tersimpan: ${path} (hasil AI: ${image})`, `Project saved: ${path} (AI result: ${image})`))
    } catch (e) {
      setErr(String(e))
    }
  }
  const openProject = useCallback(async (path) => {
    setErr('')
    try {
      const proj = JSON.parse(await invoke('load_project', { path }))
      if (!proj || proj.app !== 'StudioZea') throw new Error('NOT_PROJECT')
      const image = typeof proj.image === 'string' ? await invoke('find_project_image', { project: path, image: proj.image }) : null
      // an AI result remembers its original ("use the original")
      const from = image && proj.ai && typeof proj.ai.from === 'string' && (proj.ai.k === 2 || proj.ai.k === 4 || proj.ai.k === 8 || proj.ai.k === 16)
        ? await invoke('find_project_image', { project: path, image: proj.ai.from }) : null
      const orig = from ? { path: from, k: proj.ai.k, result: image } : null
      if (image && !(await openPath(image, { autoShirt: false, ai: orig }))) return // the message is already shown
      freshHistory()
      setS({ ...mergeSettings(DEFAULTS, proj.settings, PROJECT_KEYS), _src: image ? { path: image, ai: orig } : sRef.current._src })
      if (!image) {
        const msg = L(`Gambar proyek tidak ditemukan: ${proj.image}. Pengaturan sudah dimuat; buka gambarnya lewat "Buka gambar…".`, `Project image not found: ${proj.image}. The settings are loaded; open the image with "Open image…".`)
        setErr(msg)
        flash(msg)
      }
      else flash(L(`Proyek dibuka: ${path}`, `Project opened: ${path}`))
    } catch (e) {
      const msg = e instanceof SyntaxError || String(e.message || e) === 'NOT_PROJECT'
        ? L('File proyek rusak atau bukan file proyek StudioZea (.szproj).', 'Damaged file or not a StudioZea project (.szproj).')
        : L(`Proyek tidak bisa dibuka: ${e.message || e}`, `Cannot open the project: ${e.message || e}`)
      setErr(msg)
      flash(msg)
    }
  }, [openPath])
  const pickProject = async () => {
    const p = await open({ multiple: false, filters: PROJ_FILTERS() })
    if (p) openProject(Array.isArray(p) ? p[0] : p)
  }

  // ---------------------------------------------------------------- own presets
  const [presetSel, setPresetSel] = useState('')
  const [presetName, setPresetName] = useState(null) // null = not naming
  const applyOwnPreset = (name) => {
    const p = presets.find((x) => x.name === name)
    setPresetSel(name)
    if (p) setS((o) => mergeSettings(o, p.settings, LOOK_KEYS))
  }
  const saveOwnPreset = () => {
    const name = (presetName || '').trim().slice(0, 60)
    if (!name) return
    const entry = { name, settings: pickSettings(s, LOOK_KEYS) }
    setPresets((ps) => [...ps.filter((x) => x.name !== name), entry].sort((a, b) => a.name.localeCompare(b.name)))
    setPresetSel(name)
    setPresetName(null)
    flash(L(`Preset "${name}" tersimpan.`, `Preset "${name}" saved.`))
  }
  const deleteOwnPreset = (name) => {
    setPresets((ps) => ps.filter((x) => x.name !== name))
    flash(L(`Preset "${name}" dihapus.`, `Preset "${name}" deleted.`))
    if (presetSel === name) setPresetSel('')
  }
  const resetSettings = () => {
    setS((o) => ({ ...DEFAULTS, ...pickSettings(o, IMAGE_KEYS), _src: o._src }))
    setPresetSel('')
    flash(L('Pengaturan kembali ke default.', 'Settings reset to default.'))
  }

  const pick = async () => {
    const p = await open({ multiple: false, filters: IMG_FILTERS() })
    if (p) openPath(Array.isArray(p) ? p[0] : p)
  }

  useEffect(() => {
    let un
    try {
      getCurrentWebview()
        .onDragDropEvent((ev) => {
          if (ev.payload.type === 'drop' && ev.payload.paths?.length) {
            if (tabRef.current === 'batch') {
              const ims = ev.payload.paths.filter((x) => /\.(png|jpe?g|webp|tiff?|bmp)$/i.test(x))
              if (ims.length) setBatch((b) => (b.running ? b : { ...b, files: [...new Set([...b.files, ...ims])].sort((x, y) => x.localeCompare(y)), dir: b.dir || ims[0].replace(/[\\/][^\\/]*$/, ''), log: [], done: null }))
              return
            }
            if (tabRef.current === 'gang') {
              const ims = ev.payload.paths.filter((x) => /\.(png|jpe?g|webp|tiff?|bmp)$/i.test(x))
              if (ims.length && gangAddRef.current) gangAddRef.current(ims)
              else flash(L('Di Gang sheet hanya gambar (PNG, JPG, TIFF, WebP) yang bisa ditambahkan.', 'Only images (PNG, JPG, TIFF, WebP) can be added to a gang sheet.'))
              return
            }
            const p = ev.payload.paths[0]
            if (/\.szproj$/i.test(p)) openProject(p)
            else openPath(p)
          }
        })
        .then((f) => (un = f))
    } catch (_) {}
    return () => un && un()
  }, [openPath, openProject])

  // ---------------------------------------------------------------- process (debounced)
  // re-render only when the job itself changes (not for margin, units, batch / gang options…)
  const jobKey = useMemo(() => JSON.stringify(toJob(s)), [s])
  const job = useMemo(() => JSON.parse(jobKey), [jobKey])
  // undo / redo across an AI step: wait until the image of these settings is open
  const otherSrc = !!s._src && s._src.path !== imgPath && s._src.path !== lostSrc
  // DTF <-> sablon: the image, crop and strokes stay; batch / gang sheet are DTF only
  const chooseMode = (m) => {
    set({ printMode: m })
    setHome(false)
    if (m === 'sablon') {
      setTab('desain')
      if (sec === 'halftone') setSec('warna')
    } else if (sec === 'warna') setSec('halftone')
    setFilmView(null)
  }
  // sablon: the separation settings take part in the result key
  const sablon = s.printMode === 'sablon'
  const sablonRef = useRef(sablon)
  sablonRef.current = sablon
  const sepKey = useMemo(() => JSON.stringify(toSep(s, catPalette)), [s, catPalette])
  // ink names: the catalogue ink chosen by hand, else the catalogue ink with exactly this
  // colour (inks chosen from a catalogue), else the nearest common ink colour
  const catInk = (rgb) => findInk(rgb, allCats, activeCat.id)
  const inkLabel = (rgb, role, name) => {
    if (role === 'underbase' || role === 'highlight') {
      const w = s.sepSet === 'katalog' ? catInk(rgb) : null
      const r = L(...inkName(rgb, role))
      return w && w.cat.id !== STD_ID ? `${r} · ${inkTitle(w.ink, w.cat)}` : r
    }
    if (name) return name
    const c = catInk(rgb)
    return c ? inkLine(c.ink, c.cat) : L(...inkName(rgb, role))
  }
  const filmLabel = (f) => {
    const own = f.src != null && s.sepInks[f.src] && s.sepInks[f.src].rgb.every((v, k) => v === f.rgb[k]) ? s.sepInks[f.src].name : undefined
    return inkLabel(f.rgb, f.role, own)
  }
  const procKey = sablon ? `${jobKey}|${sepKey}` : jobKey
  useEffect(() => {
    if (!img || otherSrc) return
    const id = ++reqId.current
    const t = setTimeout(async () => {
      setBusy(true)
      setErr('')
      try {
        if (sablon) {
          // the whole separation at once (films + simulated print)
          const r = await invoke('sep_process', { job, sep: JSON.parse(sepKey) })
          if (id === reqId.current) {
            genRef.current = r.gen
            setRes({ ...r, key: procKey, job, sablon: true })
            setDone({ gen: r.gen, ms: r.ms, checks: null })
            setProg(null)
            setFilmView((v) => (v != null && v < r.films.length ? v : null))
            setOv((old) => ({ orig: old.orig }))
            refreshOverview(r.gen, ['sep', 'source'])
          }
          return
        }
        const r = await invoke('process', { job })
        if (id === reqId.current) {
          genRef.current = r.gen
          setRes({ ...r, key: procKey, job })
          setDone(null)
          setProg({ gen: r.gen, rows: 0, h: r.h })
          setOv((old) => ({ ...old, text: null }))
          refreshOverview(r.gen, ['source', 'result', ...(hatchRef.current ? ['text'] : [])])
        }
      } catch (e) {
        if (id === reqId.current) setErr(String(e))
      } finally {
        if (id === reqId.current) setBusy(false)
      }
    }, sablon ? 500 : 350)
    return () => clearTimeout(t)
  }, [job, img, otherSrc, sablon, sablon ? sepKey : ''])

  // ---------------------------------------------------------------- progressive render
  // overview refreshes are serialised; kinds asked for while one runs are queued
  const ovBusy = useRef(false)
  const ovQueue = useRef(new Set())
  const refreshOverview = useCallback(async (gen, kinds) => {
    kinds.forEach((k) => ovQueue.current.add(k))
    if (ovBusy.current) return
    ovBusy.current = true
    try {
      while (ovQueue.current.size) {
        const ks = [...ovQueue.current]
        ovQueue.current.clear()
        const got = {}
        for (const k of ks) {
          const o = unpack(await invoke(sablonRef.current ? 'sep_overview' : 'overview', { kind: k }))
          if (o && o.gen === genRef.current) got[k] = o.canvas
        }
        setOv((old) => ({ ...old, ...got }))
      }
    } catch (_) {
    } finally {
      ovBusy.current = false
    }
  }, [])
  useEffect(() => {
    const uns = []
    let last = 0
    listen('render-progress', (e) => {
      const p = e.payload
      if (p.gen !== genRef.current) return
      setProg(p)
      const now = performance.now()
      if (now - last > 900) {
        last = now
        refreshOverview(p.gen, ['result'])
      }
    }).then((u) => uns.push(u))
    listen('render-done', (e) => {
      const d = e.payload
      if (d.gen !== genRef.current) return
      setDone(d)
      setProg({ gen: d.gen, rows: d.h, h: d.h })
      refreshOverview(d.gen, ['result'])
    }).then((u) => uns.push(u))
    listen('render-error', (e) => {
      if (e.payload.gen === genRef.current) setErr(String(e.payload.msg))
    }).then((u) => uns.push(u))
    return () => uns.forEach((u) => u())
  }, [refreshOverview])

  const fetchTile = useCallback(async (kind, level, tx, ty) => {
    const o = unpack(await invoke(kind !== 'orig' && sablonRef.current ? 'sep_tile' : 'tile', { kind, level, tx, ty }))
    // only complete tiles are cached by the viewer; partial ones are re-requested
    if (!o || (kind !== 'orig' && o.gen !== genRef.current)) return null
    return o.rows >= o.h ? o.canvas : null
  }, [])
  const fetchOrig = useCallback((k, lv, x, y) => fetchTile('orig', lv, x, y), [fetchTile])
  // sablon: the result layer is the simulated print or one film
  const sepKind = filmView == null ? 'sep' : `film${filmView}`
  const fetchSep = useCallback((k, lv, x, y) => fetchTile(k === 'result' ? sepKind : k, lv, x, y), [fetchTile, sepKind])
  useEffect(() => {
    if (sablon && res?.sablon && sepKind !== 'sep' && !ov[sepKind]) refreshOverview(res.gen, [sepKind])
  }, [sablon, sepKind, res?.gen])
  const busyAll = busy || (prog && prog.rows < prog.h)
  // the shown result belongs to the current settings and is fully rendered
  const current = !!(res && res.key === procKey && !!res.sablon === sablon)
  const canExport = current && done && done.gen === res.gen && !busyAll

  // ---------------------------------------------------------------- export
  // sablon: the films (one PNG per ink, black on white with registration marks), the
  // simulated print and the print order, all next to the chosen file name
  const filmNames = () => (res?.films || []).map(filmLabel)
  const exportFilms = async () => {
    if (!canExport || !res?.sablon) return
    const base = (img?.name || 'desain').replace(/\.[^.]+$/, '')
    const p = await save({ defaultPath: `${base}_sablon_${res.films.length}film.png`, filters: [{ name: 'PNG', extensions: ['png'] }] })
    if (!p) return
    try {
      const files = await invoke('sep_export', { path: p, names: filmNames(), marginMm: Number(s.sepMargin) || 0 })
      const dir = files[0].replace(/[\\/][^\\/]*$/, '')
      flash(L(`Tersimpan ${files.length - 2} film + hasil simulasi + urutan cetak di ${dir}`, `Saved ${files.length - 2} films + simulated print + print order in ${dir}`))
    } catch (e) {
      setErr(String(e))
    }
  }
  const exportPng = async () => {
    if (!canExport) return
    if (res?.sablon) return exportFilms()
    const js = res.job.settings
    const pat = typeof js.pattern === 'string' ? js.pattern : `grain-${js.pattern.grain}`
    const base = (img?.name || 'desain').replace(/\.[^.]+$/, '')
    const name = `${base}_${res.width_cm.toFixed(0)}x${res.height_cm.toFixed(0)}cm_${res.dpi}dpi_${pat === 'am' ? js.lpi + 'lpi-' + js.shape : pat}.png`
    const p = await save({ defaultPath: name, filters: [{ name: 'PNG', extensions: ['png'] }] })
    if (!p) return
    try {
      const r = await invoke('export_png', { path: p, marginMm: Number(s.marginMm) || 0 })
      flash(L(`Tersimpan: ${r.path} (${(r.bytes / 1e6).toFixed(1)} MB, ${r.w}×${r.h} px). Cek: semi-transparan ${r.checks.semi}, titik kecil ${r.checks.tiny}.`, `Saved: ${r.path} (${(r.bytes / 1e6).toFixed(1)} MB, ${r.w}×${r.h} px). Check: semi-transparent ${r.checks.semi}, tiny dots ${r.checks.tiny}.`))
    } catch (e) {
      setErr(String(e))
    }
  }

  // ---------------------------------------------------------------- eraser
  // the viewer works in source pixels (Asli) or print pixels (other views); strokes are
  // stored in source pixels so they stay on the design whatever the crop, trim or size
  const asliView = mode === 'asli' || !res
  const box = res?.src_box
  // the print shows the design turned / mirrored (res.job): oriented size of the source box
  const ori = { rot: res?.job?.rotate || 0, mir: !!res?.job?.mirror }
  const [bw, bh] = box ? [box[2] - box[0], box[3] - box[1]] : [0, 0]
  const [ow, oh] = ori.rot % 180 === 90 ? [bh, bw] : [bw, bh]
  const printScale = box ? [ow / res.w, oh / res.h] : null // source px per print px
  const viewToSrc = ([x, y]) => {
    if (asliView) return [x, y]
    // undo the mirror (of the turned design), then the clockwise turn
    const u = ori.mir ? ow - x * printScale[0] : x * printScale[0]
    const v = y * printScale[1]
    const [px, py] = ori.rot === 90 ? [v, bh - u] : ori.rot === 180 ? [bw - u, bh - v] : ori.rot === 270 ? [bw - v, u] : [u, v]
    return [box[0] + px, box[1] + py]
  }
  const addStroke = (pts) => {
    const r2 = (v) => Math.round(v * 100) / 100
    const stroke = { mode: brush.mode, r: brush.size / 2, pts: pts.map((q) => viewToSrc(q).map(r2)) }
    hist.current.t = 0 // every stroke is its own undo step
    setS((o) => ({ ...o, erase: [...(o.erase || []), stroke] }))
  }
  const brushView = brushOn && img && !cropping ? { r: brush.size / 2 / (asliView ? 1 : printScale[0]), mode: brush.mode, onStroke: addStroke } : null
  // turn by +-90 keeping the printed size of the design (width <- height)
  // the printed size stays: a turn by 90° makes the old height the new width.
  // a0 = height / width of the design unturned (from the last render, whatever its turn)
  const orient = (next) => {
    const a0 = res ? ((res.job?.rotate || 0) % 180 === 0 ? res.height_cm / res.width_cm : res.width_cm / res.height_cm) : null
    setS((o) => {
      const r0 = o.rotate || 0
      const { rotate, mirror } = next(o)
      const r1 = ((rotate % 360) + 360) % 360
      let widthCm = o.widthCm
      if (a0 && Number(o.widthCm) > 0 && r0 % 180 !== r1 % 180) widthCm = Math.round(o.widthCm * (r0 % 180 === 0 ? a0 : 1 / a0) * 1e6) / 1e6
      return { ...o, rotate: r1, mirror, widthCm }
    })
  }
  const turn = (d) => orient((o) => ({ rotate: (o.rotate || 0) + d, mirror: !!o.mirror }))
  // the text brushes exist only with "Lindungi teks"
  const textBrushes = !!s.protectText && s.halftoneOn
  useEffect(() => {
    if (!textBrushes && isTextMode(brush.mode)) {
      setBrush((b) => ({ ...b, mode: 'hapus' }))
      setBrushOn(false)
    }
  }, [textBrushes, brush.mode])
  // two brush tools share the brush: the eraser (hapus / kembalikan, Image tab) and the
  // text brush (teks / bukan-teks, Halftone tab, with "Lindungi teks")
  const textTool = isTextMode(brush.mode)
  const pickBrush = (text) => {
    if (!img || (text && !textBrushes)) return
    setCropping(false)
    if (brushOn && textTool === text) {
      setBrushOn(false)
      return
    }
    if (textTool !== text) setBrush((b) => ({ ...b, mode: text ? 'teks' : 'hapus' }))
    setBrushOn(true)
  }
  const toggleBrush = () => pickBrush(false)
  const eraserOn = brushOn && !textTool
  const textBrushOn = brushOn && textTool
  const textStrokes = s.erase.filter((st) => isTextMode(st.mode)).length
  const eraseStrokes = s.erase.length - textStrokes
  const clearStrokes = (text) => set({ erase: s.erase.filter((st) => isTextMode(st.mode) !== text) })
  // the hatching of the protected areas: while the text brush is on, or when asked for
  const hatch = !sablon && textBrushes && (textBrushOn || showText)
  hatchRef.current = hatch
  // its overview, when the hatching is shown
  useEffect(() => {
    if (hatch && res && !ov.text) refreshOverview(res.gen, ['text'])
  }, [hatch, res?.gen])
  const sizeBrush = (f) =>
    setBrush((b) => {
      let n = Math.round(b.size * f)
      if (n === b.size) n += f > 1 ? 1 : -1
      return { ...b, size: Math.max(1, Math.min(1000, n)) }
    })
  const brushMm = printScale && res ? ` · ${((brush.size / printScale[0] / res.dpi) * 25.4).toFixed(1)} mm` : ''

  // ---------------------------------------------------------------- keyboard shortcuts
  // Ctrl+Z / Ctrl+Y (Ctrl+Shift+Z) undo/redo, Ctrl+O image, Ctrl+Shift+O project, Ctrl+S project, Ctrl+E export.
  // Zoom keys (Ctrl 0/1/+/-), Enter/Esc for crop and Space-pan live in CanvasViewer.
  const keys = useRef(null)
  keys.current = { tab, undo, redo, pick, pickProject, saveProject, exportPng, canExport, imgPath, cropping, brushOn, toggleBrush, sizeBrush, setBrushOn }
  useEffect(() => {
    const typing = (el) => el && (el.isContentEditable || el.tagName === 'TEXTAREA' ||
      (el.tagName === 'INPUT' && !['range', 'checkbox', 'radio', 'button'].includes(el.type)))
    const kd = (e) => {
      const k = keys.current
      if (k.tab !== 'desain') return
      const key = e.key.toLowerCase()
      if (!(e.ctrlKey || e.metaKey || e.altKey)) {
        // eraser: E on/off, [ ] size, Esc done (like Photoshop)
        if (typing(e.target) || e.target?.tagName === 'SELECT') return
        if (key === 'e' && !e.repeat) k.toggleBrush()
        else if (e.key === '[') k.sizeBrush(1 / 1.2)
        else if (e.key === ']') k.sizeBrush(1.2)
        else if (e.key === 'Escape' && k.brushOn && !k.cropping) k.setBrushOn(false)
        return
      }
      if (e.altKey) return
      if (key === 'z' || key === 'y') {
        if (typing(e.target)) return // the text field's own undo
        e.preventDefault()
        if (k.cropping) return
        if (key === 'y' || e.shiftKey) k.redo()
        else k.undo()
        return
      }
      const act = key === 'o' ? (e.shiftKey ? 'pickProject' : 'pick') : key === 's' && !e.shiftKey ? 'saveProject' : key === 'e' && !e.shiftKey ? 'exportPng' : null
      if (!act) return
      e.preventDefault()
      if (e.repeat) return
      // a field being typed in commits on blur; run after that has re-rendered
      if (typing(document.activeElement)) document.activeElement.blur()
      setTimeout(() => {
        const k2 = keys.current
        if (act === 'saveProject' && !k2.imgPath) flash(L('Buka gambar dulu, baru proyek bisa disimpan.', 'Open an image first, then save the project.'))
        else if (act === 'exportPng' && !k2.canExport) flash(L('Belum bisa export: tunggu render selesai (atau buka gambar dulu).', 'Cannot export yet: wait for the render (or open an image first).'))
        else k2[act]()
      }, 0)
    }
    window.addEventListener('keydown', kd)
    return () => window.removeEventListener('keydown', kd)
  }, [])

  // ---------------------------------------------------------------- batch export
  // many images, the same settings; background automatic per image, no crop / eraser
  const [batch, setBatch] = useState({ files: [], dir: null, running: false, cur: null, log: [], done: null })
  useEffect(() => {
    const uns = []
    listen('batch-item', (e) => {
      const it = e.payload
      setBatch((b) => ({ ...b, cur: it, log: it.state === 'ok' || it.state === 'gagal' ? [...b.log, it] : b.log }))
    }).then((u) => uns.push(u))
    listen('batch-done', (e) => {
      const d = e.payload
      setBatch((b) => ({ ...b, running: false, cur: null, done: d }))
      refreshAiDev()
      flash(d.cancelled ? L(`Export banyak dihentikan: ${d.ok} berhasil, ${d.failed} gagal.`, `Batch export stopped: ${d.ok} done, ${d.failed} failed.`) : L(`Export banyak selesai: ${d.ok} berhasil, ${d.failed} gagal.`, `Batch export finished: ${d.ok} done, ${d.failed} failed.`))
    }).then((u) => uns.push(u))
    return () => uns.forEach((u) => u())
  }, [])
  const dirOf = (p) => p.replace(/[\\/][^\\/]*$/, '')
  const pickBatchFiles = async () => {
    const p = await open({ multiple: true, filters: IMG_FILTERS() })
    if (!p) return
    const files = (Array.isArray(p) ? p : [p]).slice().sort((a, b) => a.localeCompare(b))
    setBatch((b) => ({ ...b, files, dir: b.dir || dirOf(files[0]), log: [], done: null }))
  }
  const pickBatchDir = async () => {
    const p = await open({ directory: true, multiple: false, defaultPath: batch.dir || undefined })
    if (p) setBatch((b) => ({ ...b, dir: Array.isArray(p) ? p[0] : p }))
  }
  const startBatch = async () => {
    const js = toJob(s)
    const ss = js.settings
    const pat = typeof ss.pattern === 'string' ? ss.pattern : `grain-${ss.pattern.grain}`
    const suffix = `_${ss.dpi}dpi_${ss.halftone_area === 0 ? 'tanpa-halftone' : (ss.true_dots ? 'titik-warna-' : '') + (pat === 'am' ? ss.lpi + 'lpi-' + ss.shape : pat)}`
    setBatch((b) => ({ ...b, running: true, cur: null, log: [], done: null }))
    try {
      await invoke('batch_export', { paths: batch.files, outDir: batch.dir, job: js, fitCm: s.batchFit === 'kotak' ? [Number(s.batchW), Number(s.batchH)] : null, marginMm: Number(s.marginMm) || 0, suffix, ai: s.batchAi ? AI_DENOISE[s.aiHalus] ?? 0.5 : null, aiDevice: s.aiDevice })
    } catch (e) {
      setBatch((b) => ({ ...b, running: false }))
      flash(String(e))
    }
  }

  // the open design, exported to the app cache, for the gang sheet
  const addCurrentToGang = img
    ? async () => {
        if (!canExport) throw new Error(L('Desain belum siap: tunggu render selesai di tab Desain.', 'Design not ready: wait for the render in the Design tab.'))
        const base = (img.name || 'desain').replace(/\.[^.]+$/, '')
        const path = await invoke('gang_temp_path', { name: base })
        await invoke('export_png', { path })
        return { path, name: `${base} (${fmt(res.width_cm)}×${fmt(res.height_cm)} cm)` }
      }
    : null
  const makeTestSheet = async () => {
    const kaos = s.shirtType === 'putih' ? 'putih' : 'hitam'
    const p = await save({ defaultPath: `studiozea_test-sheet_kaos-${kaos}_${s.dpi}dpi.png`, filters: [{ name: 'PNG', extensions: ['png'] }] })
    if (!p) return
    try {
      const r = await invoke('test_sheet', { kaos, dpi: Number(s.dpi), path: p })
      flash(L(`Lembar uji tersimpan: ${r.path} (22 × 30 cm). Cetak 100 %, pres, lalu cuci beberapa kali.`, `Test sheet saved: ${r.path} (22 × 30 cm). Print at 100 %, press, then wash a few times.`))
    } catch (e) {
      setErr(String(e))
    }
  }

  const applyPreset = (id) => {
    const p = PRESETS.find((x) => x.id === id)
    if (p) set(p.set)
  }

  const heightCm = res && s.widthCm ? (res.height_cm * s.widthCm) / res.width_cm : null
  // size fields in the chosen unit (stored as cm)
  const unitLabel = s.unit === 'px' ? 'px' : s.unit === 'in' ? L('inci', 'in') : 'cm'
  const fromCm = (cm) => (s.unit === 'px' ? Math.round((cm / 2.54) * s.dpi) : s.unit === 'in' ? cm / 2.54 : cm)
  const toCm = (v) => (s.unit === 'px' ? (v / s.dpi) * 2.54 : s.unit === 'in' ? v * 2.54 : v)
  const levels = current && res.screen ? res.screen.levels : Math.round((s.dpi / s.lpi) ** 2)
  const autoAngle = s.pattern === 'interlock' ? 45 : 22.5

  // ---------------------------------------------------------------- layout
  const topbar = (
    <header className="topbar">
      <button type="button" className="logo" title={L('Beranda: pilih DTF atau sablon', 'Home: choose DTF or screen printing')} onClick={() => setHome(true)}><img src={logoMark} alt="" className="logo-mark" />StudioZea</button>
      <div className="modeswitch" title={L('Jenis cetak. Pindah kapan saja: gambar, crop dan coretan tetap.', 'Print type. Switch any time: image, crop and strokes stay.')}>
        <Seg value={s.printMode} onChange={chooseMode} options={[['dtf', 'DTF'], ['sablon', L('Sablon', 'Screen print')]]} />
      </div>
      <button type="button" className="tb" onClick={pick} title={L('Buka gambar (Ctrl O)', 'Open image (Ctrl O)')}>
        <Icon name="image" /><span>{L('Buka gambar', 'Open')}</span>
      </button>
      <Menu icon="folder" label={L('Proyek', 'Project')} title={L('File proyek (.szproj): gambar + crop + semua pengaturan', 'Project file (.szproj): image + crop + all settings')}>
        <button type="button" data-close onClick={pickProject}><Icon name="folder" /><span>{L('Buka proyek…', 'Open project…')}</span><kbd>Ctrl Shift O</kbd></button>
        <button type="button" data-close disabled={!imgPath} onClick={saveProject}><Icon name="save" /><span>{L('Simpan proyek…', 'Save project…')}</span><kbd>Ctrl S</kbd></button>
      </Menu>
      <Menu icon="star" label="Preset" title={L('Preset saya: pengaturan yang disimpan dengan nama (kaos, ukuran, halftone, tekstur)', 'My presets: saved settings (shirt, size, halftone, texture)')}>
        {presets.length ? presets.map((p) => (
          <div className="mi2" key={p.name}>
            <button type="button" data-close className={presetSel === p.name ? 'cur' : ''} onClick={() => applyOwnPreset(p.name)}>
              <Icon name="star" /><span>{p.name}</span>
            </button>
            <button type="button" className="del" title={L(`Hapus preset "${p.name}"`, `Delete preset "${p.name}"`)} onClick={() => deleteOwnPreset(p.name)}><Icon name="x" size={14} /></button>
          </div>
        )) : <p className="menunote">{L('Belum ada preset. Atur semuanya, lalu simpan di bawah.', 'No presets yet. Set everything up, then save it below.')}</p>}
        <hr />
        {presetName == null ? (
          <button type="button" onClick={() => setPresetName(presetSel || '')}><Icon name="plus" /><span>{L('Simpan pengaturan sebagai preset…', 'Save settings as preset…')}</span></button>
        ) : (
          <div className="menuform">
            <input className="name" autoFocus value={presetName} maxLength={60} placeholder={L('Nama, mis. Kaos hitam vintage 30 cm', 'Name, e.g. Black shirt vintage 30 cm')}
              onChange={(e) => setPresetName(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Enter') saveOwnPreset(); else if (e.key === 'Escape') setPresetName(null) }} />
            <button className="btn primary" type="button" disabled={!presetName.trim()} onClick={saveOwnPreset}>{L('Simpan', 'Save')}</button>
          </div>
        )}
        <button type="button" data-close onClick={resetSettings}><Icon name="reset" /><span>{L('Reset ke pengaturan awal', 'Reset to default settings')}</span></button>
      </Menu>
      <span className="sep" />
      <button className="tb icon" type="button" title={L('Batalkan (Ctrl Z)', 'Undo (Ctrl Z)')} disabled={!histN[0] || cropping} onClick={undo}><Icon name="undo" /></button>
      <button className="tb icon" type="button" title={L('Ulangi (Ctrl Y)', 'Redo (Ctrl Y)')} disabled={!histN[1] || cropping} onClick={redo}><Icon name="redo" /></button>
      <span className="grow" />
      <nav className="tabs">
        {[['desain', 'brush', L('Desain', 'Design')], ...(sablon ? [] : [['batch', 'files', L('Export banyak', 'Batch')], ['gang', 'grid', 'Gang sheet']])].map(([v, ic, l]) => (
          <button key={v} type="button" className={tab === v ? 'tab on' : 'tab'} onClick={() => setTab(v)}>
            <Icon name={ic} /><span>{l}</span>
          </button>
        ))}
      </nav>
      <span className="grow" />
      <Menu icon="gear" title={L('Pengaturan: tema, bahasa, AI', 'Settings: theme, language, AI')} align="right">
        <p className="menuhead">{L('Tampilan', 'Theme')}</p>
        <div className="menuseg">
          <Seg value={theme} onChange={setTheme} options={THEMES.map(([v, id, en]) => [v, L(id, en)])} />
        </div>
        <p className="menunote">{L('Hanya warna tampilan; hasil cetak dan pratinjau tidak berubah.', 'Interface colours only; the print and preview stay the same.')}</p>
        <hr />
        <p className="menuhead">{L('Bahasa', 'Language')}</p>
        <div className="menuseg">
          <Seg value={lang} onChange={chooseLang} options={LANGS.map(([v, l]) => [v, l])} />
        </div>
        <hr />
        <p className="menuhead" title={L('Halus: bersihkan noise dan kotak-kotak JPG. Seimbang: bawaan. Tekstur: pertahankan tekstur halus / aus (desain vintage).', 'Smooth: clean JPG noise and blocks. Balanced: default. Texture: keep fine / worn texture (vintage designs).')}>{L('Kehalusan AI', 'AI style')}</p>
        <div className="menuseg">
          <Seg value={s.aiHalus} onChange={(v) => set({ aiHalus: v })} options={[['halus', L('Halus', 'Smooth')], ['seimbang', L('Seimbang', 'Balanced')], ['tekstur', L('Tekstur', 'Texture')]]} />
        </div>
        <p className="menunote">{s.aiHalus === 'halus' ? L('Bersihkan noise dan kotak-kotak JPG.', 'Cleans JPG noise and blocks.') : s.aiHalus === 'tekstur' ? L('Pertahankan tekstur halus / aus (vintage).', 'Keeps fine / worn texture (vintage).') : L('Bawaan: untuk kebanyakan gambar.', 'Default: for most images.')}</p>
        <p className="menuhead" title={L('Otomatis: kartu grafis (AMD, NVIDIA, Intel) dipakai bila ada dan lolos tes saat pertama dipakai (hasilnya dibandingkan dengan prosesor); kalau tidak ada, gagal tes, atau bermasalah di tengah jalan, AI otomatis memakai prosesor dengan hasil yang sama. Prosesor: selalu prosesor.',
          'Auto: the graphics card (AMD, NVIDIA, Intel) is used if there is one and it passes a test on first use (compared with the processor); if not, or if it fails later, the processor is used with the same result. CPU: always the processor.')}>{L('Perangkat AI', 'AI device')}</p>
        <div className="menuseg">
          <Seg value={s.aiDevice} onChange={(v) => set({ aiDevice: v })} options={[['otomatis', L('Otomatis', 'Auto')], ['cpu', 'CPU']]} />
        </div>
        <p className="menunote wrap">
          {s.aiDevice === 'cpu' ? L('Selalu prosesor.', 'Always the processor.') : !aiDev || aiDev.state === 'belum' ? L('Dicek saat pertama dipakai.', 'Checked on first use.')
            : aiDev.state === 'gpu' ? L(`Kartu grafis: ${aiDev.name}`, `Graphics card: ${aiDev.name}`)
            : /^(tidak ada kartu grafis|no usable graphics card)/.test(aiDev.reason) ? L('Prosesor (tidak ada kartu grafis)', 'Processor (no graphics card)') : L(`Prosesor (kartu grafis tidak dipakai: ${aiDev.reason})`, `Processor (graphics card not used: ${aiDev.reason})`)}
          {s.aiDevice === 'otomatis' && aiDev?.state === 'cpu' && /:/.test(aiDev.reason) && (
            <> <button className="link" type="button" title={aiDev.reason} onClick={() => invoke('ai_device_retry').then(refreshAiDev)}>{L('coba lagi', 'retry')}</button></>
          )}
        </p>
      </Menu>
      <button type="button" className={upd.st.phase === 'ada' ? 'tb icon upd' : 'tb icon'}
        title={`StudioZea v${upd.version}${upd.st.phase === 'ada' ? L(` · versi ${upd.st.version} tersedia, klik untuk update`, ` · version ${upd.st.version} available, click to update`) : L(' · cek update', ' · check for updates')}`}
        disabled={upd.st.phase === 'checking' || upd.st.phase === 'unduh' || upd.st.phase === 'pasang'}
        onClick={upd.st.phase === 'ada' ? upd.install : upd.check}>
        <Icon name="refresh" />
        {upd.st.phase === 'ada' && <span className="dot" />}
      </button>
    </header>
  )

  if (home) {
    return (
      <div className="shell">
        {topbar}
        <div className="home">
          <h1>StudioZea</h1>
          <p className="lead">{L('Pilih jenis cetak', 'Choose the print type')}</p>
          <div className="homecards">
            <button type="button" className={s.printMode === 'dtf' ? 'homecard on' : 'homecard'} onClick={() => chooseMode('dtf')}>
              <Icon name="layers" size={34} />
              <b>DTF</b>
              <span>{L('Film transfer: halftone, underbase putih otomatis, tekstur vintage, export PNG siap cetak, export banyak, gang sheet.', 'Film transfer: halftone, automatic white underbase, vintage textures, print-ready PNG, batch export, gang sheet.')}</span>
            </button>
            <button type="button" className={s.printMode === 'sablon' ? 'homecard on' : 'homecard'} onClick={() => chooseMode('sablon')}>
              <Icon name="dots" size={34} />
              <b>{L('Sablon (separasi warna)', 'Screen printing (colour separation)')}</b>
              <span>{L('Pecah warna jadi film per tinta: underbase, warna, highlight putih, simulasi hasil cetak, tanda registrasi dan urutan cetak.', 'Split the colours into one film per ink: underbase, colours, highlight white, simulated print, registration marks and print order.')}</span>
            </button>
          </div>
          <label className="remember">
            <input type="checkbox" checked={rememberMode} onChange={(e) => setRememberMode(e.target.checked)} />
            <span>{L('Ingat pilihan saya (langsung dibuka berikutnya; halaman ini tetap bisa dibuka lewat logo StudioZea)', 'Remember my choice (open it directly next time; this page stays one click away on the StudioZea logo)')}</span>
          </label>
          <div className="btnrow homebtns">
            <button className="btn" type="button" onClick={pick}><Icon name="image" /><span>{L('Buka gambar…', 'Open image…')}</span></button>
            <button className="btn" type="button" onClick={pickProject}><Icon name="folder" /><span>{L('Buka proyek…', 'Open project…')}</span></button>
          </div>
          {img && <p className="small">{L(`Gambar terbuka: ${img.name}. Pilih jenis cetak untuk melanjutkan.`, `Open image: ${img.name}. Choose a print type to continue.`)}</p>}
          {toastEl}
        </div>
        <UpdateBar u={upd} />
      </div>
    )
  }

  if (tab === 'gang') {
    return (
      <div className="shell">
        {topbar}
        <GangSheet s={s} set={set} items={gangItems} setItems={setGangItems} pins={gangPins} setPins={setGangPins} job={job} tabs={null} flash={flash} toast={toast} addCurrent={addCurrentToGang}
          run={gangRun} setRun={setGangRun} runErr={gangErr} setRunErr={setGangErr} addRef={gangAddRef} />
        <UpdateBar u={upd} />
      </div>
    )
  }

  const stage = (
    <main className="stage">
      <div className="toolbar">
        <Seg value={mode} onChange={(m) => { setCropping(false); setMode(m) }} options={[['asli', L('Asli', 'Original')], ['latar', L('Tanpa latar', 'No BG')], ['hasil', sablon ? L('Hasil cetak', 'Print') : L('Hasil', 'Result')], ['split', 'Split']]} />
        {sablon && res?.sablon && (mode === 'hasil' || mode === 'split') && (
          <select className="filmpick" value={filmView ?? ''} title={L('Lihat simulasi hasil cetak, atau satu film (hitam = tinta)', 'Show the simulated print, or one film (black = ink)')}
            onChange={(e) => setFilmView(e.target.value === '' ? null : Number(e.target.value))}>
            <option value="">{L('Simulasi semua tinta', 'All inks (simulated)')}</option>
            {res.films.map((f, i) => <option key={i} value={i}>{L('Film', 'Film')} {i + 1}: {filmLabel(f)}</option>)}
          </select>
        )}
        {mode === 'asli' && img && !cropping && (
          <>
            <button className="btn" onClick={() => { setBrushOn(false); setCropping(true) }} type="button" title={L('Crop: tarik sudut/sisi kotak (Shift = rasio tetap), Enter = terapkan, Esc = batal.', 'Crop: drag the corners/sides (Shift = keep ratio), Enter = apply, Esc = cancel.')}>
              <Icon name="crop" /><span>Crop</span>
            </button>
            {s.crop && <button className="btn" onClick={() => set({ crop: null })} type="button" title={L('Kembali ke gambar utuh', 'Back to the whole image')}><Icon name="x" /><span>{L('Hapus crop', 'Clear crop')}</span></button>}
          </>
        )}
        {mode === 'asli' && img && cropping && (
          <>
            <button className="btn primary" onClick={() => viewer.current?.applyCrop()} type="button">{L('Terapkan (Enter)', 'Apply (Enter)')}</button>
            <button className="btn" onClick={() => viewer.current?.cancelCrop()} type="button">{L('Batal (Esc)', 'Cancel (Esc)')}</button>
          </>
        )}
        {img && !cropping && (
          <button className={eraserOn ? 'btn on' : 'btn'} type="button" onClick={toggleBrush}
            title={L('Penghapus: hapus bagian gambar dengan kuas (E). [ dan ] = ukuran kuas.', 'Eraser: brush away parts of the image (E). [ and ] = brush size.')}>
            <Icon name="eraser" /><span>{L('Penghapus', 'Eraser')}</span>
          </button>
        )}
        {img && !cropping && textBrushes && (
          <button className={textBrushOn ? 'btn on' : 'btn'} type="button" onClick={() => pickBrush(true)}
            title={L('Kuas teks (Lindungi teks): tandai bagian yang dicetak sebagai teks, atau bukan teks. Area teks diberi arsir biru.', 'Text brush (Protect text): mark what prints as text, or not text. Text areas are hatched blue.')}>
            <Icon name="type" /><span>{L('Kuas teks', 'Text brush')}</span>
          </button>
        )}
        <span className="grow" />
        <Seg value={backdrop} onChange={setBackdrop} options={[['shirt', L('Warna kaos', 'Shirt')], ['grid', L('Transparan', 'Transparent')]]} />
        <div className="zoombox">
          <button className="btn icon" type="button" title={L('Perkecil (Ctrl -)', 'Zoom out (Ctrl -)')} onClick={() => viewer.current?.zoomOut()}><Icon name="zoomout" /></button>
          <span className="mono zoomv">{(view.zoom * 100).toFixed(view.zoom < 0.1 ? 1 : 0)}%</span>
          <button className="btn icon" type="button" title={L('Perbesar (Ctrl +)', 'Zoom in (Ctrl +)')} onClick={() => viewer.current?.zoomIn()}><Icon name="zoomin" /></button>
          <button className="btn icon" type="button" title={L('Pas layar (Ctrl 0)', 'Fit (Ctrl 0)')} onClick={() => viewer.current?.fit()}><Icon name="fit" /></button>
          <button className="btn" type="button" title={L('Ukuran cetak 1:1 (Ctrl 1)', 'Print size 1:1 (Ctrl 1)')} onClick={() => viewer.current?.actual()}>100%</button>
        </div>
      </div>
      {brushView && (
        <div className="toolbar sub">
          <Icon name={textTool ? 'type' : 'eraser'} />
          <Seg value={brush.mode} onChange={(m) => setBrush((b) => ({ ...b, mode: m }))}
            options={textTool ? [['teks', L('Teks', 'Text')], ['bukan-teks', L('Bukan teks', 'Not text')]] : [['hapus', L('Hapus', 'Erase')], ['kembalikan', L('Kembalikan', 'Restore')]]} />
          <span className="small">{L('Kuas', 'Brush')}</span>
          <input type="range" className="brushsize" min="0" max="1000" value={Math.round((Math.log(brush.size) / Math.log(1000)) * 1000)}
            onChange={(e) => setBrush((b) => ({ ...b, size: Math.max(1, Math.round(Math.exp((Number(e.target.value) / 1000) * Math.log(1000)))) }))} />
          <span className="mono small">{brush.size} px{brushMm}</span>
          <Help text={(textTool
            ? L('Tarik di gambar: Teks = bagian ini dicetak sebagai teks (padat atau titik halus, lihat tab Halftone); Bukan teks = halftone biasa lagi. Area teks diberi arsir biru di pratinjau (tidak ikut dicetak).', 'Drag on the image: Text = this part prints as text (solid or fine dots, see the Halftone tab); Not text = normal halftone again. Text areas are hatched blue in the preview (not printed).')
            : L('Tarik di gambar untuk menghapus. Kembalikan = memunculkan lagi piksel asli.', 'Drag on the image to erase. Restore = bring the original pixels back.')) + L(' [ dan ] = ukuran kuas, Esc = selesai, Ctrl Z = batal.', ' [ and ] = brush size, Esc = done, Ctrl Z = undo.')} />
          <span className="grow" />
          {(textTool ? textStrokes : eraseStrokes) > 0 && (
            <button className="btn" type="button" onClick={() => clearStrokes(textTool)} title={textTool ? L('Buang semua coretan kuas teks di gambar ini', 'Remove all text brush strokes on this image') : L('Buang semua coretan penghapus di gambar ini', 'Remove all eraser strokes on this image')}>
              <Icon name="x" /><span>{L('Semua coretan', 'All strokes')} ({textTool ? textStrokes : eraseStrokes})</span>
            </button>
          )}
          <button className="btn" type="button" onClick={() => setBrushOn(false)}>{L('Selesai (Esc)', 'Done (Esc)')}</button>
        </div>
      )}
      <Viewer img={img} res={res} mode={mode} ov={sablon ? { ...ov, result: ov[sepKind] } : ov} vgen={sablon && res ? `${res.gen}/${sepKind}` : undefined}
        backdrop={sablon && filmView != null && mode !== 'asli' ? '#ffffff' : backdrop === 'grid' ? 'grid' : shirtCss(s)} fetchTile={sablon ? fetchSep : fetchTile} fetchOrig={fetchOrig}
        cropping={cropping && mode === 'asli'} crop={s.crop} setCrop={(c) => set({ crop: c })} onCropExit={() => setCropping(false)} onOpen={pick} viewer={viewer} onView={setView}
        brush={brushView} strokes={s.erase} hatch={hatch} />
      <div className="statusbar">
        {busyAll ? (
          <span className="busy">
            {busy || !prog ? L('Menyiapkan…', 'Preparing…') : `Render ${Math.round((prog.rows / prog.h) * 100)}%`}
            {prog && <span className="pbar"><span style={{ width: `${(prog.rows / prog.h) * 100}%` }} /></span>}
          </span>
        ) : res && done ? (
          <span className="meta">{res.w}×{res.h} px · {res.width_cm.toFixed(1)}×{res.height_cm.toFixed(1)} cm @ {res.dpi} DPI · {done.ms} ms</span>
        ) : <span className="meta">{img ? `${img.name} · ${img.w}×${img.h} px` : ''}</span>}
        <span className="grow" />
        {view.x != null && (
          <span className="meta mono">
            {mode === 'asli' ? `x ${Math.floor(view.x)} y ${Math.floor(view.y)} px` : res ? `x ${(view.x / res.dpi * 2.54).toFixed(2)} y ${(view.y / res.dpi * 2.54).toFixed(2)} cm` : ''}
          </span>
        )}
        <Help text={L('Roda mouse = zoom · tarik atau Spasi + tarik = geser · klik ganda = 100 % · Ctrl 0 = pas layar · Ctrl Z / Ctrl Y = batal / ulangi · E = penghapus', 'Mouse wheel = zoom · drag or Space + drag = pan · double-click = 100 % · Ctrl 0 = fit · Ctrl Z / Ctrl Y = undo / redo · E = eraser')} />
      </div>
    </main>
  )

  const checksOk = done?.checks && done.checks.semi === 0 && done.checks.tiny === 0 && done.checks.impure === 0
  const alerts = (
    <div className="alerts">
      {err && <p className="error"><Icon name="alert" /><span>{err}</span></p>}
      {ai && (
        <p className="ok">
          <Icon name="sparkles" />
          <span className="grow">
            {L('Perbesar AI', 'AI Upscale')} {ai.k}×… {ai.total ? `${Math.round((ai.done / ai.total) * 100)} %` : ''}
            <span className="pbar"><span style={{ width: `${ai.total ? (ai.done / ai.total) * 100 : 0}%` }} /></span>
          </span>
          <button className="link" type="button" onClick={() => invoke('ai_cancel')}>{L('Batal', 'Cancel')}</button>
        </p>
      )}
      {res?.warnings?.map((w, i) => (
        <p key={i} className="warn">
          <Icon name="alert" />
          <span>
            {warningText(w)}
            {w.code === 'res' && lowRes && !ai && aiK && (
              <> <button className="link" type="button" onClick={() => runAi()}>{L('Perbesar AI', 'AI Upscale')} {aiK}×</button></>
            )}
          </span>
        </p>
      ))}
      {done?.checks && current && (
        <p className={checksOk ? 'ok' : 'error'} title={L('Pemeriksaan otomatis: tidak ada piksel semi-transparan, warna tidak murni, atau titik/lubang di bawah 0,15 mm', 'Automatic check: no semi-transparent pixels, impure colours, or dots/holes under 0.15 mm')}>
          <Icon name={checksOk ? 'check' : 'alert'} />
          <span>{checksOk ? L('Siap cetak: bersih, tanpa semi-transparan', 'Print ready: clean, no semi-transparency') : L(`Semi-transparan ${done.checks.semi} · warna tidak murni ${done.checks.impure} · titik kecil ${done.checks.tiny}`, `Semi-transparent ${done.checks.semi} · impure colours ${done.checks.impure} · tiny dots ${done.checks.tiny}`)}</span>
        </p>
      )}
    </div>
  )

  const SECTIONS = [['gambar', 'image', L('Gambar', 'Image')], ['kaos', 'shirt', L('Kaos', 'Shirt')], ['ukuran', 'ruler', L('Ukuran', 'Size')], sablon ? ['warna', 'dots', L('Warna', 'Colours')] : ['halftone', 'dots', 'Halftone'], ['tekstur', 'layers', L('Tekstur', 'Texture')]]
  const bgHint = s.bg === 'otomatis'
    ? (s.shirtType === 'putih' ? L('Otomatis: putih jadi transparan; latar polos lain (hitam, navy, kertas…) juga dihapus.', 'Auto: white becomes transparent; other flat backgrounds (black, navy, paper…) are removed too.') : s.shirtType === 'hitam' ? L('Otomatis: hitam jadi transparan; latar polos lain (putih, navy, marun…) juga dihapus.', 'Auto: black becomes transparent; other flat backgrounds (white, navy, maroon…) are removed too.') : L('Otomatis: latar polos dari tepi gambar dihapus (gambar transparan dipakai apa adanya).', 'Auto: a flat background at the image edge is removed (transparent images are used as they are).'))
    : s.bg === 'warna' ? L('Warna latar: paksa hapus warna yang ada di tepi gambar.', 'Force: remove the colour at the image edge.') : L('Tidak: gambar dipakai apa adanya.', 'Off: the image is used as it is.')
  const sectionGambar = (
    <>
      <button className="btn primary wide" onClick={pick} type="button" title="PNG / JPG / TIFF / WebP (Ctrl O)">
        <Icon name="image" /><span>{img ? L('Ganti gambar…', 'Change image…') : L('Buka gambar…', 'Open image…')}</span>
      </button>
      {img && <p className="small">{img.name} · {img.w}×{img.h} px{img.has_alpha && !img.opaque ? L(' · transparan', ' · transparent') : ''}</p>}
      {aiFrom && img && (
        <p className="small">
          {L('Hasil Perbesar AI', 'AI upscaled')} {aiFrom.k}× ·{' '}
          <button className="link" type="button" disabled={!!ai} onClick={backToOriginal} title={aiFrom.path}>{L('pakai gambar asli', 'use the original')}</button>
        </p>
      )}
      <Row label={L('Perbesar AI', 'AI Upscale')}
        hint={L('AI (Real-ESRGAN, di komputer ini, tanpa internet) membesarkan gambar kecil 2× atau 4× dan menajamkan detail. Hasilnya langsung dipakai (Ctrl Z kembali ke asli); file PNG baru dibuat di samping proyek saat proyek disimpan. File asli tidak berubah. Pakai bila resolusi efektif di bawah ' + LOW_DPI + ' DPI. Kehalusan dan perangkat AI: menu ⚙ Pengaturan.',
          'AI (Real-ESRGAN, on this computer, no internet) enlarges small images 2× or 4× and sharpens the detail. The result is used at once (Ctrl Z goes back); a PNG file is written next to the project when you save it. The original stays unchanged. Use it below ' + LOW_DPI + ' effective DPI. AI style and device: ⚙ Settings menu.')}
        note={!img ? null : !aiK ? L('Gambar sudah terlalu besar untuk AI (hasil maks. 40 Mpx).', 'Image too large for AI (result max. 40 MP).') : !effDpi ? null
          : lowRes ? L(`${Math.round(effDpi)} → ±${Math.round(effDpi * aiK)} DPI efektif`, `${Math.round(effDpi)} → ±${Math.round(effDpi * aiK)} effective DPI`) : L(`Resolusi sudah cukup: ${Math.round(effDpi)} DPI efektif`, `Resolution is fine: ${Math.round(effDpi)} effective DPI`)}>
        {ai ? (
          <>
            <span className="small">{ai.total ? `${Math.round((ai.done / ai.total) * 100)} %` : L('Mulai…', 'Starting…')}</span>
            <button className="btn" type="button" onClick={() => invoke('ai_cancel')}><Icon name="x" /><span>{L('Batal', 'Cancel')}</span></button>
          </>
        ) : (
          <button className={lowRes ? 'btn primary' : 'btn'} type="button" disabled={!img || !imgPath || !aiK} onClick={() => runAi()}>
            <Icon name="sparkles" /><span>{aiK ? L(`Perbesar ${aiK}×`, `Upscale ${aiK}×`) : L('Perbesar', 'Upscale')}</span>
          </button>
        )}
      </Row>
      <Row label="Auto-trim" hint={L('Potong ruang kosong di sekeliling desain, jadi ukuran cetak = ukuran desainnya.', 'Cut the empty space around the design, so the print size = the design size.')}>
        <input type="checkbox" checked={s.autoTrim} onChange={(e) => set({ autoTrim: e.target.checked })} />
      </Row>
      <Row label={L('Putar', 'Rotate')} hint={L('Putar 90° (ukuran cetak tetap, lebar dan tinggi bertukar) atau cermin kiri-kanan. Diterapkan sebelum halftone, jadi sudut pola tetap.', 'Turn 90° (print size kept, width and height swap) or mirror left-right. Done before the halftone, so the screen angle stays.')}
        note={s.rotate || s.mirror ? L(`Diputar ${s.rotate}°${s.mirror ? ', dicermin' : ''}`, `Rotated ${s.rotate}°${s.mirror ? ', mirrored' : ''}`) : null}>
        <button className="btn icon" type="button" disabled={!img} title={L('Putar 90° ke kiri', 'Rotate 90° left')} onClick={() => turn(-90)}><Icon name="rotl" /></button>
        <button className="btn icon" type="button" disabled={!img} title={L('Putar 90° ke kanan', 'Rotate 90° right')} onClick={() => turn(90)}><Icon name="rotr" /></button>
        <button className={s.mirror ? 'btn icon on' : 'btn icon'} type="button" disabled={!img} title={L('Cermin kiri-kanan', 'Mirror')} onClick={() => set({ mirror: !s.mirror })}><Icon name="flip" /></button>
        {(s.rotate || s.mirror) ? <button className="link" type="button" onClick={() => orient(() => ({ rotate: 0, mirror: false }))}>reset</button> : null}
      </Row>
      <Row label="Crop" hint={L('Potong sebagian gambar: tampilan Asli → tombol Crop di atas gambar.', 'Cut part of the image: Original view → Crop button above the image.')}>
        {s.crop ? (
          <>
            <span className="small mono">{s.crop[2] - s.crop[0]}×{s.crop[3] - s.crop[1]} px</span>
            <button className="link" onClick={() => set({ crop: null })} type="button">reset</button>
          </>
        ) : (
          <button className="btn" type="button" disabled={!img} onClick={() => { setMode('asli'); setBrushOn(false); setCropping(true) }}><Icon name="crop" /><span>Crop…</span></button>
        )}
      </Row>
      <Row label={L('Penghapus', 'Eraser')} hint={L('Kuas untuk menghapus bagian gambar (atau mengembalikannya). Tombol E, [ dan ] = ukuran kuas.', 'Brush to erase parts of the image (or bring them back). Key E, [ and ] = brush size.')}>
        <button className={eraserOn ? 'btn on' : 'btn'} type="button" disabled={!img} onClick={toggleBrush}><Icon name="eraser" /><span>{eraserOn ? L('Aktif', 'On') : L('Kuas…', 'Brush…')}</span></button>
        {eraseStrokes > 0 && (
          <>
            <span className="small">{L(`${eraseStrokes} coretan`, `${eraseStrokes} strokes`)}</span>
            <button className="link" onClick={() => clearStrokes(false)} type="button">{L('hapus semua', 'clear all')}</button>
          </>
        )}
      </Row>
    </>
  )
  const sectionKaos = (
    <>
      <Row label={L('Jenis kaos', 'Shirt')} hint={L('Di DTF, warna latar desain = warna kaos: latar itu tidak dicetak, diganti warna kaosnya sendiri.', 'In DTF the design background = the shirt colour: it is not printed, the shirt shows through.')}>
        <Seg value={s.shirtType} onChange={(v) => set({ shirtType: v })} options={[['hitam', L('Hitam', 'Black')], ['putih', L('Putih', 'White')], ['warna', L('Warna', 'Colour')]]} />
      </Row>
      {s.shirtType === 'warna' && (
        <Row label={L('Warna kaos', 'Shirt colour')}>
          <input type="color" value={s.shirtColor} onChange={(e) => set({ shirtColor: e.target.value })} />
          <span className="small mono">{s.shirtColor}</span>
        </Row>
      )}
      <Row label={L('Kaos otomatis', 'Auto shirt')} hint={L('Saat gambar dibuka, kaos dipilih sesuai warna latarnya: latar hitam → kaos hitam, putih → putih, warna lain → kaos warna itu. Tetap bisa diganti manual.', 'When an image opens, the shirt follows its background: black → black shirt, white → white, other colours → that colour. You can still change it.')}>
        <input type="checkbox" checked={!!s.autoShirt} onChange={(e) => set({ autoShirt: e.target.checked })} />
      </Row>
      <Row label={L('Hapus latar', 'Remove BG')} hint={bgHint}>
        <Seg value={s.bg} onChange={(v) => set({ bg: v })} options={[['otomatis', L('Otomatis', 'Auto')], ['warna', L('Paksa', 'Force')], ['tidak', L('Tidak', 'Off')]]} />
      </Row>
      {(s.bg === 'warna' || (s.bg === 'otomatis' && res?.bg_used)) && (
        <>
          <Row label={L('Toleransi', 'Tolerance')} hint={L('Kecilkan kalau warna terang di dalam desain ikut hilang.', 'Lower it if light colours inside the design disappear too.')}>
            <input type="range" min="0.02" max="0.4" step="0.01" value={s.bgTolerance ?? res?.bg_used?.[0] ?? 0.3} onChange={(e) => set({ bgTolerance: Number(e.target.value) })} />
            <span className="small mono">{Number(s.bgTolerance ?? res?.bg_used?.[0] ?? 0.3).toFixed(2)}</span>
          </Row>
          <Row label={L('Di dalam desain', 'Inside design')} hint={L('Hapus juga warna latar yang terkurung di dalam desain (lubang huruf). Mati otomatis untuk latar putih/hitam.', 'Also remove background colour enclosed in the design (letter holes). Off by default for white/black backgrounds.')}>
            <input type="checkbox" checked={s.bgPockets ?? res?.bg_used?.[1] ?? true} onChange={(e) => set({ bgPockets: e.target.checked })} />
            {(s.bgTolerance != null || s.bgPockets != null) && <button className="link" type="button" onClick={() => set({ bgTolerance: null, bgPockets: null })}>{L('otomatis', 'auto')}</button>}
          </Row>
        </>
      )}
      {res?.bg && (
        <p className="small">
          {L('Latar terdeteksi:', 'Background found:')} <span className="swatch" style={{ background: `rgb(${res.bg.join(',')})` }} /> rgb({res.bg.join(', ')})
        </p>
      )}
      {s.shirtType === 'warna' && (
        <Row label={L('Hemat tinta', 'Save ink')} hint={L('Warna desain yang mirip warna kaos dibuat transparan.', 'Design colours close to the shirt colour become transparent.')}>
          <input type="range" min="0" max="0.3" step="0.01" value={s.knockout} onChange={(e) => set({ knockout: Number(e.target.value) })} />
          <span className="small mono">{Number(s.knockout).toFixed(2)}</span>
        </Row>
      )}
    </>
  )
  const sectionUkuran = (
    <>
      <Row label={L('Satuan', 'Units')}>
        <Seg value={s.unit} options={[['cm', 'cm'], ['in', L('inci', 'inch')], ['px', L('piksel', 'pixels')]]} onChange={(v) => set({ unit: v })} />
      </Row>
      <Row label={`${L('Lebar', 'Width')} (${unitLabel})`} hint={L('Tinggi ikut otomatis (rasio tetap). Enter atau klik di luar kolom untuk menerapkan.', 'Height follows (ratio kept). Press Enter or click outside the field to apply.')}>
        <NumField value={fromCm(s.widthCm)} digits={s.unit === 'px' ? 0 : s.unit === 'in' ? 2 : 1} min={fromCm(1)} max={fromCm(200)} step={s.unit === 'px' ? 1 : 0.1}
          onCommit={(v) => set({ widthCm: Math.round(toCm(v) * 1e6) / 1e6 })} />
      </Row>
      <Row label={`${L('Tinggi', 'Height')} (${unitLabel})`}>
        <NumField value={heightCm == null ? null : fromCm(heightCm)} digits={s.unit === 'px' ? 0 : s.unit === 'in' ? 2 : 1} min={fromCm(1)} max={fromCm(300)} step={s.unit === 'px' ? 1 : 0.1} placeholder={L('otomatis', 'auto')}
          onCommit={(v) => res && v > 0 && set({ widthCm: Math.round(((toCm(v) * res.width_cm) / res.height_cm) * 1e6) / 1e6 })} />
      </Row>
      <div className="sizes">
        {SIZES.map((o) => [sizeLabel(o), o[1]]).map(([l, cm]) => (
          <button key={l} type="button" className={Math.abs(s.widthCm - cm) < 0.01 ? 'chip on' : 'chip'} onClick={() => set({ widthCm: cm })} title={L(`Lebar ${fmt(cm)} cm`, `Width ${fmt(cm)} cm`)}>
            {l}
          </button>
        ))}
      </div>
      <Row label="DPI" hint={L('300 = standar DTF. 600 = lebih halus (file 4× lebih besar).', '300 = DTF standard. 600 = finer (4× larger file).')}>
        <select value={s.dpi} onChange={(e) => set({ dpi: Number(e.target.value) })}>
          {DPIS.map((d) => <option key={d} value={d}>{d}</option>)}
        </select>
      </Row>
      <Row label="Resize" hint={L('Lanczos: paling tajam (disarankan). Bicubic: lebih halus, seperti Photoshop.', 'Lanczos: sharpest (recommended). Bicubic: softer, like Photoshop.')}>
        <Seg value={s.resample} options={[['lanczos', 'Lanczos'], ['bicubic', 'Bicubic']]} onChange={(v) => set({ resample: v })} />
      </Row>
      <Row label={L('Margin export', 'Margin')} hint={L('Ruang transparan di sekeliling file PNG (mis. untuk potong). 0 = tanpa margin; pratinjau tidak berubah.', 'Transparent space around the PNG (e.g. for cutting). 0 = none; the preview does not change.')}>
        <NumField className="short" value={s.marginMm} min={0} max={100} step={0.5} digits={1} onCommit={(v) => set({ marginMm: v })} />
        <span className="small">mm</span>
      </Row>
      {res && (
        <div className="result">
          <b>{fmt(res.width_cm)} × {fmt(res.height_cm)} cm</b>
          <span>{res.w}×{res.h} px · {fmt(res.width_cm / 2.54, 2)}×{fmt(res.height_cm / 2.54, 2)} {L('inci', 'in')} @ {res.dpi} DPI</span>
          {s.marginMm > 0 && <span>{L('File PNG + margin:', 'PNG + margin:')} {fmt(res.width_cm + s.marginMm / 5)} × {fmt(res.height_cm + s.marginMm / 5)} cm</span>}
          {res.effective_dpi && <span>{L('Resolusi sumber:', 'Source resolution:')} {res.effective_dpi.toFixed(0)} {L('DPI efektif', 'effective DPI')}{res.effective_dpi < s.dpi ? L(' (diperbesar)', ' (enlarged)') : ''}</span>}
        </div>
      )}
    </>
  )
  const amLike = s.pattern === 'am' || s.pattern === 'interlock'
  const sectionHalftone = (
    <>
      <Row label="Halftone" hint={s.halftoneOn
        ? L('Nyala: desain dipecah jadi titik (pola di bawah).', 'On: the design is turned into dots (pattern below).')
        : L('Mati: tanpa titik dan tanpa setengah transparan. Tiap piksel dicetak penuh dengan warna aslinya (bila minimal 50 % tidak transparan) atau tidak sama sekali. Cocok untuk desain yang hanya memakai tekstur vintage.', 'Off: no dots and no semi-transparency. Each pixel prints in full in its own colour (if at least 50 % opaque) or not at all. Good for texture-only vintage looks.')}>
        <Seg value={s.halftoneOn ? 'on' : 'off'} options={[['on', L('Nyala', 'On')], ['off', L('Mati', 'Off')]]} onChange={(v) => set({ halftoneOn: v === 'on' })} />
      </Row>
      {!s.halftoneOn && (
        <Row label={L('Buang mirip kaos', 'Drop shirt-like')} hint={L(`Warna yang ${s.shirtCut} % atau kurang berbeda dari warna kaos tidak dicetak (bayangan hampir ${s.shirtType === 'hitam' ? 'hitam' : s.shirtType === 'putih' ? 'putih' : 'sewarna kaos'}, noise JPG): hemat tinta dan underbase. 0 = cetak semua.`, `Colours within ${s.shirtCut} % of the shirt colour are not printed (near-${s.shirtType === 'hitam' ? 'black' : s.shirtType === 'putih' ? 'white' : 'shirt-colour'} shadows, JPG noise): saves ink and underbase. 0 = print everything.`)}>
          <Slider min={0} max={40} step={1} value={s.shirtCut} onCommit={(v) => set({ shirtCut: v })} />
          <NumField className="short" value={s.shirtCut} min={0} max={100} step={1} onCommit={(v) => set({ shirtCut: v })} />
        </Row>
      )}
      {s.halftoneOn && (
        <Row label={L('Lindungi teks', 'Protect text')}
          hint={L('AI mencari tulisan di desain dan mencetaknya sebagai teks (padat atau titik halus, di bawah); bagian lain tetap halftone. Jadi tulisan kecil tidak pecah. Tulisan yang terlewat atau salah bisa diatur dengan Kuas teks.', 'AI finds the text in the design and prints it as text (solid or fine dots, below); the rest stays halftone, so small text does not break up. Fix missed or wrong areas with the Text brush.')}
          note={!s.protectText ? null : !res || !current ? L('Mencari tulisan…', 'Finding text…') : res.text_words == null ? L('Detektor teks tidak jalan; pakai kuas Teks.', 'Text detector not available; use the Text brush.') : res.text_words === 0 ? L('Tidak ada tulisan ditemukan; pakai kuas Teks bila perlu.', 'No text found; use the Text brush if needed.') : L(`${res.text_words} tulisan ditemukan`, `${res.text_words} text areas found`)}>
          <input type="checkbox" checked={!!s.protectText} onChange={(e) => set({ protectText: e.target.checked })} />
        </Row>
      )}
      {textBrushes && (
        <Row label={L('Teks dicetak', 'Text prints')}
          hint={L('Padat: huruf dicetak penuh dengan warna aslinya (seperti spot color); paling tajam, tapi huruf bergradasi / metalik jadi terlihat seperti foto. Titik halus: huruf tetap halftone dengan pola yang sama, tapi titiknya lebih rapat, jadi tetap terbaca dan gayanya menyatu. Titik halus hanya untuk pola AM dan Interlock; pola lain sudah bertitik halus, jadi teksnya padat.',
            'Solid: letters print in full in their own colours (like a spot colour); sharpest, but gradient / metallic letters look like a photo. Fine dots: letters stay halftone in the same pattern with denser dots, so they read well and match the style. Fine dots only for AM and Interlock; the other patterns are already fine, so their text is solid.')}
          note={s.textStyle === 'halus' ? (amLike ? L(`Titik teks ±${Math.round(textFineLpi(Number(s.lpi), Number(s.dpi)))} LPI (desain ${Math.round(Number(s.lpi))} LPI)`, `Text dots ±${Math.round(textFineLpi(Number(s.lpi), Number(s.dpi)))} LPI (design ${Math.round(Number(s.lpi))} LPI)`) : L('Pola ini: teks padat.', 'This pattern: solid text.')) : null}>
          <Seg value={s.textStyle} onChange={(v) => set({ textStyle: v })} options={[['padat', L('Padat', 'Solid')], ['halus', L('Titik halus', 'Fine dots')]]} />
        </Row>
      )}
      {textBrushes && (
        <Row label={L('Area teks', 'Text areas')}
          hint={L('Kuas teks: tandai bagian yang terlewat (Teks) atau yang salah dianggap teks (Bukan teks). Arsir: area teks diberi garis miring biru di pratinjau (hanya tampilan, tidak dicetak).', 'Text brush: mark what was missed (Text) or wrongly taken as text (Not text). Hatch: text areas get blue diagonal lines in the preview (display only, not printed).')}>
          <button className={textBrushOn ? 'btn on' : 'btn'} type="button" disabled={!img} onClick={() => pickBrush(true)}><Icon name="type" /><span>{textBrushOn ? L('Aktif', 'On') : L('Kuas teks…', 'Text brush…')}</span></button>
          <button className={hatch ? 'btn icon on' : 'btn icon'} type="button" disabled={!img} title={L('Tampilkan arsir area teks', 'Show the text area hatching')} onClick={() => setShowText((v) => !v)}><Icon name="eye" /></button>
          {textStrokes > 0 && <button className="link" type="button" title={L('Buang semua coretan kuas teks', 'Remove all text brush strokes')} onClick={() => clearStrokes(true)}>{L(`hapus (${textStrokes})`, `clear (${textStrokes})`)}</button>}
        </Row>
      )}
      {s.halftoneOn && (
        <>
          <Row label="Preset">
            <select value="" onChange={(e) => applyPreset(e.target.value)}>
              <option value="">{L('— pilih preset —', '— choose a preset —')}</option>
              {PRESETS.map((p) => <option key={p.id} value={p.id}>{L(p.name, p.en)}</option>)}
            </select>
          </Row>
          <Row label={L('Pola', 'Pattern')} hint={PATTERN_HINT()[s.pattern] || L('AM: titik bersudut (rosette) seperti sablon. FM: titik acak halus. Grain: butiran vintage.', 'AM: angled dots (rosette) like screen printing. FM: fine random dots. Grain: vintage grain.')}>
            <select value={s.pattern} onChange={(e) => set({ pattern: e.target.value })}>
              {PATTERNS.map((o) => <option key={o[0]} value={o[0]}>{optLabel(o)}</option>)}
            </select>
          </Row>
          {amLike && (
            <>
              {s.pattern === 'am' && <PatternPicker shape={s.shape} angle={s.angle} onChange={(v) => set({ shape: v })} />}
              <Row label={L('Ukuran (LPI)', 'Size (LPI)')} hint={L('Kecil = titik besar (3–10 pop art), 35 = seperti referensi, besar = titik halus.', 'Low = big dots (3–10 pop art), 35 = classic, high = fine dots.')} note={L(`Jarak titik ${fmt(25.4 / s.lpi, 2)} mm`, `Dot spacing ${fmt(25.4 / s.lpi, 2)} mm`)}>
                <Slider min={0} max={1000} step={1} value={s.lpi} toSlider={lpiToSlider} fromSlider={sliderToLpi} onCommit={(v) => set({ lpi: v })} />
                <NumField className="short" value={s.lpi} min={LPI_MIN} max={LPI_MAX} step={0.5} onCommit={(v) => set({ lpi: v })} />
              </Row>
              <Row label={L('Sudut (°)', 'Angle (°)')} hint={s.pattern === 'interlock' ? 'Auto = 45°.' : L('Auto = 22,5° seperti referensi.', 'Auto = 22.5° (classic).')}>
                <Seg value={s.angle == null ? 'auto' : ANGLES.includes(Number(s.angle)) ? String(s.angle) : 'x'}
                  options={[['auto', 'Auto'], ...ANGLES.map((a) => [String(a), `${a}°`])]}
                  onChange={(v) => set({ angle: v === 'auto' ? null : Number(v) })} />
              </Row>
              <Row label="">
                <Slider min={0} max={359} step={1} value={s.angle ?? autoAngle} onCommit={(v) => set({ angle: v })} />
                <NumField className="short" value={s.angle ?? autoAngle} min={0} max={359.9} step={0.5} onCommit={(v) => set({ angle: v })} />
              </Row>
              {/* rosette needs a screen per ink: the true-colour looks (fades only, true-colour dots) use one screen */}
              {s.shirtType !== 'hitam' && s.pattern === 'am' && !s.fades && !s.trueDots && (
                <Row label={L('Warna', 'Colours')} hint={L(`Rosette: C/Y ${fmt(s.angle ?? 22.5)}°, M ${fmt(((s.angle ?? 22.5) + 45) % 360)}° (seperti referensi). Satu sudut: semua warna di sudut yang sama (titik di atas titik). Auto: rosette di bawah 55 LPI.`, `Rosette: C/Y ${fmt(s.angle ?? 22.5)}°, M ${fmt(((s.angle ?? 22.5) + 45) % 360)}° (classic). One angle: all colours at the same angle (dot on dot). Auto: rosette below 55 LPI.`)}>
                  <Seg value={s.rosette == null ? 'auto' : s.rosette ? 'ya' : 'tidak'}
                    options={[['auto', 'Auto'], ['ya', 'Rosette'], ['tidak', L('Satu sudut', 'One angle')]]}
                    onChange={(v) => set({ rosette: v === 'auto' ? null : v === 'ya' })} />
                </Row>
              )}
              {s.pattern === 'am' && (
                <Row label="Grid" hint={L('Auto: grid 8:3 seperti referensi bila cocok, selain itu presisi. Rapi: semua titik identik.', 'Auto: the classic 8:3 grid when it fits, else precise. Even: all dots identical.')}>
                  <Seg value={s.grid} onChange={(v) => set({ grid: v })} options={[['auto', 'Auto'], ['rapi', L('Rapi', 'Even')], ['presisi', L('Presisi', 'Precise')]]} />
                </Row>
              )}
              {res?.screen && (
                <p className="small">
                  {L('Nyata:', 'Actual:')} {res.screen.lpi.toFixed(2)} LPI · {res.screen.angle.toFixed(2)}° · {res.screen.levels} {L('tingkat nada', 'tone levels')}
                </p>
              )}
            </>
          )}
          <Row label="Area" hint={s.trueDots
            ? L('Titik warna asli: seluruh desain jadi titik dengan satu raster (satu sudut), tiap titik memakai warna asli desain (tidak dipecah jadi tinta murni), bagian sangat terang tetap solid. Seperti kaos band halftone warna. Paling pas dengan bentuk Bulat murni dan LPI rendah (10-15).', 'True-colour dots: the whole design becomes dots of one screen (one angle), every dot keeps the design\'s own colour (not split into pure inks), very light parts stay solid. Like colour-halftone band shirts. Best with the Pure round shape and a low LPI (10-15).')
            : s.fades
            ? L('Gradasi saja: warna blok tetap solid (warna asli), titik hanya di bagian yang memudar ke warna kaos (glow, bayangan, asap, pinggiran). Geser ke kanan = area titik makin luas.', 'Fades only: solid areas keep their true colours, dots only where the design fades into the shirt (glow, shadow, smoke, edges). Slide right = more dots.')
            : L('Semua: seluruh warna dipecah jadi titik warna murni (seperti referensi).', 'All: every colour becomes dots of pure colour (classic).')}>
            <Seg value={s.trueDots ? 'warna' : s.fades ? 'gradasi' : 'semua'} options={[['semua', L('Semua', 'All')], ['gradasi', L('Gradasi saja', 'Fades only')], ['warna', L('Titik warna asli', 'True-colour dots')]]} onChange={(v) => set({ fades: v === 'gradasi', trueDots: v === 'warna' })} />
          </Row>
          {s.trueDots && (
            <>
              <Row label={L('Naik warna', 'Colour lift')} hint={L('Warna tiap titik dinaikkan ke kecerahan penuh (warna dan kepekatan tetap), titiknya dikecilkan sebanding: dari jauh warnanya sama, dari dekat titiknya cerah. 100 % = seperti kaos band halftone (paling cerah, cocok untuk desain warna rata di kaos hitam). Untuk lukisan / foto atau kaos berwarna coba sekitar 30 %. 0 = warna desain apa adanya.', 'Each dot colour is raised to full brightness (hue and saturation kept) and the dot made smaller to match: the same colour from a distance, bright dots up close. 100 % = like halftone band shirts (brightest, best for flat-colour art on black). For paintings / photos or coloured shirts try about 30 %. 0 = the design colour as it is.')}>
                <Slider min={0} max={100} step={1} value={s.liftColour} onCommit={(v) => set({ liftColour: v })} />
                <NumField className="short" value={s.liftColour} min={0} max={100} step={1} onCommit={(v) => set({ liftColour: v })} />
              </Row>
              <Row label={L('Ukuran titik', 'Dot size')} hint={L('Besar titik maksimum (%). 100 = warna tidak turun (dari jauh sama dengan desain). Lebih kecil = lebih banyak kaos terlihat di sela titik, warna jadi lebih gelap / pudar.', 'Largest dot size (%). 100 = colours do not fade (the same as the design from a distance). Smaller = more shirt between the dots, colours look darker / duller.')}>
                <Slider min={20} max={100} step={1} value={s.dotSize} onCommit={(v) => set({ dotSize: v })} />
                <NumField className="short" value={s.dotSize} min={1} max={100} step={1} onCommit={(v) => set({ dotSize: v })} />
              </Row>
              <Row label={L('Padat mulai', 'Solid from')} hint={L('Warna yang kecerahannya sejauh ini (%) atau lebih dari warna kaos dicetak solid tanpa titik. Di kaos gelap: putih dan sorotan terang; di kaos putih: hitam dan warna gelap. Turunkan (mis. 75) supaya lebih banyak yang solid; 100 = tidak ada yang solid.', 'Colours whose brightness is this far (%) or more from the shirt colour print solid, without dots. On a dark shirt: white and bright highlights; on a white shirt: black and dark colours. Lower it (e.g. 75) for more solid areas; 100 = nothing solid.')}>
                <Slider min={50} max={100} step={1} value={s.solidFrom} onCommit={(v) => set({ solidFrom: v })} />
                <NumField className="short" value={s.solidFrom} min={1} max={100} step={1} onCommit={(v) => set({ solidFrom: v })} />
              </Row>
            </>
          )}
          {s.fades && !s.trueDots && (
            <Row label="">
              <Slider min={5} max={100} step={1} value={s.fadesArea} onCommit={(v) => set({ fadesArea: v })} />
              <NumField className="short" value={s.fadesArea} min={1} max={100} step={1} onCommit={(v) => set({ fadesArea: v })} />
            </Row>
          )}
          {s.shirtType !== 'hitam' && !s.fades && !s.trueDots && (
            <Row label={L('Hitam (K)', 'Black (K)')} hint={L('GCR: abu-abu/bayangan jadi titik hitam bersih. Garis saja: hanya garis hitam pekat.', 'GCR: greys/shadows become clean black dots. Lines: only solid black lines.')}>
              <Seg value={s.k} onChange={(v) => set({ k: v })} options={[['off', L('Mati', 'Off')], ['garis', L('Garis', 'Lines')], ['gcr', 'GCR']]} />
            </Row>
          )}
          <Row label={L('Tepi tajam', 'Sharp edges')} hint={L('Tepi huruf/garis keras dibuat solid dan bersih (tanpa titik terpotong).', 'Hard letter/line edges are made solid and clean (no cut dots).')}>
            <input type="checkbox" checked={s.edges} onChange={(e) => set({ edges: e.target.checked })} />
          </Row>
          {amLike && levels < 30 && <p className="warn"><Icon name="alert" /><span>{L(`Hanya ${levels} tingkat nada pada ${s.dpi} DPI. Naikkan DPI atau turunkan LPI.`, `Only ${levels} tone levels at ${s.dpi} DPI. Raise the DPI or lower the LPI.`)}</span></p>}
        </>
      )}
    </>
  )
  // ---------------------------------------------------------------- sablon: inks and films
  const sepFilms = res?.sablon && current ? res.films : null
  const hasBase = !!(res?.sablon && res.films.some((f) => f.role === 'underbase'))
  // the ink list: set by hand, else the result's (from the ink set; dropped inks too)
  const inkList = s.sepInks.length ? s.sepInks : (res?.sablon ? res.inks : [])
  const baseAt = Math.min(s.sepInks.length ? s.sepBaseAfter : (res?.sablon ? res.base_after : 0), inkList.length)
  // rows in print order: the underbase among the inks
  const inkRows = [...inkList.map((ink, i) => ({ ink, i })).slice(0, baseAt), ...(hasBase ? [{ base: true }] : []), ...inkList.map((ink, i) => ({ ink, i })).slice(baseAt)]
  // a change to the rows: the inks become a list set by hand
  const editRows = (fn) => {
    const named = (ink) => { const c = ink.name ? null : catInk(ink.rgb); return ink.name || (c ? inkLine(c.ink, c.cat) : undefined) }
    const rows = fn(inkRows.map((r) => (r.base ? r : { ink: { rgb: r.ink.rgb, opacity: r.ink.opacity, boost: r.ink.boost || 0, name: named(r.ink) } })))
    const b = rows.findIndex((r) => r.base)
    set({ sepInks: rows.filter((r) => !r.base).map((r) => r.ink), sepBaseAfter: b < 0 ? Math.min(baseAt, rows.length) : b })
  }
  const moveRow = (k, d) => editRows((l) => { const c = [...l]; [c[k + d], c[k]] = [c[k], c[k + d]]; return c })
  const tone = shirtTone(s)
  const sets = inkSets(tone)
  const setOpts = [...sets, ...(myInks ? [['saya', 'Tinta saya', 'My inks']] : [])]
  if (!setOpts.some((o) => o[0] === s.sepSet)) setOpts.push([s.sepSet, ...(inkSets('gelap').concat(inkSets('warna')).find((o) => o[0] === s.sepSet) || [s.sepSet, 'Tinta saya', 'My inks']).slice(1)])
  // an ink from a catalogue for ink row i (any brand)
  const chooseCatInk = (i, ci, cat) => {
    const rgb = hexToRgb8(ci.h)
    editRows((l) => l.map((x) => (x.ink && x === l.filter((y) => !y.base)[i] ? { ink: { ...x.ink, rgb, name: inkLine(ci, cat) } } : x)))
    setInkPick(null)
  }
  const pickCats = pickCatId === '*' ? allCats : [allCats.find((c) => c.id === pickCatId) || activeCat]
  const pickQl = pickQ.trim().toLowerCase()
  const pickInks = pickCats.flatMap((cat) => cat.inks.filter((ci) => !pickQl || `${ci.n} ${ci.c || ''} ${ci.p || ''} ${ci.s || ''} ${cat.id === STD_ID ? '' : cat.name}`.toLowerCase().includes(pickQl)).map((ci) => ({ ci, cat }))).slice(0, 400)
  const specialCount = activeCat.inks.filter((ci) => inkKind(ci)).length
  const importXlsx = async (e) => {
    const file = e.target.files?.[0]
    e.target.value = ''
    if (!file) return
    try {
      if (file.size > 20 << 20) throw new Error(L('file lebih dari 20 MB', 'file over 20 MB'))
      const cats = catalogsFromSheets(await readXlsx(await file.arrayBuffer()))
      if (!cats.length) throw new Error(L('tidak ada tabel dengan kolom Hex dan Nama', 'no table with a Hex and a Name column'))
      setCatalogs((o) => [...o.filter((c) => !cats.some((n) => n.id === c.id)), ...cats].slice(0, 100))
      set({ sepSet: 'katalog', sepKatalog: cats[0].id, sepInks: [], sepBaseAfter: 0 })
      const n = cats.reduce((a, c) => a + c.inks.length, 0)
      flash(L(`Diimpor ${cats.length} katalog, ${n} warna: ${cats.map((c) => c.name).join(', ')}`, `Imported ${cats.length} catalogues, ${n} colours: ${cats.map((c) => c.name).join(', ')}`))
    } catch (x) {
      setErr(L('Impor katalog gagal: ', 'Catalogue import failed: ') + (x?.message || x))
    }
  }
  const chooseSet = (v) => (v === 'saya' && myInks ? set({ sepSet: 'saya', sepInks: myInks.inks, sepBaseAfter: myInks.baseAfter }) : set({ sepSet: v, sepInks: [], sepBaseAfter: 0 }))
  const dE = res?.sablon ? res.delta_e : null
  const dEText = dE == null ? null : dE < 3 ? L('sangat mirip', 'very close') : dE < 6 ? L('mirip', 'close') : dE < 10 ? L('cukup, bisa tambah warna', 'fair, add a colour') : L('jauh, tambah warna', 'far, add colours')
  const sim = s.sepMethod === 'simulasi'
  const sectionWarna = (
    <>
      <Row label="Preset" hint={L('Film PRO: dipelajari dari film pecah warna buatan tangan (CorelDRAW) untuk kaos band: 35 LPI, warna titik di atas titik 22,5°, film putih (underbase + highlight) di sudut sendiri 7,5° supaya tidak moire dan tetap rapi bila screen putih sedikit bergeser, putih maks. 85 % di gradasi (gradasi halus), tinta warna kuat, tulisan / logo padat seperti film block. Warna diutamakan sama dengan desain; naikkan Hemat tinta bila ingin lebih hemat. Standar: pengaturan bawaan StudioZea.', 'PRO films: learnt from hand-made film separations (CorelDRAW) for band shirts: 35 LPI, colours dot on dot at 22.5°, the white films (underbase + highlight) on their own 7.5° angle so there is no moiré and the print stays clean when the white screen shifts a little, at most 85 % white in gradients (smooth gradients), strong inks, lettering / logos solid like a block film. Colour comes first; raise the Ink saver to use less ink. Standard: the StudioZea defaults.')}>
        <select value="" onChange={(e) => { const p = SEP_PRESETS.find((x) => x.id === e.target.value); if (p) set(p.set) }}>
          <option value="">{L('— pilih preset —', '— choose a preset —')}</option>
          {SEP_PRESETS.map((p) => <option key={p.id} value={p.id}>{L(p.name, p.en)}</option>)}
        </select>
      </Row>
      <Row label={L('Metode', 'Method')}
        hint={L('Simulasi: titik halftone per tinta, untuk desain foto / gradasi (seperti simulated process). Index: kotak-kotak kecil, satu tinta per titik, warna tajam tanpa moire. Spot: warna rata tanpa titik, untuk desain vektor / warna blok.', 'Simulated: halftone dots per ink, for photo / gradient designs (simulated process). Index: small squares, one ink per dot, crisp colours without moiré. Spot: flat colours without dots, for vector / block-colour designs.')}>
        <Seg value={s.sepMethod} onChange={(v) => set({ sepMethod: v })} options={[['simulasi', L('Simulasi', 'Simulated')], ['index', 'Index'], ['spot', 'Spot']]} />
      </Row>
      <Row label={L('Set tinta', 'Ink set')}
        hint={L('Otomatis: tinta dipilih dari warna desain (paling mirip, jumlahnya bisa diatur). Set tetap: tinta standar yang biasa ada di workshop, disesuaikan dengan warna kaos (kaos gelap: underbase, abu dan highlight putih; kaos putih / berwarna: dengan hitam). Filmnya tetap dihitung dengan model cetak, jadi tidak perlu Boost manual. Tinta yang hampir tidak terpakai dibuang. Tinta saya: daftar tinta yang Anda simpan sendiri.', 'Automatic: inks chosen from the design colours (closest, the number can be set). Fixed sets: standard inks most shops stock, fitted to the shirt colour (dark shirts: underbase, grey and highlight white; white / coloured shirts: with black). The films are still computed with the print model, so no manual boost is needed. Inks that would print almost nothing are dropped. My inks: an ink list you saved yourself.')}>
        <select value={s.sepSet} onChange={(e) => chooseSet(e.target.value)}>
          {setOpts.map((o) => <option key={o[0]} value={o[0]}>{L(o[1], o[2])}</option>)}
        </select>
      </Row>
      {s.sepSet === 'katalog' && (
        <Row label={L('Katalog', 'Catalogue')}
          hint={L('Tinta dipilih dari katalog ini saja, warnanya persis seperti di katalog (nama + kode ikut di film dan urutan cetak). Metode Simulasi = "simulasi dari stok": titik halftone dari tinta yang ada di gudang. Metode Spot = "Spot Pantone": satu tinta katalog per warna, dicetak padat di atas underbase padat; warna putih desain = underbase + highlight putih; di kaos hitam, hitam dan warna gelap kusam (navy, coklat tua) dibiarkan warna kaos. Neon dan metalik tidak dipilih otomatis (pratinjau tidak bisa menunjukkan kilau / nyalanya), tapi bisa dipilih manual di daftar tinta. Impor .xlsx: tabel dengan kolom Hex dan Nama (opsional Merek, Seri, Kode pabrik, Pantone); tiap merek jadi satu katalog, disimpan di komputer ini saja.', 'Inks are chosen from this catalogue only, exactly as listed (name + code go on the films and the print order). Method Simulated = "simulated from stock": halftone dots from the inks you have. Method Spot = "Spot Pantone": one catalogue ink per colour, printed solid on a solid underbase; design white = underbase + highlight white; on a black shirt, black and dark muted colours (navy, dark brown) are left to the shirt. Neon and metallic inks are not chosen automatically (the preview cannot show their glow / shine), but can be picked by hand in the ink list. Import .xlsx: a table with Hex and Name columns (optional Brand, Series, Factory code, Pantone); each brand becomes a catalogue, kept on this computer only.')}
          note={specialCount ? L(`${catPalette.length} warna dipakai otomatis · ${specialCount} neon/metalik hanya manual`, `${catPalette.length} colours used automatically · ${specialCount} neon/metallic by hand only`) : L(`${catPalette.length} warna`, `${catPalette.length} colours`)}>
          <select value={activeCat.id} onChange={(e) => set({ sepKatalog: e.target.value, sepInks: [], sepBaseAfter: 0 })}>
            {allCats.map((c) => <option key={c.id} value={c.id}>{c.id === STD_ID ? L(STD.name, STD.en) : c.name} ({c.inks.length})</option>)}
          </select>
          <button className="btn" type="button" title={L('Impor katalog warna merek dari file Excel (.xlsx)', 'Import brand colour charts from an Excel file (.xlsx)')} onClick={() => xlsxRef.current?.click()}><Icon name="plus" /><span>.xlsx</span></button>
          {activeCat.id !== STD_ID && <button className="btn icon" type="button" title={L('Hapus katalog ini dari komputer ini', 'Remove this catalogue from this computer')}
            onClick={() => { setCatalogs((o) => o.filter((c) => c.id !== activeCat.id)); set({ sepKatalog: STD_ID, sepInks: [], sepBaseAfter: 0 }) }}><Icon name="x" size={14} /></button>}
        </Row>
      )}
      <input ref={xlsxRef} type="file" accept=".xlsx" hidden onChange={importXlsx} />
      {(s.sepSet === 'auto' || s.sepSet === 'katalog') && (
        <Row label={L('Jumlah warna', 'Colours')}
          hint={L('Jumlah tinta di atas underbase (highlight putih ikut dihitung). Lebih banyak = lebih mirip desain, tapi lebih banyak screen. Dipilih otomatis supaya semua warna desain bisa dicapai; warna yang sama dengan kaos tidak perlu tinta.', 'Inks on top of the underbase (the highlight white counts). More = closer to the design, but more screens. Chosen automatically so every design colour can be reached; colours equal to the shirt need no ink.')}
          note={s.sepInks.length ? L('Tinta diatur manual', 'Inks set by hand') : null}>
          <Slider min={1} max={12} step={1} value={s.sepInks.length || s.sepColors} onCommit={(v) => set({ sepColors: v, sepInks: [], sepBaseAfter: 0 })} />
          <NumField className="short" value={s.sepInks.length || s.sepColors} min={1} max={12} step={1} digits={0} onCommit={(v) => set({ sepColors: v, sepInks: [], sepBaseAfter: 0 })} />
        </Row>
      )}
      <div className="inklist">
        {inkRows.map((r, k) => {
          const up = k > 0 && <button className="btn icon" type="button" title={L('Cetak lebih dulu', 'Print earlier')} onClick={() => moveRow(k, -1)}>↑</button>
          const down = k < inkRows.length - 1 && <button className="btn icon" type="button" title={L('Cetak lebih akhir', 'Print later')} onClick={() => moveRow(k, 1)}>↓</button>
          const film = sepFilms ? sepFilms.findIndex((f) => (r.base ? f.role === 'underbase' : f.src === r.i)) : -1
          const f = film >= 0 ? sepFilms[film] : null
          const no = f ? `${film + 1}.` : sepFilms ? '–' : `${k + 1}.`
          // rosette: each film's angle; interlock: which films have inverted dots
          const screen = f?.angle != null && (s.sepDots === 'roset' || (s.sepWhiteAngle != null && f.role !== 'ink')) ? ` · ${fmt(f.angle, 1)}°` : f?.inverted ? L(' · dibalik', ' · inverted') : ''
          if (r.base) {
            return (
              <div className="ink" key="base">
                <span className="sw" style={{ background: '#ffffff' }} />
                <span className="nm">{no} {L('Underbase putih', 'White underbase')}<small>{f ? `${Math.round(f.coverage * 100)} %${screen}` : '\u00a0'}</small></span>
                <span className="slot">{up}</span><span className="slot">{down}</span><span className="slot" />
              </div>
            )
          }
          const { ink, i } = r
          const role = f ? f.role : i === inkList.length - 1 && ink.rgb.every((v) => v === 255) && hasBase ? 'highlight' : 'ink'
          const label = inkLabel(ink.rgb, role, ink.name)
          const kind = role === 'ink' ? inkKind({ n: label }) : null
          return (
            <React.Fragment key={i}>
            <div className={'ink' + (sepFilms && !f ? ' off' : '')}>
              <input type="color" className="sw" value={rgbHex(ink.rgb)} title={L('Ganti warna tinta (warna bebas)', 'Change the ink colour (any colour)')}
                onChange={(e) => { const rgb = hexRgb(e.target.value); editRows((l) => l.map((x) => (x.ink && x === l.filter((y) => !y.base)[i] ? { ink: { ...x.ink, rgb, name: undefined } } : x))) }} />
              <span className="nm">{no} {label}
                {kind && <small className="inkwarn">{kind === 'neon' ? L('Neon: di pratinjau lebih kusam dari aslinya; cetak di atas underbase penuh', 'Neon: duller in the preview than in print; print on a full underbase') : L('Metalik: kilaunya tidak tampak di pratinjau; biasanya dicetak terakhir, padat', 'Metallic: its shine does not show in the preview; usually printed last, solid')}</small>}
                <small>{rgbHex(ink.rgb).toUpperCase()}{f ? ` · ${Math.round(f.coverage * 100)} %${screen}` : sepFilms ? L(' · tidak dipakai', ' · not used') : ''}</small></span>
              {sim && (
                <span className="boost" title={L('Boost tinta ini: titiknya dibuat lebih rapat (+%), seperti "Boost Red" manual. Biasanya tidak perlu.', 'Boost this ink: its dots made denser (+%), like a manual "Boost Red". Usually not needed.')}>
                  <NumField className="tiny" value={Math.round((ink.boost || 0) * 100)} min={0} max={100} step={5} digits={0}
                    onCommit={(v) => editRows((l) => l.map((x) => (x.ink && x === l.filter((y) => !y.base)[i] ? { ink: { ...x.ink, boost: v / 100 } } : x)))} />
                </span>
              )}
              <span className="slot">{up}</span><span className="slot">{down}</span>
              <span className="slot"><button className={'btn icon' + (inkPick === i ? ' on' : '')} type="button" title={L('Pilih tinta dari katalog (Pantone / merek mana pun)', 'Pick an ink from a catalogue (Pantone / any brand)')}
                onClick={() => { setInkPick(inkPick === i ? null : i); setPickQ('') }}><Icon name="layers" size={14} /></button></span>
              <span className="slot">{inkList.length > 1 && <button className="btn icon" type="button" title={L('Hapus tinta ini', 'Remove this ink')} onClick={() => editRows((l) => { const ink = l.filter((y) => !y.base)[i]; return l.filter((x) => x !== ink) })}><Icon name="x" size={14} /></button>}</span>
            </div>
            {inkPick === i && (
              <div className="catpick">
                <div className="catpick-top">
                  <select value={pickCatId === '*' ? '*' : (pickCats[0] || activeCat).id} onChange={(e) => setPickCatId(e.target.value)}>
                    {allCats.map((c) => <option key={c.id} value={c.id}>{c.id === STD_ID ? L(STD.name, STD.en) : c.name}</option>)}
                    {allCats.length > 1 && <option value="*">{L('Semua katalog', 'All catalogues')}</option>}
                  </select>
                  <input type="search" placeholder={L('Cari nama / kode / Pantone', 'Search name / code / Pantone')} value={pickQ} onChange={(e) => setPickQ(e.target.value)} />
                </div>
                <div className="catgrid">
                  {pickInks.map(({ ci, cat }, k) => {
                    const kd = inkKind(ci)
                    return (
                      <button key={k} type="button" className="catsw" title={`${inkLine(ci, cat)}${ci.s ? ` · ${ci.s}` : ''} · ${ci.h}${kd ? ` · ${kd}` : ''}`} onClick={() => chooseCatInk(i, ci, cat)}>
                        <span className="chip" style={{ background: ci.h }}>{kd === 'neon' ? 'N' : kd === 'metalik' ? 'M' : ''}</span>
                        <small>{ci.c && cat.id !== STD_ID ? ci.c : ci.n}</small>
                      </button>
                    )
                  })}
                  {!pickInks.length && <p className="small">{L('Tidak ada yang cocok', 'Nothing matches')}</p>}
                </div>
              </div>
            )}
            </React.Fragment>
          )
        })}
        <div className="btnrow">
          <button className="btn" type="button" disabled={inkList.length >= 12 || !inkList.length} onClick={() => editRows((l) => {
            // a new ink goes before a highlight white (that stays last)
            const lastInk = l.length && !l[l.length - 1].base && l[l.length - 1].ink.rgb.every((v) => v === 255) ? l.length - 1 : l.length
            return [...l.slice(0, lastInk), { ink: { rgb: [128, 128, 128], opacity: 0.6, boost: 0 } }, ...l.slice(lastInk)]
          })}><Icon name="plus" /><span>{L('Tinta', 'Ink')}</span></button>
          {s.sepInks.length > 0 && s.sepSet !== 'saya' && <button className="btn" type="button" onClick={() => set({ sepInks: [], sepBaseAfter: 0 })}><Icon name="reset" /><span>{s.sepSet === 'auto' ? L('Otomatis lagi', 'Automatic again') : L('Kembali ke set', 'Back to the set')}</span></button>}
          {inkList.length > 0 && <button className="btn" type="button" title={L('Simpan daftar tinta ini (urutan + boost) sebagai "Tinta saya", mis. tinta yang ada di gudang', 'Save this ink list (order + boost) as "My inks", e.g. the inks you have in stock')}
            onClick={() => { const v = { inks: inkList.map((x) => { const c = x.name ? null : catInk(x.rgb); const name = x.name || (c ? inkLine(c.ink, c.cat) : undefined); return { rgb: x.rgb, opacity: x.opacity, boost: x.boost || 0, ...(name ? { name } : {}) } }), baseAfter: baseAt }; setMyInks(v); set({ sepSet: 'saya', sepInks: v.inks, sepBaseAfter: v.baseAfter }); flash(L('Tersimpan sebagai Tinta saya', 'Saved as My inks')) }}>
            <Icon name="save" /><span>{L('Simpan sbg Tinta saya', 'Save as My inks')}</span></button>}
        </div>
        <p className="small">{L('Urutan = urutan cetak: underbase, warna terang ke gelap, hitam, highlight putih terakhir (↑ ↓ juga bisa memindah underbase, mis. abu dicetak sebelum underbase). Klik kotak warna untuk warna bebas, atau tombol katalog untuk memilih tinta Pantone / merek mana pun (N = neon, M = metalik).', 'Order = print order: underbase, light to dark colours, black, highlight white last (↑ ↓ can move the underbase too, e.g. grey printed before it). Click a swatch for any colour, or the catalogue button to pick a Pantone / any brand ink (N = neon, M = metallic).')}
          {sim && L(' Angka di samping = boost tinta (%).', ' The number next to it = ink boost (%).')}</p>
      </div>
      {dE != null && (
        <Row label={L('Kemiripan', 'Match')} hint={L('Selisih warna rata-rata antara desain dan simulasi hasil cetak (ΔE, dilihat dari jarak dekat). Di bawah 3 hampir tidak terlihat, di bawah 6 mirip. Simulasi memakai titik film yang sebenarnya, jadi pratinjau = perkiraan hasil cetak.', 'Average colour difference between the design and the simulated print (ΔE, seen from close by). Below 3 hardly visible, below 6 close. The simulation uses the real film dots, so the preview is what the print should look like.')}
          note={dEText}>
          <span className="mono">ΔE {fmt(dE, 1)}</span>
        </Row>
      )}
      <Row label="Underbase" hint={L('Lapisan putih di bawah warna supaya warna tetap terang di kaos gelap. Otomatis: dipakai di kaos gelap / berwarna, tidak di kaos putih. Ketebalannya dihitung per warna: padat di bawah warna terang dan jenuh, tipis di bawah warna gelap.', 'White layer under the colours so they stay bright on dark shirts. Auto: used on dark / coloured shirts, not on white. Its density is computed per colour: solid under bright and saturated colours, thin under dark ones.')}>
        <Seg value={s.sepBase} onChange={(v) => set({ sepBase: v })} options={[['auto', L('Otomatis', 'Auto')], ['ya', L('Ya', 'Yes')], ['tidak', L('Tidak', 'No')]]} />
      </Row>
      <Row label="Choke" hint={L('Underbase dibuat sedikit lebih kecil dari warna di atasnya supaya putihnya tidak mengintip di tepi bila registrasi bergeser sedikit. 0,1–0,3 mm umum dipakai.', 'The underbase is made slightly smaller than the colours on it so the white does not peek out at the edges when registration shifts a little. 0.1–0.3 mm is common.')}>
        <NumField className="short" value={s.sepChoke} min={0} max={2} step={0.05} digits={2} onCommit={(v) => set({ sepChoke: v })} />
        <span className="small"> mm</span>
      </Row>
      <Row label={L('Highlight putih', 'Highlight white')} hint={L('Tinta putih kedua, dicetak terakhir, hanya di bagian paling putih/terang supaya bersinar. Terlalu banyak membuat warna pucat seperti kapur, jadi dipakai seperlunya.', 'A second white, printed last, only on the whitest / brightest parts so they pop. Too much makes colours chalky, so it is used only where needed.')}>
        <input type="checkbox" checked={!!s.sepHighlight} onChange={(e) => set({ sepHighlight: e.target.checked, ...(s.sepSet === 'saya' ? {} : { sepInks: [], sepBaseAfter: 0 }) })} />
      </Row>
      <Row label={L('Expand warna', 'Colour expand')} note={s.sepExpand > 0 ? `≈ ${fmt((s.sepExpand / s.dpi) * 25.4, 2)} mm` : null}
        hint={L('Film warna dibuat sedikit lebih lebar (piksel pada DPI cetak), supaya kalau registrasi bergeser tidak ada celah atau underbase yang mengintip di tepi warna. Bersama Choke underbase. 1–3 px umum dipakai; 0 = mati.', 'The colour films are made slightly wider (pixels at the print DPI), so a small registration shift leaves no gap or peeking underbase at colour edges. Works with the underbase choke. 1–3 px is common; 0 = off.')}>
        <NumField className="short" value={s.sepExpand} min={0} max={20} step={1} digits={0} onCommit={(v) => set({ sepExpand: v })} />
        <span className="small"> px</span>
      </Row>
      <Row label={L('Film kosong', 'Empty films')}
        hint={L('Tinta yang mencetak kurang dari persentase ini dari luas desain tidak dibuatkan film (hemat screen); tinta lain dihitung ulang tanpa tinta itu. 0 = semua tinta dipakai.', 'Inks printing on less than this share of the design get no film (saves a screen); the other inks are computed again without them. 0 = keep every ink.')}>
        <span className="small">{L('buang di bawah', 'drop below')} </span>
        <NumField className="short" value={s.sepDrop} min={0} max={10} step={0.1} digits={1} onCommit={(v) => set({ sepDrop: v })} />
        <span className="small"> %</span>
      </Row>
      {s.sepMethod === 'simulasi' && (
        <Row label={L('Hemat tinta', 'Ink saver')} hint={L('Underbase, highlight dan tinta dikurangi di bagian yang warnanya hanya berubah sedikit: lebih murah, cetakan lebih lembut. 50 % ≈ 12–25 % tinta lebih sedikit, 100 % lebih hemat lagi (mendekati film buatan tangan), tapi warna turun: diuji di 6 desain, 50 % menambah selisih warna 0,1–1,7 ΔE. Bandingkan angka Kemiripan.', 'Underbase, highlight and inks reduced where the colour changes only a little: cheaper, a softer print. 50 % ≈ 12–25 % less ink, 100 % even less (close to hand-made films), but colours drop: tested on 6 designs, 50 % adds 0.1–1.7 ΔE. Compare the Match number.')}>
          <Slider min={0} max={100} step={5} value={s.sepSaver} onCommit={(v) => set({ sepSaver: v })} />
          <NumField className="short" value={s.sepSaver} min={0} max={100} step={5} digits={0} onCommit={(v) => set({ sepSaver: v })} />
        </Row>
      )}
      {s.sepMethod === 'simulasi' && (
        <Row label={L('Titik putih maks.', 'Max white dot')} hint={L('Gradasi halus: di bagian gradasi, underbase dan highlight paling banyak sekian persen, jadi gradasi terang (kulit, tengkorak, langit) tetap bertitik dan tidak jadi petak putih rata. Warna yang memang rata di desain (tulisan, garis putih) tetap padat. 85 % seperti film buatan tangan; 100 = mati.', 'Smooth gradients: in gradients the underbase and highlight print at most this much, so light gradients (skin, skulls, sky) keep their dots instead of turning into flat white patches. Colours that are flat in the design (lettering, white outlines) stay solid. 85 % like hand-made films; 100 = off.')}>
          <Slider min={70} max={100} step={1} value={s.sepMaxDot} onCommit={(v) => set({ sepMaxDot: v })} />
          <NumField className="short" value={s.sepMaxDot} min={50} max={100} step={1} digits={0} onCommit={(v) => set({ sepMaxDot: v })} />
        </Row>
      )}
      {s.sepSet === 'auto' && shirtTone(s) === 'gelap' && (
        <Row label={L('Tinta kuat', 'Strong inks')} hint={L('Kaos gelap: tinta otomatis hanya warna kuat, tanpa tinta krem / pastel. Bagian terang dibuat oleh underbase dan highlight putih, tinta memberi warnanya, seperti film buatan tangan. Tinta krem cenderung dicetak penuh dan membuat petak datar di kulit / tengkorak.', 'Dark shirts: automatic inks are strong colours only, no cream / pastel ink. The white base and highlight make the light tones, the inks the hue, as in hand-made films. A cream ink tends to print full and makes flat patches in skin / skulls.')}>
          <input type="checkbox" checked={!!s.sepStrongInks} onChange={(e) => set({ sepStrongInks: e.target.checked, sepInks: [], sepBaseAfter: 0 })} />
        </Row>
      )}
      {s.sepMethod !== 'spot' && (
        <Row label={L('Boost warna', 'Colour boost')} note={s.sepBoost > 0 ? L('ΔE dihitung terhadap desain asli', 'ΔE is measured against the original design') : null}
          hint={L('Warna dibuat lebih jenuh dari desain (+%), karena hasil sablon di kaos sering terlihat lebih kalem dari layar. 0 = sesuai desain. Untuk satu tinta saja, pakai angka boost di daftar tinta.', 'Colours aimed more saturated than the design (+%), since prints on shirts often look calmer than the screen. 0 = as designed. For one ink only, use the boost number in the ink list.')}>
          <Slider min={0} max={30} step={1} value={s.sepBoost} onCommit={(v) => set({ sepBoost: v })} />
          <NumField className="short" value={s.sepBoost} min={0} max={50} step={1} digits={0} onCommit={(v) => set({ sepBoost: v })} />
        </Row>
      )}
      {s.sepMethod === 'simulasi' && (
        <>
          <Row label={L('Gaya titik', 'Dot style')}
            hint={L('Satu sudut: semua film di sudut yang sama, titik bertumpuk (titik di atas titik), paling aman dari moire. Roset: tiap film sudut berbeda (seperti CMYK), titik menyebar. Interlock: separuh warna (berselang menurut hue) memakai titik yang dibalik, jadi warna yang bersebelahan dicetak berdampingan, tidak menindih: gradasi antar warna (mis. toska ke biru) lebih bersih. Bandingkan angka Kemiripan untuk memilih.', 'One angle: every film at the same angle, dots stacked (dot on dot), safest against moiré. Rosette: a different angle per film (like CMYK), dots spread. Interlock: half of the colours (alternating by hue) use inverted dots, so neighbouring colours print side by side instead of on top of each other: cleaner blends between colours (e.g. teal to blue). Compare the Match number to choose.')}>
            <Seg value={s.sepDots} onChange={(v) => set({ sepDots: v })} options={[['satu-sudut', L('Satu sudut', 'One angle')], ['roset', L('Roset', 'Rosette')], ['interlock', 'Interlock']]} />
          </Row>
          <Row label={L('Film (LPI)', 'Film (LPI)')} hint={L('Kerapatan titik halftone di semua film. 45–55 LPI umum untuk kaos (screen 230–305 mesh). Sudut = sudut film (Roset: sudut underbase; warna lain bergeser 15–75°).', 'Halftone dot spacing on every film. 45–55 LPI is common for shirts (230–305 mesh screens). Angle = the film angle (Rosette: the underbase angle; the colours are offset by 15–75°).')}>
            <NumField className="short" value={s.sepLpi} min={10} max={100} step={1} digits={0} onCommit={(v) => set({ sepLpi: v })} />
            <span className="small">LPI · {L('sudut', 'angle')}</span>
            <NumField className="short" value={s.sepAngle} min={-90} max={90} step={0.5} digits={1} onCommit={(v) => set({ sepAngle: v })} />
            <span className="small">°</span>
          </Row>
          {s.sepDots !== 'roset' && <Row label={L('Sudut putih', 'White angle')} hint={L('Film putih (underbase dan highlight) di sudut sendiri, mis. 15° dari warna (22,5° → 7,5°), seperti film pecah warna buatan tangan: tidak moire dengan warna, dan bila screen putih sedikit bergeser saat cetak, warnanya tetap rapi (diuji: geser 0,25 mm, selisih warna naik 0,4 saja, bukan 1,4). Sama = satu sudut dengan warna.', 'The white films (underbase and highlight) on their own angle, e.g. 15° from the colours (22.5° → 7.5°), like hand-made film separations: no moiré with the colours, and when the white screen shifts a little on press the colours stay clean (tested: a 0.25 mm shift raises the colour difference by 0.4, not 1.4). Same = one angle with the colours.')}>
            <Seg value={s.sepWhiteAngle == null ? 'sama' : 'beda'} onChange={(v) => set({ sepWhiteAngle: v === 'sama' ? null : Number(s.sepAngle) - 15 })} options={[['sama', L('Sama', 'Same')], ['beda', L('Sendiri', 'Own')]]} />
            {s.sepWhiteAngle != null && (
              <>
                <NumField className="short" value={s.sepWhiteAngle} min={-90} max={90} step={0.5} digits={1} onCommit={(v) => set({ sepWhiteAngle: v })} />
                <span className="small">°</span>
              </>
            )}
          </Row>}
          <Row label={L('Bentuk titik', 'Dot shape')}>
            <select value={s.sepShape} onChange={(e) => set({ sepShape: e.target.value })}>
              {SHAPES.filter((o) => ['elips', 'bulat', 'bulat-murni', 'kotak', 'berlian', 'garis'].includes(o[0])).map((o) => <option key={o[0]} value={o[0]}>{optLabel(o)}</option>)}
            </select>
          </Row>
        </>
      )}
      <Row label={L('Lindungi teks', 'Protect text')} hint={L('Tulisan dicetak dengan satu tinta terdekat, padat (tanpa titik), supaya tajam.', 'Lettering prints in the nearest single ink, solid (no dots), so it stays sharp.')}>
        <input type="checkbox" checked={!!s.protectText} onChange={(e) => set({ protectText: e.target.checked })} />
      </Row>
      <Row label={L('Tepi film', 'Film border')} hint={L('Ruang di sekeliling desain pada setiap film, berisi tanda registrasi dan nama film.', 'Space around the design on every film, with registration marks and the film name.')}>
        <NumField className="short" value={s.sepMargin} min={0} max={50} step={1} digits={0} onCommit={(v) => set({ sepMargin: v })} />
        <span className="small"> mm</span>
      </Row>
    </>
  )
  const sectionTekstur = <TextureLayers layers={s.distress || []} onChange={(d) => set({ distress: d })} />
  const sectionBody = { gambar: sectionGambar, kaos: sectionKaos, ukuran: sectionUkuran, halftone: sectionHalftone, warna: sectionWarna, tekstur: sectionTekstur }[sablon && sec === 'halftone' ? 'warna' : !sablon && sec === 'warna' ? 'halftone' : sec]

  const sideDesain = (
    <aside className="side">
      <nav className="sidetabs">
        {SECTIONS.map(([v, ic, l]) => (
          <button key={v} type="button" className={sec === v ? 'on' : ''} onClick={() => setSec(v)}>
            <Icon name={ic} size={18} /><span>{l}</span>
          </button>
        ))}
      </nav>
      <div className="scroll">{sectionBody}</div>
      <footer className="foot">
        {alerts}
        <button className="btn primary wide big" disabled={!canExport} onClick={exportPng} type="button" title={sablon ? L('Simpan film per tinta + simulasi + urutan cetak (Ctrl E)', 'Save the films per ink + simulation + print order (Ctrl E)') : L('Simpan PNG siap cetak (Ctrl E)', 'Save the print-ready PNG (Ctrl E)')}>
          <Icon name="download" size={18} /><span>{sablon ? L(`Export ${res?.films?.length || ''} film`, `Export ${res?.films?.length || ''} films`) : 'Export PNG'}</span>
        </button>
        {!sablon && <button className="link testsheet" onClick={makeTestSheet} type="button" title={L('22 × 30 cm: grid LPI × bentuk titik, tes titik/lubang minimum, chart warna', '22 × 30 cm: LPI × dot shape grid, min. dot/hole test, colour chart')}>
          <Icon name="sheet" size={13} /><span>{L('Lembar uji cetak', 'Test print sheet')} ({s.shirtType === 'putih' ? L('kaos putih', 'white shirt') : L('kaos hitam', 'black shirt')}, {s.dpi} DPI)…</span>
        </button>}
        {toastEl}
      </footer>
    </aside>
  )

  const sideBatch = (
    <aside className="side">
      <div className="sidehead">
        <Icon name="files" size={18} /><b>{L('Export banyak', 'Batch export')}</b>
        <Help text={L('Banyak gambar sekaligus dengan pengaturan tab Desain (kaos, DPI, halftone, tekstur). Background dihapus otomatis per gambar dan auto-trim ikut; crop, penghapus dan putar tidak ikut (khusus satu gambar). File bisa juga diseret ke jendela.', 'Many images at once with the Design tab settings (shirt, DPI, halftone, texture). The background is removed per image and auto-trim applies; crop, eraser and rotation do not (they belong to one image). You can also drop files on the window.')} />
      </div>
      <div className="scroll">
        <Row label={L('Ukuran', 'Size')} hint={s.batchFit === 'kotak' ? L('Muat dalam kotak: setiap desain dibesarkan sebesar mungkin tanpa melewati lebar maupun tinggi kotak; rasio tetap.', 'Fit in box: each design as large as possible within the box width and height; ratio kept.') : L('Lebar tetap: semua desain selebar ukuran di tab Desain → Ukuran; tinggi ikut rasio.', 'Fixed width: every design as wide as set in Design → Size; height follows.')}>
          <Seg value={s.batchFit} onChange={(v) => set({ batchFit: v })} options={[['lebar', L('Lebar tetap', 'Fixed width')], ['kotak', L('Muat kotak', 'Fit in box')]]} />
        </Row>
        {s.batchFit === 'kotak' ? (
          <Row label={L('Kotak (cm)', 'Box (cm)')}>
            <NumField className="short" value={s.batchW} min={1} max={300} step={0.5} digits={1} onCommit={(v) => set({ batchW: v })} />
            <span className="small">×</span>
            <NumField className="short" value={s.batchH} min={1} max={300} step={0.5} digits={1} onCommit={(v) => set({ batchH: v })} />
          </Row>
        ) : <p className="small">{L('Lebar', 'Width')} {fmt(s.widthCm)} cm · {s.dpi} DPI</p>}
        <Row label={L('Perbesar AI', 'AI Upscale')} hint={L(`Gambar dengan resolusi efektif di bawah ${LOW_DPI} DPI diperbesar dulu dengan AI (2× atau 4×), kehalusan seperti di tab Desain → Gambar. Lebih lama, hasil lebih tajam.`, `Images below ${LOW_DPI} effective DPI are upscaled with AI first (2× or 4×), with the AI style of Design → Image. Slower, sharper.`)}>
          <input type="checkbox" checked={s.batchAi} disabled={batch.running} onChange={(e) => set({ batchAi: e.target.checked })} />
        </Row>
        <div className="btnrow">
          <button className="btn" type="button" disabled={batch.running} onClick={pickBatchFiles}><Icon name="image" /><span>{L('Pilih gambar…', 'Choose images…')}</span></button>
          <button className="btn" type="button" disabled={batch.running || !batch.files.length} onClick={pickBatchDir}><Icon name="folder" /><span>{L('Folder hasil…', 'Output folder…')}</span></button>
        </div>
        {batch.files.length > 0 && (
          <p className="small">
            <b>{L(`${batch.files.length} gambar`, `${batch.files.length} images`)}</b>: {batch.files.slice(0, 3).map((f) => f.split(/[\\/]/).pop()).join(', ')}{batch.files.length > 3 ? ', …' : ''}
            <br />{L('Disimpan ke:', 'Saved to:')} <span className="mono">{batch.dir}</span>
          </p>
        )}
        {batch.running && (
          <p className="small">
            {batch.cur ? L(`Gambar ${batch.cur.i + 1} dari ${batch.cur.n}: ${batch.cur.name}`, `Image ${batch.cur.i + 1} of ${batch.cur.n}: ${batch.cur.name}`) : L('Menyiapkan…', 'Preparing…')}
            {batch.cur?.state === 'render' ? ` · ${Math.round(batch.cur.frac * 100)} %` : batch.cur?.state === 'ai' ? ` · AI ${Math.round(batch.cur.frac * 100)} %` : ''}
            <span className="pbar"><span style={{ width: `${batch.cur ? ((batch.cur.i + (batch.cur.state === 'render' ? batch.cur.frac : 0)) / batch.cur.n) * 100 : 0}%` }} /></span>
          </p>
        )}
        {batch.done && <p className="small">{batch.done.cancelled ? L('Dihentikan', 'Stopped') : L('Selesai', 'Finished')}: {L(`${batch.done.ok} berhasil, ${batch.done.failed} gagal.`, `${batch.done.ok} done, ${batch.done.failed} failed.`)}</p>}
        {batch.log.length > 0 && (
          <ul className="batchlog">
            {batch.log.map((it) => (
              <li key={it.i} className={it.state === 'ok' ? 'ok' : 'bad'} title={it.msg}>
                {it.state === 'ok' ? '✓' : '✗'} {it.name}{it.state === 'gagal' ? ` — ${it.msg}` : ` → ${it.msg.split(/[\\/]/).pop()}`}
              </li>
            ))}
          </ul>
        )}
      </div>
      <footer className="foot">
        {!batch.running ? (
          <button className="btn primary wide big" type="button" disabled={!batch.files.length || !batch.dir} onClick={startBatch}>
            <Icon name="download" size={18} /><span>{batch.files.length ? L(`Mulai export ${batch.files.length} gambar`, `Export ${batch.files.length} images`) : L('Mulai export', 'Start export')}</span>
          </button>
        ) : (
          <button className="btn wide big" type="button" onClick={() => invoke('batch_cancel')}><Icon name="x" size={18} /><span>{L('Hentikan', 'Stop')}</span></button>
        )}
        {toastEl}
      </footer>
    </aside>
  )

  return (
    <div className="shell">
      {topbar}
      <div className="app">
        {stage}
        {tab === 'batch' ? sideBatch : sideDesain}
      </div>
      <UpdateBar u={upd} />
    </div>
  )
}

// ------------------------------------------------------------------ texture picker
const TEX_FILTERS = () => [{ name: L('Tekstur', 'Textures'), extensions: ['png', 'jpg', 'jpeg', 'tif', 'tiff', 'webp', 'bmp'] }]

// up to 3 textures stacked on one design: holes of every layer are joined, fades multiply
const MAX_TEX_LAYERS = 3
function TextureLayers({ layers, onChange }) {
  const [act, setAct] = useState(0)
  const n = layers.length
  const i = Math.min(act, Math.max(0, n - 1))
  const setLayer = (l) => {
    if (l) onChange(n ? layers.map((x, k) => (k === i ? l : x)) : [l])
    else {
      // "Tanpa tekstur" removes this layer
      onChange(layers.filter((_, k) => k !== i))
      setAct(Math.max(0, i - 1))
    }
  }
  const add = () => {
    if (n >= MAX_TEX_LAYERS) return
    // a new layer starts turned and shifted, so the same texture twice does not line up
    const v = n * 90
    onChange([...layers, { ...newTexture(null), angle: v, offset_mm: [n * 17, n * -11] }])
    setAct(n)
  }
  const label = (l) => (l?.path ? l.path.replace(/^lib:/, '').split(/[\\/]/).pop().replace(/\.[^.]+$/, '').replace(/[-_]/g, ' ') : L('pilih tekstur', 'choose texture'))
  return (
    <>
      {n > 0 && (
        <div className="layertabs">
          <Help text={L('Sampai 3 tekstur bisa ditumpuk: klik "+ Lapisan". Lubang semua lapisan digabung, pudar ditumpuk. Pilih "Tanpa tekstur" untuk membuang lapisan yang dipilih.', 'Stack up to 3 textures: click "+ Layer". Holes of all layers add up, fades multiply. Choose "No texture" to remove the selected layer.')} />
          {layers.map((l, k) => (
            <button key={k} type="button" className={k === i ? 'chip on' : 'chip'} onClick={() => setAct(k)} title={label(l)}>
              {k + 1}. {label(l)}
            </button>
          ))}
          {n < MAX_TEX_LAYERS && layers[n - 1]?.path && (
            <button type="button" className="chip" onClick={add} title={L('Tumpuk tekstur lain di atasnya (maks. 3)', 'Stack another texture on top (max. 3)')}><Icon name="plus" size={11} /> {L('Lapisan', 'Layer')}</button>
          )}
        </div>
      )}
      <TexturePicker layer={layers[i]} onChange={setLayer} />
    </>
  )
}

function TexturePicker({ layer, onChange }) {
  const [lib, setLib] = useState([]) // built-in + own library (own: true)
  const [ext, setExt] = useState(null) // chosen file outside the library (older projects)
  const [err, setErr] = useState('')
  const [adding, setAdding] = useState(false)
  const [ask, setAsk] = useState(null) // own texture waiting for "Hapus?" confirmation
  useEffect(() => {
    invoke('textures').then(setLib).catch((e) => setErr(String(e)))
  }, [])
  const upd = (patch) => onChange({ ...layer, ...patch })
  const choose = (path) => onChange(path ? { ...(layer || newTexture(path)), path } : null)
  const sel = layer?.path
  useEffect(() => {
    if (!sel || sel.startsWith('lib:') || lib.some((t) => t.path === sel)) {
      setExt(null)
      return
    }
    invoke('texture_file', { path: sel }).then(setExt).catch(() => setExt(null))
  }, [sel, lib])
  // several files at once; they are copied into the app's own library and stay there
  const addFiles = async () => {
    const p = await open({ multiple: true, filters: TEX_FILTERS() })
    if (!p) return
    setAdding(true)
    setErr('')
    try {
      const r = await invoke('add_textures', { paths: Array.isArray(p) ? p : [p] })
      setLib((xs) => {
        const have = new Set(xs.map((x) => x.path))
        return [...xs, ...r.added.filter((a) => !have.has(a.path))]
      })
      if (r.added.length === 1) choose(r.added[0].path)
      if (r.failed.length) setErr(L(`Tidak bisa ditambahkan: ${r.failed.join(' · ')}`, `Could not add: ${r.failed.join(' · ')}`))
    } catch (e) {
      setErr(String(e))
    } finally {
      setAdding(false)
    }
  }
  const remove = async (path) => {
    setAsk(null)
    try {
      await invoke('remove_texture', { path })
      setLib((xs) => xs.filter((x) => x.path !== path))
      if (sel === path) choose(null)
    } catch (e) {
      setErr(String(e))
    }
  }
  const variasi = () => {
    const r = (n) => Math.floor(Math.random() * n)
    upd({ angle: [0, 90, 180, 270][r(4)], mirror: r(2) === 1, offset_mm: [r(81) - 40, r(81) - 40] })
  }
  const items = [...lib, ...(ext ? [{ ...ext, ext: true }] : [])]
  const nOwn = lib.filter((t) => t.own).length
  const mode = layer?.mode || 'lubang'
  return (
    <>
      <div className="texgrid">
        <button type="button" className={!sel ? 'tex on' : 'tex'} onClick={() => choose(null)} title={L('Tanpa tekstur', 'No texture')}>
          <span className="texnone">{L('Tanpa', 'None')}</span>
          <span className="texname">{L('Tanpa tekstur', 'No texture')}</span>
        </button>
        {items.map((t) => (
          <div className="texwrap" key={t.path}>
            <button type="button" className={sel === t.path ? 'tex on' : 'tex'} onClick={() => choose(t.path)}
              title={`${t.name} · ${t.w}×${t.h} px${t.own ? L(' · pustaka saya', ' · my library') : t.ext ? L(' · file di luar pustaka', ' · file outside the library') : ''}`}>
              <img src={t.thumb} alt={t.name} draggable={false} />
              <span className="texname">{t.own ? '★ ' : ''}{t.name}</span>
            </button>
            {t.own && (ask === t.path ? (
              <button type="button" className="texdel ask" onClick={() => remove(t.path)} onMouseLeave={() => setAsk(null)} title={L('Klik lagi untuk menghapus dari pustaka', 'Click again to remove from the library')}>{L('Hapus?', 'Delete?')}</button>
            ) : (
              <button type="button" className="texdel" onClick={() => setAsk(t.path)} title={L('Hapus dari pustaka saya', 'Remove from my library')}>×</button>
            ))}
          </div>
        ))}
        <button type="button" className="tex" disabled={adding} onClick={addFiles} title={L('Tambah satu atau banyak file tekstur (PNG/JPG/TIFF/WebP). Disimpan di pustaka aplikasi, tidak perlu upload lagi.', 'Add one or more texture files (PNG/JPG/TIFF/WebP). They stay in the app library, no need to add them again.')}>
          <span className="texnone">{adding ? '…' : '+'}</span>
          <span className="texname">{adding ? L('Menambahkan…', 'Adding…') : L('Tambah…', 'Add…')}</span>
        </button>
      </div>
      {err && <p className="error">{err}</p>}
      {nOwn > 0 && <p className="small">{L(`★ = pustaka saya (${nOwn}), tersimpan di aplikasi.`, `★ = my library (${nOwn}), kept in the app.`)}</p>}
      {layer && (
        <>
          <Row label={L('Cara', 'Mode')} hint={L('Lubang: area gelap tekstur jadi lubang di tinta. Campur: lubang + memudar di area abu-abu (paling alami). Pudar: hanya memudarkan nada (titik mengecil).', 'Holes: dark texture areas become holes in the ink. Mix: holes + fading in grey areas (most natural). Fade: only fades the tones (smaller dots).')}>
            <Seg value={mode} onChange={(v) => upd({ mode: v })} options={[['lubang', L('Lubang', 'Holes')], ['campur', L('Campur', 'Mix')], ['pudar', L('Pudar', 'Fade')]]} />
          </Row>
          {mode !== 'pudar' && (
            <Row label={L('Kekuatan', 'Strength')}>
              <input type="range" min="0.05" max="0.95" step="0.01" value={layer.threshold ?? 0.5} onChange={(e) => upd({ threshold: Number(e.target.value) })} />
              <span className="small mono">{Math.round((layer.threshold ?? 0.5) * 100)}%</span>
            </Row>
          )}
          {mode !== 'lubang' && (
            <Row label={L('Pudar', 'Fade')}>
              <input type="range" min="0" max="1" step="0.01" value={layer.amount ?? 0.5} onChange={(e) => upd({ amount: Number(e.target.value) })} />
              <span className="small mono">{Math.round((layer.amount ?? 0.5) * 100)}%</span>
            </Row>
          )}
          <Row label={L('Ukuran', 'Scale')} hint={L('100% = tekstur menutupi seluruh desain.', '100% = the texture covers the whole design.')}>
            <input type="range" min="0.5" max="3" step="0.05" value={layer.zoom ?? 1} onChange={(e) => upd({ zoom: Number(e.target.value) })} />
            <span className="small mono">{Math.round((layer.zoom ?? 1) * 100)}%</span>
          </Row>
          <Row label={L('Putar', 'Rotate')}>
            <Seg value={String(((Number(layer.angle) || 0) % 360 + 360) % 360)} onChange={(v) => upd({ angle: Number(v) })} options={[['0', '0°'], ['90', '90°'], ['180', '180°'], ['270', '270°']]} />
          </Row>
          <Row label="">
            <Slider min={0} max={359} step={1} value={((Number(layer.angle) || 0) % 360 + 360) % 360} onCommit={(v) => upd({ angle: v })} format={(v) => `${v}°`} />
          </Row>
          <Row label={L('Cermin / balik', 'Mirror / invert')} hint={L('Cermin: tekstur dibalik kiri-kanan. Balik: hitam dan putih tekstur ditukar (yang aus jadi yang utuh).', 'Mirror: flip the texture left-right. Invert: swap its black and white (worn becomes solid).')}>
            <button className={layer.mirror ? 'btn icon on' : 'btn icon'} type="button" title={L('Cermin kiri-kanan', 'Mirror')} onClick={() => upd({ mirror: !layer.mirror })}><Icon name="flip" /></button>
            <button className={layer.invert ? 'btn on' : 'btn'} type="button" title={L('Tukar hitam/putih tekstur', 'Swap texture black/white')} onClick={() => upd({ invert: !layer.invert })}>{L('Balik', 'Invert')}</button>
            <button className="btn" type="button" onClick={variasi} title={L('Putar, cermin dan geser acak', 'Random rotate, mirror and shift')}><Icon name="refresh" /><span>{L('Acak', 'Random')}</span></button>
          </Row>
          <Row label={L('Geser X / Y', 'Shift X / Y')} hint={L('Geser tekstur dalam mm.', 'Move the texture, in mm.')}>
            <NumField className="short" value={layer.offset_mm?.[0] ?? 0} min={-500} max={500} step={1} onCommit={(v) => upd({ offset_mm: [v, layer.offset_mm?.[1] ?? 0] })} />
            <NumField className="short" value={layer.offset_mm?.[1] ?? 0} min={-500} max={500} step={1} onCommit={(v) => upd({ offset_mm: [layer.offset_mm?.[0] ?? 0, v] })} />
            <span className="small">mm</span>
          </Row>
        </>
      )}
    </>
  )
}

// ------------------------------------------------------------------ viewer
function Viewer({ img, res, mode, ov, vgen, backdrop, fetchTile, fetchOrig, cropping, crop, setCrop, onCropExit, onOpen, viewer, onView, brush, strokes, hatch }) {
  if (!img) {
    return (
      <div className="viewer empty" onClick={onOpen}>
        <div>
          <p className="big">{L('Buka atau seret gambar ke sini', 'Open or drop an image here')}</p>
          <p className="small">{L('PNG, JPG, WebP, TIFF · desain dengan background hitam, putih, warna rata, atau transparan', 'PNG, JPG, WebP, TIFF · designs on black, white, flat colour or transparent backgrounds')}</p>
        </div>
      </div>
    )
  }
  if (mode === 'asli' || !res) {
    return (
      <CanvasViewer ref={viewer} w={img.w} h={img.h} gen={`o${img.id}`} overview={{ source: ov.orig }} fetchTile={fetchOrig}
        mode="asli" backdrop={backdrop} cropping={cropping} crop={crop} setCrop={setCrop} onCropExit={onCropExit} onView={onView}
        brush={brush} strokes={strokes} />
    )
  }
  return (
    <CanvasViewer ref={viewer} w={res.w} h={res.h} gen={vgen ?? res.gen} overview={{ source: ov.source, result: ov.result, text: ov.text }} fetchTile={fetchTile}
      mode={mode === 'latar' ? 'asli' : mode} backdrop={backdrop} onView={onView} brush={brush} hatch={hatch} />
  )
}
