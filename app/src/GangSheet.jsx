// Gang sheet builder: print-ready PNGs (exported by StudioZea at one DPI) packed on a
// roll of film of a fixed width, with copies, a gap between designs, a margin at the
// edges and optional 90° turns. The pixels are copied exactly (no resampling), so the
// halftone dots print as exported. Raw images (JPG, PNG with a background or soft
// edges) are made print-ready first with the Design tab settings. Layout by the engine
// (gang::pack_with), drawn here; a design dragged with the mouse stays where it is put
// (a pin) and the others are packed around it.
import React, { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { open, save } from '@tauri-apps/plugin-dialog'
import { Card, NumField, Row, Seg, fmt, unpack } from './ui.jsx'
import { cssVar } from './theme.js'
import { L, getLang } from './i18n.js'

const WIDTHS = () => [[L('22 inci', '22 in'), 55.88], ['58 cm', 58], ['60 cm', 60], ['30 cm', 30]]
const IMAGES = () => [{ name: L('Gambar (PNG hasil export, atau JPG / PNG / TIFF / WebP mentah)', 'Images (exported PNG, or raw JPG / PNG / TIFF / WebP)'), extensions: ['png', 'jpg', 'jpeg', 'tif', 'tiff', 'webp', 'bmp'] }]

export default function GangSheet({ s, set, items, setItems, pins, setPins, job, tabs, flash, toast, addCurrent, run, setRun, runErr, setRunErr, addRef }) {
  const [err, setErr] = useState('') // layout problem
  const [sheets, setSheets] = useState([])
  const [adding, setAdding] = useState(null) // name of the file being added
  const [addErr, setAddErr] = useState([]) // files that could not be added
  const wrap = useRef(null)
  const cv = useRef(null)
  const [cw, setCw] = useState(800)

  // the sheet DPI is the DPI of the designs (the first one that has it)
  const first = items.find((it) => it.dpi)
  const dpi = first ? Math.round(first.dpi) : Number(s.dpi)
  const sameDpi = (it) => !it.dpi || Math.abs(it.dpi - dpi) <= dpi * 0.005
  const used = items.filter((it) => sameDpi(it) && it.count > 0)
  const px = (cm) => Math.round((cm / 2.54) * dpi)
  const mm = (v) => Math.round((v / 25.4) * dpi)
  const opts = {
    width: px(s.gangWidth),
    max_len: s.gangMaxLen > 0 ? px(s.gangMaxLen) : null,
    gap: mm(s.gangGap),
    margin: mm(s.gangMargin),
    rotate: !!s.gangRotate,
  }
  const pieces = used.map((it) => ({ w: it.w, h: it.h, count: it.count }))
  // pins ({path, sheet, x, y, rot}) of the designs in use; Placed.pin = index in `sent`
  const sent = pins.map((pn, idx) => ({ idx, piece: used.findIndex((u) => u.path === pn.path), sheet: pn.sheet, x: pn.x, y: pn.y, rot: !!pn.rot })).filter((pn) => pn.piece >= 0)
  const pinArgs = sent.map(({ piece, sheet, x, y, rot }) => ({ piece, sheet, x, y, rot }))
  const key = JSON.stringify([pieces, opts, pinArgs])
  useEffect(() => {
    if (!pieces.length) {
      setSheets([])
      setErr('')
      return
    }
    let live = true
    invoke('gang_layout', { pieces, opts, pins: pinArgs })
      .then((sh) => live && (setSheets(sh), setErr('')))
      .catch((e) => live && (setSheets([]), setErr(String(e))))
    return () => (live = false)
  }, [key])


  // ------------------------------------------------------------ designs
  const itemsRef = useRef(items)
  itemsRef.current = [...items]
  // print files are added as they are; raw images are made print-ready first (job)
  const addPaths = async (paths, name) => {
    setAddErr([])
    const bad = []
    const known = itemsRef.current
    for (const p of paths) {
      const file = p.split(/[\\/]/).pop()
      // the same file again: one more copy (also for a raw image already processed)
      const same = known.find((x) => x.path === p || x.from === p)
      if (same) {
        setItems((xs) => xs.map((x) => (x.path === same.path ? { ...x, count: x.count + 1 } : x)))
        continue
      }
      setAdding(file)
      try {
        const info = await invoke('gang_add', { path: p, job })
        const th = unpack(await invoke('gang_thumb', { path: info.path }))
        const item = { ...info, name: name || info.name, count: 1, thumb: th?.canvas, src: th?.canvas.toDataURL() }
        known.push(item)
        setItems((xs) => (xs.some((x) => x.path === info.path) ? xs.map((x) => (x.path === info.path ? { ...x, count: x.count + 1 } : x)) : [...xs, item]))
      } catch (e) {
        bad.push(`${file}: ${e}`)
      }
    }
    setAdding(null)
    setAddErr(bad)
  }

  if (addRef) addRef.current = addPaths // files dropped on the window while this tab is open
  const pickFiles = async () => {
    const p = await open({ multiple: true, filters: IMAGES() })
    if (p) addPaths((Array.isArray(p) ? p : [p]).slice().sort((a, b) => a.localeCompare(b)))
  }
  const addNow = async () => {
    try {
      const r = await addCurrent()
      await addPaths([r.path], r.name)
    } catch (e) {
      flash(String(e.message || e))
    }
  }
  const setCount = (path, count) => setItems((xs) => xs.map((x) => (x.path === path ? { ...x, count } : x)))
  const remove = (path) => {
    setItems((xs) => xs.filter((x) => x.path !== path))
    setPins((ps) => ps.filter((pn) => pn.path !== path))
    invoke('gang_forget', { path }).catch(() => {})
  }

  // ------------------------------------------------------------ export
  const exportSheet = async () => {
    const w = fmt(s.gangWidth)
    const p = await save({ defaultPath: `gangsheet_${w}cm_${dpi}dpi.png`, filters: [{ name: 'PNG', extensions: ['png'] }] })
    if (!p) return
    try {
      setRunErr('')
      setRun({ sheet: 0, sheets: sheets.length, frac: 0 })
      await invoke('gang_export', { items: used.map((it) => ({ path: it.path, count: it.count })), opts, pins: pinArgs, dpi, path: p })
    } catch (e) {
      setRun(null)
      setRunErr(String(e))
      flash(String(e))
    }
  }

  // ------------------------------------------------------------ preview
  useEffect(() => {
    const el = wrap.current
    if (!el) return
    const ro = new ResizeObserver(() => setCw(el.clientWidth))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])
  const LABEL = 26
  // browsers draw nothing on a canvas taller than 32767 device pixels
  const maxH = 30000 / (window.devicePixelRatio || 1)
  const scale = sheets.length ? Math.min((cw - 48) / opts.width, maxH / sheets.reduce((a, sh) => a + sh.h + 1, 0)) : 1
  // where the sheets and designs are on the canvas (CSS px), for the mouse
  const geo = useRef({ sheets: [], boxes: [] })
  const [drag, setDrag] = useState(null) // {box, dx, dy, mx, my} while a design is dragged
  useEffect(() => {
    const c = cv.current
    if (!c) return
    const dpr = window.devicePixelRatio || 1
    const W = Math.max(1, cw - 24)
    const H = sheets.reduce((a, sh) => a + Math.ceil(sh.h * scale) + LABEL + 16, 16)
    c.width = Math.round(W * dpr)
    c.height = Math.round(H * dpr)
    c.style.width = W + 'px'
    c.style.height = H + 'px'
    const ctx = c.getContext('2d')
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
    ctx.clearRect(0, 0, W, H)
    let y = 8
    const x0 = 12
    const g = { sheets: [], boxes: [] }
    sheets.forEach((sh, i) => {
      ctx.fillStyle = cssVar('--stage-text', '#c8c8c8')
      ctx.font = '12px Segoe UI, sans-serif'
      ctx.fillText(L(`Lembar ${i + 1} · ${fmt((sh.w / dpi) * 2.54)} × ${fmt((sh.h / dpi) * 2.54)} cm · ${sh.placed.length} desain`, `Sheet ${i + 1} · ${fmt((sh.w / dpi) * 2.54)} × ${fmt((sh.h / dpi) * 2.54)} cm · ${sh.placed.length} designs`), x0, y + 16)
      y += LABEL
      const sw = sh.w * scale
      const shh = sh.h * scale
      g.sheets.push({ i, x: x0, y, w: sw, h: shh, sh })
      // film: light checkerboard = transparent
      ctx.fillStyle = '#f2f2f2'
      ctx.fillRect(x0, y, sw, shh)
      ctx.fillStyle = '#e1e1e1'
      const q = 10
      for (let yy = 0; yy < shh; yy += q) for (let xx = (Math.floor(yy / q) % 2) * q; xx < sw; xx += 2 * q) ctx.fillRect(x0 + xx, y + yy, Math.min(q, sw - xx), Math.min(q, shh - yy))
      ctx.strokeStyle = cssVar('--accent', '#b7ff72')
      ctx.lineWidth = 1
      ctx.strokeRect(x0 + 0.5, y + 0.5, sw - 1, shh - 1)
      if (opts.margin > 0) {
        ctx.setLineDash([4, 4])
        ctx.strokeStyle = 'rgba(0,0,0,.25)'
        ctx.strokeRect(x0 + opts.margin * scale, y + opts.margin * scale, (sh.w - 2 * opts.margin) * scale, (sh.h - 2 * opts.margin) * scale)
        ctx.setLineDash([])
      }
      for (const p of sh.placed) {
        const it = used[p.piece]
        if (!it) continue
        const [w, h] = p.rot ? [it.h, it.w] : [it.w, it.h]
        const X = x0 + p.x * scale
        const Y = y + p.y * scale
        if (it.thumb) {
          ctx.save()
          if (p.rot) {
            // 90° clockwise around the box
            ctx.translate(X + w * scale, Y)
            ctx.rotate(Math.PI / 2)
            ctx.drawImage(it.thumb, 0, 0, it.w * scale, it.h * scale)
          } else ctx.drawImage(it.thumb, X, Y, w * scale, h * scale)
          ctx.restore()
        }
        const moving = drag && drag.box.sheet === i && drag.box.p === p
        if (moving) {
          ctx.fillStyle = 'rgba(255,255,255,.6)'
          ctx.fillRect(X, Y, w * scale, h * scale)
        }
        ctx.strokeStyle = cssVar('--accent', '#b7ff72')
        ctx.lineWidth = p.pin != null ? 2 : 1
        ctx.strokeRect(X + 0.5, Y + 0.5, w * scale - 1, h * scale - 1)
        ctx.lineWidth = 1
        if (p.pin != null) {
          // pinned by hand: a small pin badge in the corner
          ctx.fillStyle = cssVar('--accent', '#b7ff72')
          ctx.beginPath()
          ctx.arc(X + 8, Y + 8, 5, 0, 2 * Math.PI)
          ctx.fill()
          ctx.strokeStyle = 'rgba(0,0,0,.6)'
          ctx.stroke()
        }
        g.boxes.push({ sheet: i, p, it, x: X, y: Y, w: w * scale, h: h * scale })
      }
      y += shh + 16
    })
    // the dragged design under the mouse
    if (drag) {
      const b = drag.box
      const X = drag.mx - drag.dx
      const Y = drag.my - drag.dy
      ctx.save()
      ctx.globalAlpha = 0.85
      if (b.it.thumb) {
        if (b.p.rot) {
          ctx.translate(X + b.w, Y)
          ctx.rotate(Math.PI / 2)
          ctx.drawImage(b.it.thumb, 0, 0, b.h, b.w)
        } else ctx.drawImage(b.it.thumb, X, Y, b.w, b.h)
      }
      ctx.restore()
      ctx.strokeStyle = cssVar('--accent', '#b7ff72')
      ctx.lineWidth = 2
      ctx.strokeRect(X + 0.5, Y + 0.5, b.w - 1, b.h - 1)
      ctx.lineWidth = 1
    }
    geo.current = g
    // getLang / the theme: the labels and outline colour are drawn into the canvas
  }, [sheets, cw, scale, items, drag, getLang(), document.documentElement.dataset.theme])

  // ------------------------------------------------------------ moving designs by hand
  const at = (e) => {
    const r = cv.current.getBoundingClientRect()
    return [e.clientX - r.left, e.clientY - r.top]
  }
  const boxAt = (mx, my) => geo.current.boxes.find((b) => mx >= b.x && mx < b.x + b.w && my >= b.y && my < b.y + b.h)
  const onDown = (e) => {
    if (e.button !== 0 || run) return
    const [mx, my] = at(e)
    const box = boxAt(mx, my)
    if (box) setDrag({ box, dx: mx - box.x, dy: my - box.y, mx, my })
  }
  // while dragging near the top / bottom edge the preview scrolls (to reach other sheets)
  const edgeScroll = useRef(null)
  const stopScroll = () => {
    clearInterval(edgeScroll.current)
    edgeScroll.current = null
  }
  useEffect(() => stopScroll, [])
  const onMove = (e) => {
    const [mx, my] = at(e)
    if (!drag) {
      cv.current.style.cursor = boxAt(mx, my) ? 'grab' : 'default'
      return
    }
    setDrag((d) => d && { ...d, mx, my })
    const el = wrap.current
    const r = el.getBoundingClientRect()
    const step = e.clientY > r.bottom - 48 ? 24 : e.clientY < r.top + 48 ? -24 : 0
    stopScroll()
    if (step) {
      edgeScroll.current = setInterval(() => {
        const before = el.scrollTop
        el.scrollTop += step
        // the canvas moved under the still mouse: keep the design under it
        const dy = el.scrollTop - before
        if (dy) setDrag((d) => d && { ...d, my: d.my + dy })
        else stopScroll()
      }, 30)
    }
  }
  const onUp = (e) => {
    stopScroll()
    if (!drag) return
    const [mx, my] = at(e)
    const { box, dx, dy } = drag
    setDrag(null)
    if (Math.abs(mx - dx - box.x) < 3 && Math.abs(my - dy - box.y) < 3) return // a click, not a move
    // the sheet under the design's top-left corner (or the nearest one)
    const cx = mx - dx
    const cy = my - dy
    const tg = geo.current.sheets.reduce((best, sg) => (Math.abs(cy - (sg.y + sg.h / 2)) - sg.h / 2 < Math.abs(cy - (best.y + best.h / 2)) - best.h / 2 ? sg : best), geo.current.sheets[0])
    if (!tg) return
    const [pw, ph] = box.p.rot ? [box.it.h, box.it.w] : [box.it.w, box.it.h]
    const maxY = (opts.max_len || tg.sh.h) - opts.margin - ph
    const x = Math.round(Math.min(Math.max((cx - tg.x) / scale, opts.margin), opts.width - opts.margin - pw))
    const y = Math.round(Math.min(Math.max((cy - tg.y) / scale, opts.margin), Math.max(opts.margin, maxY)))
    // not on top of another design placed by hand (the others make room)
    const own = box.p.pin != null ? sent[box.p.pin]?.idx : null
    const clash = tg.sh.placed.some((q) => {
      if (q.pin == null || (q.pin === box.p.pin && tg.i === box.sheet)) return false
      const qi = used[q.piece]
      const [qw, qh] = q.rot ? [qi.h, qi.w] : [qi.w, qi.h]
      return x < q.x + qw + opts.gap && q.x < x + pw + opts.gap && y < q.y + qh + opts.gap && q.y < y + ph + opts.gap
    })
    if (clash) {
      flash(L('Tidak bisa ditaruh di atas desain lain yang sudah dikunci.', 'Cannot go on top of another pinned design.'))
      return
    }
    const pin = { path: box.it.path, sheet: tg.i, x, y, rot: box.p.rot }
    setPins((ps) => (own != null ? ps.map((pn, k) => (k === own ? pin : pn)) : [...ps, pin]))
  }
  // double click: back to the automatic layout for that design
  const onDouble = (e) => {
    const box = boxAt(...at(e))
    if (box && box.p.pin != null) {
      const own = sent[box.p.pin]?.idx
      setPins((ps) => ps.filter((_, k) => k !== own))
    }
  }
  const pinned = sheets.reduce((a, sh) => a + sh.placed.filter((p) => p.pin != null).length, 0)

  const total = used.reduce((a, it) => a + it.count, 0)
  const area = used.reduce((a, it) => a + it.w * it.h * it.count, 0)
  const sheetArea = sheets.reduce((a, sh) => a + sh.w * sh.h, 0)
  const lenCm = sheets.reduce((a, sh) => a + (sh.h / dpi) * 2.54, 0)

  return (
    <div className="app">
      <main className="stage">
        <div className="toolbar">
          <b>Gang sheet</b>
          <span className="meta">
            {sheets.length
              ? L(`${sheets.length} lembar · lebar ${fmt(s.gangWidth)} cm · total panjang ${fmt(lenCm)} cm · ${total} desain · terisi ${Math.round((area / sheetArea) * 100)} % · ${dpi} DPI`, `${sheets.length} sheets · width ${fmt(s.gangWidth)} cm · total length ${fmt(lenCm)} cm · ${total} designs · ${Math.round((area / sheetArea) * 100)} % filled · ${dpi} DPI`)
              : L('Tambahkan desain di panel kanan.', 'Add designs in the right panel.')}
          </span>
        </div>
        <div className="viewer gangview" ref={wrap}>
          {sheets.length ? <canvas ref={cv} onMouseDown={onDown} onMouseMove={onMove} onMouseUp={onUp} onMouseLeave={() => (stopScroll(), setDrag(null))} onDoubleClick={onDouble} /> : (
            <div className="center">
              <p className="small">
                {L('Gang sheet menyusun banyak desain siap cetak dalam satu lembar film supaya hemat. Pakai file PNG hasil Export (atau Export banyak) dengan DPI yang sama; titik halftone disalin persis, tidak di-resize. Gambar mentah (JPG / PNG dengan latar) diproses otomatis dengan pengaturan tab Desain.',
                  'A gang sheet packs many print-ready designs on one film to save material. Use PNG files from Export (or Batch) at the same DPI; the halftone dots are copied exactly, never resized. Raw images (JPG / PNG with a background) are processed with the Design tab settings.')}
              </p>
            </div>
          )}
        </div>
        <div className="statusbar">
          {err || runErr ? <span className="err">{err || runErr}</span> : <span className="meta">{L('Susunan otomatis: desain terbesar dulu, rapat ke atas-kiri. Geser desain dengan mouse untuk menaruhnya sendiri (dikunci, yang lain menyesuaikan); klik dua kali = lepas.', 'Auto layout: largest designs first, packed to the top left. Drag a design with the mouse to place it yourself (pinned, the others make room); double click = unpin.')}</span>}
          {pinned > 0 && !run && (
            <button className="link" type="button" title={L('Kembali ke susunan otomatis', 'Back to the automatic layout')} onClick={() => setPins([])}>{L(`Lepas kunci (${pinned})`, `Unpin all (${pinned})`)}</button>
          )}
        </div>
      </main>
      <aside className="side">
        {tabs}
        <div className="scroll">
          <Card title="Film">
            <Row label={L('Lebar film', 'Film width')}>
              <NumField className="short" value={s.gangWidth} min={5} max={300} step={0.1} digits={1} onCommit={(v) => set({ gangWidth: v })} />
              <span className="small"> cm</span>
            </Row>
            <div className="sizes">
              {WIDTHS().map(([l, v]) => (
                <button key={l} type="button" className={Math.abs(s.gangWidth - v) < 0.01 ? 'chip on' : 'chip'} onClick={() => set({ gangWidth: v })}>{l}</button>
              ))}
            </div>
            <Row label={L('Panjang', 'Length')} hint={s.gangMaxLen > 0 ? L('Desain yang tidak muat pindah ke lembar berikutnya (file _1, _2, …).', 'Designs that do not fit go to the next sheet (files _1, _2, …).') : L('Satu lembar sepanjang yang dibutuhkan.', 'One sheet as long as needed.')}>
              <Seg value={s.gangMaxLen > 0 ? 'maks' : 'bebas'} onChange={(v) => set({ gangMaxLen: v === 'maks' ? 100 : 0 })} options={[['bebas', L('Tanpa batas', 'No limit')], ['maks', L('Maksimal', 'Max.')]]} />
            </Row>
            {s.gangMaxLen > 0 && (
              <Row label={L('Panjang maks', 'Max. length')}>
                <NumField className="short" value={s.gangMaxLen} min={5} max={5000} step={1} digits={0} onCommit={(v) => set({ gangMaxLen: v })} />
                <span className="small"> cm</span>
              </Row>
            )}
            <Row label={L('Jarak desain', 'Gap')}>
              <NumField className="short" value={s.gangGap} min={0} max={100} step={0.5} digits={1} onCommit={(v) => set({ gangGap: v })} />
              <span className="small"> mm</span>
            </Row>
            <Row label={L('Margin tepi', 'Edge margin')}>
              <NumField className="short" value={s.gangMargin} min={0} max={100} step={0.5} digits={1} onCommit={(v) => set({ gangMargin: v })} />
              <span className="small"> mm</span>
            </Row>
            <Row label={L('Boleh diputar 90°', 'Allow 90° turn')}>
              <input type="checkbox" checked={!!s.gangRotate} onChange={(e) => set({ gangRotate: e.target.checked })} />
            </Row>
          </Card>

          <Card title={`${L('Desain', 'Designs')} (${items.length})`}>
            <div className="btnrow">
              <button className="btn" type="button" disabled={!!adding || !!run} onClick={pickFiles} title={L('PNG hasil export dipakai apa adanya. Gambar mentah (JPG, PNG dengan latar) diproses dulu dengan pengaturan tab Desain (lebar cetak, kaos, halftone, tekstur). Bisa pilih banyak.', 'Exported PNGs are used as they are. Raw images (JPG, PNG with a background) are processed first with the Design tab settings (print width, shirt, halftone, texture). Pick many.')}>{L('Tambah gambar…', 'Add images…')}</button>
              <button className="btn" type="button" disabled={!!adding || !!run || !addCurrent} onClick={addNow} title={L('Export desain yang sedang dibuka (tab Desain) lalu tambahkan ke sini', 'Export the design open in the Design tab and add it here')}>
                {L('Desain sekarang', 'Current design')}
              </button>
            </div>
            {adding && <p className="small">{L(`Menambahkan ${adding}…`, `Adding ${adding}…`)}</p>}
            {addErr.length > 0 && (
              <p className="error">{L('Tidak bisa ditambahkan:', 'Could not add:')} {addErr.join(' · ')}</p>
            )}
            <ul className="ganglist">
              {items.map((it) => (
                <li key={it.path} className={sameDpi(it) ? '' : 'bad'}>
                  {it.src ? <img src={it.src} alt="" /> : <span className="noimg" />}
                  <div className="gname">
                    <span title={it.path}>{it.name}</span>
                    <span className="small">
                      {fmt((it.w / dpi) * 2.54)} × {fmt((it.h / dpi) * 2.54)} cm{it.dpi ? ` · ${Math.round(it.dpi)} DPI` : ''}
                      {it.from ? L(' · diproses', ' · processed') : ''}
                      {!sameDpi(it) ? L(' · DPI beda, tidak dipakai', ' · different DPI, not used') : ''}
                    </span>
                  </div>
                  <NumField className="tiny" value={it.count} min={0} max={500} step={1} digits={0} onCommit={(v) => setCount(it.path, v)} />
                  <button className="btn icon" type="button" title={L('Hapus dari gang sheet', 'Remove from the gang sheet')} disabled={!!run} onClick={() => remove(it.path)}>×</button>
                </li>
              ))}
            </ul>
            {items.length > 0 && <p className="small">{L('Angka = jumlah salinan. Ukuran mengikuti file (sudah sesuai DPI). "Diproses" = gambar mentah yang dibuat siap cetak dengan pengaturan tab Desain.', 'Number = copies. Size comes from the file (at its DPI). "Processed" = a raw image made print-ready with the Design tab settings.')}</p>}
          </Card>
        </div>
        <footer className="foot">
          {!run ? (
            <button className="btn primary wide big" type="button" disabled={!sheets.length || !!adding} onClick={exportSheet}>
              {sheets.length > 1 ? L(`Buat gang sheet (${sheets.length} file PNG)`, `Make gang sheet (${sheets.length} PNG files)`) : L('Buat gang sheet (PNG)', 'Make gang sheet (PNG)')}
            </button>
          ) : (
            <div>
              <p className="small">
                {L(`Menyimpan lembar ${run.sheet + 1} dari ${run.sheets}`, `Saving sheet ${run.sheet + 1} of ${run.sheets}`)} · {Math.round(run.frac * 100)} %
                <span className="pbar"><span style={{ width: `${((run.sheet + run.frac) / Math.max(1, run.sheets)) * 100}%` }} /></span>
              </p>
              <button className="btn wide" type="button" onClick={() => invoke('gang_cancel')}>{L('Hentikan', 'Stop')}</button>
            </div>
          )}
          {toast && <p className="toast">{toast}</p>}
        </footer>
      </aside>
    </div>
  )
}
