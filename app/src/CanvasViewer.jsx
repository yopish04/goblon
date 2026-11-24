// Photoshop-like viewer: one <canvas>, zoom at the cursor (wheel / Ctrl+wheel / pinch),
// pan (drag, Space+drag, middle button), Ctrl+0 fit, Ctrl+1 100 %, Ctrl +/- steps,
// transparency grid behind the image, split view, crop tool.
//
// Pixels come from the backend in tiles of TILE print pixels per side at mip level L
// (1 tile pixel = 2^L x 2^L print pixels, box average). A small overview image covers
// the whole print while tiles load. At >= 100 % pixels are drawn without smoothing so
// the halftone dots are seen exactly as they will print.
import React, { useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react'
import { cssVar } from './theme.js'
import { L } from './i18n.js'

export const TILE = 512
const MAX_TILES = 160
// Photoshop zoom presets (fractions)
const STEPS = [0.01, 0.02, 0.03, 0.04, 0.05, 1 / 16, 1 / 12, 1 / 8, 1 / 6, 1 / 4, 1 / 3, 1 / 2, 2 / 3, 1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 24, 32]
const MIN_Z = 0.01
const MAX_Z = 32

/** RGBA bytes (straight alpha) -> canvas element */
export function rgbaToCanvas(bytes, w, h) {
  const c = document.createElement('canvas')
  c.width = w
  c.height = h
  const ctx = c.getContext('2d')
  const data = bytes instanceof Uint8ClampedArray ? bytes : new Uint8ClampedArray(bytes.buffer || bytes, bytes.byteOffset || 0, w * h * 4)
  ctx.putImageData(new ImageData(data, w, h), 0, 0)
  return c
}

function checker(ctx, x, y, w, h, dpr) {
  // transparency grid fixed in screen space (like Photoshop): 8 css px squares
  const s = 8 * dpr
  if (!checker.pat || checker.s !== s) {
    const p = document.createElement('canvas')
    p.width = p.height = s * 2
    const c = p.getContext('2d')
    c.fillStyle = '#ffffff'
    c.fillRect(0, 0, s * 2, s * 2)
    c.fillStyle = '#cccccc'
    c.fillRect(0, 0, s, s)
    c.fillRect(s, s, s, s)
    checker.pat = p
    checker.s = s
  }
  ctx.save()
  ctx.fillStyle = ctx.createPattern(checker.pat, 'repeat')
  ctx.fillRect(x, y, w, h)
  ctx.restore()
}

// blue diagonal stripes on a light blue tint (the text brush colour)
const hatch = {}
function hatchTile(dpr) {
  if (hatch.dpr !== dpr) {
    const s = Math.round(10 * dpr)
    const t = document.createElement('canvas')
    t.width = t.height = s
    const g = t.getContext('2d')
    g.fillStyle = 'rgba(70,150,255,.22)'
    g.fillRect(0, 0, s, s)
    g.strokeStyle = 'rgba(70,150,255,.9)'
    g.lineWidth = 2 * dpr
    g.beginPath()
    for (const d of [-s, 0, s]) {
      g.moveTo(d, s)
      g.lineTo(d + s, 0)
    }
    g.stroke()
    hatch.tile = t
    hatch.dpr = dpr
  }
  return hatch.tile
}

/**
 * props:
 *  w, h            print size in pixels (result) — source tiles use the same grid
 *  gen             changes whenever the backend result changes (drops the tile cache)
 *  overview        { result: canvas, source: canvas } covering the whole print (any size)
 *  fetchTile(kind, level, tx, ty) -> Promise<canvas|null>
 *  mode            'asli' | 'hasil' | 'split'
 *  backdrop        'grid' (transparent) | css colour
 *  dpi             for the cursor read-out
 *  cropping, crop, setCrop, onCropExit   Photoshop-like crop tool in image pixels:
 *                  the box can be moved, resized at its 8 handles (Shift = keep ratio)
 *                  or drawn anew; Enter / double click applies, Esc cancels
 *  onView(view)    reports {zoom, x, y} (cursor in print px) for the status bar
 *  brush           brush tool {r (radius in image px), mode: 'hapus'|'kembalikan'|'teks'|'bukan-teks', onStroke(pts)}:
 *                  left drag paints a stroke (shown red / green until the new render arrives)
 *  strokes         committed strokes in image px, drawn on top (Asli view)
 *  hatch           hatch the "Lindungi teks" areas (overview.text / 'text' tiles: alpha = protected)
 */
const CanvasViewer = React.forwardRef(function CanvasViewer(props, ref) {
  const { w, h, gen, overview, fetchTile, mode, backdrop, cropping, crop, setCrop, onCropExit, onView, brush } = props
  const wrap = useRef(null)
  const cv = useRef(null)
  const view = useRef({ z: 1, x: 0, y: 0, fit: true }) // screen = (print - 0) * z + (x, y)  [css px]
  const tiles = useRef(new Map()) // key -> {c, t}
  const pending = useRef(new Set())
  const raf = useRef(0)
  const hatchCv = useRef(null) // offscreen canvas of the text hatching
  const [split, setSplit] = useState(0.5)
  const splitRef = useRef(0.5)
  const [cursor, setCursor] = useState('grab')
  const space = useRef(false)
  const drag = useRef(null)
  const cropDraft = useRef(null)
  const hover = useRef(null) // cursor in image px (brush outline)
  const painted = useRef([]) // strokes sent to the app, shown until the image updates
  const tick = useRef(0)
  const live = useRef(props)
  live.current = props

  const size = () => {
    const el = wrap.current
    return el ? [el.clientWidth, el.clientHeight] : [1, 1]
  }

  const fitView = useCallback(() => {
    const [cw, ch] = size()
    const m = 24
    const z = Math.max(MIN_Z, Math.min(MAX_Z, Math.min((cw - 2 * m) / w, (ch - 2 * m) / h)))
    view.current = { z, x: (cw - w * z) / 2, y: (ch - h * z) / 2, fit: true }
    draw()
    report(null, null)
  }, [w, h])

  const zoomAt = (nz, sx, sy) => {
    const v = view.current
    nz = Math.max(MIN_Z, Math.min(MAX_Z, nz))
    const px = (sx - v.x) / v.z
    const py = (sy - v.y) / v.z
    view.current = { z: nz, x: sx - px * nz, y: sy - py * nz, fit: false }
    draw()
    report(sx, sy)
  }
  // zoom shown to the user = screen (device) pixels per print pixel, like Photoshop:
  // 100 % is exactly one print pixel per screen pixel, also with Windows scaling 125/150 %
  const dpr = () => window.devicePixelRatio || 1
  const step = (dir, sx, sy) => {
    const z = view.current.z * dpr()
    const next = dir > 0 ? STEPS.find((s) => s > z * 1.001) ?? MAX_Z : [...STEPS].reverse().find((s) => s < z / 1.001) ?? MIN_Z
    zoomAt(next / dpr(), sx, sy)
  }
  const center = () => {
    const [cw, ch] = size()
    return [cw / 2, ch / 2]
  }
  useImperativeHandle(ref, () => ({
    fit: fitView,
    actual: (sx, sy) => {
      const [cx, cy] = center()
      zoomAt(1 / dpr(), sx ?? cx, sy ?? cy)
    },
    zoomIn: () => step(1, ...center()),
    zoomOut: () => step(-1, ...center()),
    zoom: () => view.current.z * dpr(),
    applyCrop: () => applyCrop(),
    cancelCrop: () => cancelCrop(),
  }))

  // ------------------------------------------------------------ crop draft
  // the box being edited while the crop tool is on (image pixels, floats)
  useEffect(() => {
    if (cropping) {
      const c = live.current.crop
      cropDraft.current = c ? [...c] : [0, 0, w, h]
    } else cropDraft.current = null
    draw()
  }, [cropping, w, h])
  const applyCrop = () => {
    const d = cropDraft.current
    if (!d) return
    const c = [Math.round(d[0]), Math.round(d[1]), Math.round(d[2]), Math.round(d[3])]
    const full = c[0] <= 0 && c[1] <= 0 && c[2] >= w && c[3] >= h
    if (c[2] - c[0] >= 8 && c[3] - c[1] >= 8) setCrop(full ? null : c)
    onCropExit && onCropExit()
  }
  const cancelCrop = () => {
    onCropExit && onCropExit()
  }
  /** which part of the crop box is under the screen point: handle id, 'in' or null */
  const cropHit = (sx, sy) => {
    const d = cropDraft.current
    if (!d) return null
    const v = view.current
    const X0 = v.x + d[0] * v.z
    const Y0 = v.y + d[1] * v.z
    const X1 = v.x + d[2] * v.z
    const Y1 = v.y + d[3] * v.z
    const t = 7
    const nx = Math.abs(sx - X0) <= t ? 'w' : Math.abs(sx - X1) <= t ? 'e' : ''
    const ny = Math.abs(sy - Y0) <= t ? 'n' : Math.abs(sy - Y1) <= t ? 's' : ''
    const inX = sx > X0 - t && sx < X1 + t
    const inY = sy > Y0 - t && sy < Y1 + t
    if (ny && nx) return ny + nx
    if (nx && inY) return nx
    if (ny && inX) return ny
    if (sx > X0 && sx < X1 && sy > Y0 && sy < Y1) return 'in'
    return null
  }
  const CROP_CURSOR = { n: 'ns-resize', s: 'ns-resize', e: 'ew-resize', w: 'ew-resize', nw: 'nwse-resize', se: 'nwse-resize', ne: 'nesw-resize', sw: 'nesw-resize', in: 'move' }

  const report = (sx, sy) => {
    const v = view.current
    onView && onView({ zoom: v.z * dpr(), x: sx == null ? null : (sx - v.x) / v.z, y: sy == null ? null : (sy - v.y) / v.z })
  }

  // ------------------------------------------------------------ tiles
  const level = () => {
    const z = view.current.z * dpr()
    if (z >= 1) return 0
    return Math.max(0, Math.floor(Math.log2(1 / z)))
  }
  const want = (kind, lv, tx, ty) => {
    const { gen, fetchTile } = live.current
    const key = `${gen}/${kind}/${lv}/${tx}/${ty}`
    const hit = tiles.current.get(key)
    if (hit) {
      hit.t = ++tick.current
      return hit.c
    }
    if (!pending.current.has(key)) {
      pending.current.add(key)
      fetchTile(kind, lv, tx, ty)
        .then((c) => {
          if (c && key.startsWith(`${live.current.gen}/`)) {
            tiles.current.set(key, { c, t: ++tick.current })
            if (tiles.current.size > MAX_TILES) {
              const old = [...tiles.current.entries()].sort((a, b) => a[1].t - b[1].t)
              for (let i = 0; i < old.length - MAX_TILES; i++) tiles.current.delete(old[i][0])
            }
            draw()
          }
        })
        .catch(() => {})
        .finally(() => pending.current.delete(key))
    }
    return null
  }

  useEffect(() => {
    tiles.current.clear()
    pending.current.clear()
    painted.current = []
    draw()
  }, [gen, fetchTile])
  useEffect(() => {
    painted.current = []
    draw()
  }, [props.strokes])

  // the crop frame / split line use the theme accent
  useEffect(() => {
    const f = () => draw()
    window.addEventListener('sz-theme', f)
    return () => window.removeEventListener('sz-theme', f)
  }, [])

  // ------------------------------------------------------------ draw
  const draw = () => {
    cancelAnimationFrame(raf.current)
    raf.current = requestAnimationFrame(paint)
  }
  const paint = () => {
    const c = cv.current
    const el = wrap.current
    if (!c || !el) return
    const dpr = window.devicePixelRatio || 1
    const cw = el.clientWidth
    const ch = el.clientHeight
    if (c.width !== Math.round(cw * dpr) || c.height !== Math.round(ch * dpr)) {
      c.width = Math.round(cw * dpr)
      c.height = Math.round(ch * dpr)
      c.style.width = cw + 'px'
      c.style.height = ch + 'px'
    }
    const p = live.current
    const ctx = c.getContext('2d')
    ctx.setTransform(1, 0, 0, 1, 0, 0)
    ctx.fillStyle = cssVar('--stage', '#2a2a2a')
    ctx.fillRect(0, 0, c.width, c.height)
    const v = view.current
    const z = v.z * dpr
    // at >= 100 % the print pixel grid is snapped to screen pixels (crisp, like Photoshop)
    const snap = z >= 0.999
    const ox = snap ? Math.round(v.x * dpr) : v.x * dpr
    const oy = snap ? Math.round(v.y * dpr) : v.y * dpr
    // visible print rectangle
    const vx0 = Math.max(0, Math.floor(-ox / z))
    const vy0 = Math.max(0, Math.floor(-oy / z))
    const vx1 = Math.min(p.w, Math.ceil((c.width - ox) / z))
    const vy1 = Math.min(p.h, Math.ceil((c.height - oy) / z))
    if (vx1 <= vx0 || vy1 <= vy0) return
    const r = [ox + vx0 * z, oy + vy0 * z, (vx1 - vx0) * z, (vy1 - vy0) * z]
    if (p.backdrop === 'grid') checker(ctx, ...r, dpr)
    else {
      ctx.fillStyle = p.backdrop
      ctx.fillRect(...r)
    }
    const sx = ox + p.w * z * splitRef.current
    const layers = p.mode === 'split' ? [['source', 0, sx], ['result', sx, c.width]] : [[p.mode === 'asli' ? 'source' : 'result', 0, c.width]]
    const lv = level()
    const span = TILE << lv
    // one kind of image (overview, then the tiles the zoom needs) clipped to [cx0, cx1);
    // `under` = the backdrop under each tile (the tile replaces the overview there)
    const drawKind = (g, kind, cx0, cx1, under) => {
      g.save()
      g.beginPath()
      g.rect(cx0, 0, cx1 - cx0, c.height)
      g.clip()
      g.imageSmoothingEnabled = v.z * dpr < 0.999
      g.imageSmoothingQuality = 'high'
      const ov = p.overview && p.overview[kind]
      if (ov) g.drawImage(ov, ox, oy, p.w * z, p.h * z)
      // skip tiles when the overview is already as sharp as the screen
      const ovSharp = ov && ov.width >= p.w * v.z * dpr * 0.98
      if (!ovSharp) {
        for (let ty = Math.floor(vy0 / span); ty * span < vy1; ty++) {
          for (let tx = Math.floor(vx0 / span); tx * span < vx1; tx++) {
            const x0 = tx * span
            const y0 = ty * span
            if (ox + (x0 + span) * z < cx0 || ox + x0 * z > cx1) continue
            const t = want(kind, lv, tx, ty)
            if (t) {
              // edges snapped to device pixels so neighbouring tiles leave no seam
              const rx = Math.round(ox + x0 * z)
              const ry = Math.round(oy + y0 * z)
              const rw = Math.round(ox + (x0 + t.width * (1 << lv)) * z) - rx
              const rh = Math.round(oy + (y0 + t.height * (1 << lv)) * z) - ry
              g.save()
              g.beginPath()
              g.rect(rx, ry, rw, rh)
              g.clip()
              if (under) under(g, rx, ry, rw, rh)
              else g.clearRect(rx, ry, rw, rh)
              g.drawImage(t, rx, ry, rw, rh)
              g.restore()
            }
          }
        }
      }
      g.restore()
    }
    const fillBackdrop = (g, x, y, ww, hh) => {
      if (p.backdrop === 'grid') checker(g, x, y, ww, hh, dpr)
      else {
        g.fillStyle = p.backdrop
        g.fillRect(x, y, ww, hh)
      }
    }
    for (const [kind, cx0, cx1] of layers) drawKind(ctx, kind, cx0, cx1, fillBackdrop)
    // "Lindungi teks" areas: blue diagonal hatching over the print (display only); no
    // overview = no text areas at all (then the 'text' tiles are empty too)
    if (p.hatch && p.overview && p.overview.text) {
      const off = hatchCv.current || (hatchCv.current = document.createElement('canvas'))
      if (off.width !== c.width || off.height !== c.height) {
        off.width = c.width
        off.height = c.height
      }
      const g = off.getContext('2d')
      g.globalCompositeOperation = 'source-over'
      g.clearRect(0, 0, off.width, off.height)
      drawKind(g, 'text', 0, c.width, null)
      g.globalCompositeOperation = 'source-in'
      g.fillStyle = g.createPattern(hatchTile(dpr), 'repeat')
      g.fillRect(0, 0, off.width, off.height)
      g.globalCompositeOperation = 'source-over'
      ctx.drawImage(off, 0, 0)
    }
    // split divider
    if (p.mode === 'split') {
      ctx.fillStyle = cssVar('--accent', '#b7ff72')
      ctx.fillRect(sx - dpr, 0, 2 * dpr, c.height)
      ctx.fillStyle = 'rgba(0,0,0,.7)'
      ctx.fillRect(sx + 6 * dpr, 8 * dpr, (L('asli | hasil', 'original | result').length * 6 + 12) * dpr, 18 * dpr)
      ctx.fillStyle = '#ddd'
      ctx.font = `${11 * dpr}px Segoe UI, sans-serif`
      ctx.fillText(L('asli | hasil', 'original | result'), sx + 12 * dpr, 21 * dpr)
    }
    // eraser strokes: committed ones (Asli), sent ones until the render arrives, the one being drawn
    const strokeList = [...(p.strokes || []), ...painted.current, ...(drag.current?.kind === 'brush' ? [drag.current.stroke] : [])]
    if (strokeList.length) {
      ctx.save()
      ctx.lineCap = 'round'
      ctx.lineJoin = 'round'
      for (const st of strokeList) {
        const col = { hapus: 'rgba(255,70,70,.55)', kembalikan: 'rgba(80,220,140,.55)', teks: 'rgba(70,150,255,.5)', 'bukan-teks': 'rgba(255,170,40,.5)' }[st.mode] || 'rgba(255,70,70,.55)'
        ctx.strokeStyle = col
        ctx.fillStyle = col
        ctx.lineWidth = Math.max(1, 2 * st.r * z)
        ctx.beginPath()
        if (st.pts.length === 1) {
          ctx.arc(ox + st.pts[0][0] * z, oy + st.pts[0][1] * z, Math.max(0.5, st.r * z), 0, 2 * Math.PI)
          ctx.fill()
        } else {
          st.pts.forEach(([x, y], i) => (i ? ctx.lineTo(ox + x * z, oy + y * z) : ctx.moveTo(ox + x * z, oy + y * z)))
          ctx.stroke()
        }
      }
      ctx.restore()
    }
    // brush outline at the cursor
    if (p.brush && hover.current) {
      const R = Math.max(1.5 * dpr, p.brush.r * z)
      const [hx, hy] = [ox + hover.current[0] * z, oy + hover.current[1] * z]
      ctx.lineWidth = 1 * dpr
      ctx.strokeStyle = 'rgba(0,0,0,.8)'
      ctx.beginPath()
      ctx.arc(hx, hy, R + dpr, 0, 2 * Math.PI)
      ctx.stroke()
      ctx.strokeStyle = '#ffffff'
      ctx.beginPath()
      ctx.arc(hx, hy, R, 0, 2 * Math.PI)
      ctx.stroke()
    }
    // crop box (committed crop shown in Asli; the editable box while cropping)
    const cb = p.cropping ? cropDraft.current : p.crop
    if (cb && p.mode === 'asli') {
      const [a, b, cc, d] = cb
      const X = ox + a * z
      const Y = oy + b * z
      const W = (cc - a) * z
      const H = (d - b) * z
      ctx.fillStyle = p.cropping ? 'rgba(0,0,0,.55)' : 'rgba(0,0,0,.45)'
      ctx.fillRect(0, 0, c.width, Y)
      ctx.fillRect(0, Y + H, c.width, c.height - Y - H)
      ctx.fillRect(0, Y, X, H)
      ctx.fillRect(X + W, Y, c.width - X - W, H)
      if (p.cropping) {
        // rule of thirds, solid border, 8 handles, size label (like Photoshop)
        ctx.strokeStyle = 'rgba(255,255,255,.45)'
        ctx.lineWidth = 1 * dpr
        ctx.beginPath()
        for (const f of [1 / 3, 2 / 3]) {
          ctx.moveTo(X + W * f, Y)
          ctx.lineTo(X + W * f, Y + H)
          ctx.moveTo(X, Y + H * f)
          ctx.lineTo(X + W, Y + H * f)
        }
        ctx.stroke()
        ctx.strokeStyle = '#ffffff'
        ctx.lineWidth = 1.5 * dpr
        ctx.strokeRect(X, Y, W, H)
        const hs = 8 * dpr
        ctx.fillStyle = '#ffffff'
        ctx.strokeStyle = '#000000'
        ctx.lineWidth = 1 * dpr
        for (const [hx, hy] of [[X, Y], [X + W / 2, Y], [X + W, Y], [X + W, Y + H / 2], [X + W, Y + H], [X + W / 2, Y + H], [X, Y + H], [X, Y + H / 2]]) {
          ctx.fillRect(hx - hs / 2, hy - hs / 2, hs, hs)
          ctx.strokeRect(hx - hs / 2, hy - hs / 2, hs, hs)
        }
        const label = `${Math.round(cc - a)} × ${Math.round(d - b)} px · ${L('Enter = terapkan · Esc = batal', 'Enter = apply · Esc = cancel')}`
        ctx.font = `${12 * dpr}px Segoe UI, sans-serif`
        const tw = ctx.measureText(label).width + 12 * dpr
        const ly = Y > 30 * dpr ? Y - 26 * dpr : Y + 6 * dpr
        ctx.fillStyle = 'rgba(0,0,0,.75)'
        ctx.fillRect(X, ly, tw, 20 * dpr)
        ctx.fillStyle = '#ffffff'
        ctx.fillText(label, X + 6 * dpr, ly + 14 * dpr)
      } else {
        ctx.setLineDash([6 * dpr, 4 * dpr])
        ctx.strokeStyle = cssVar('--accent', '#b7ff72')
        ctx.lineWidth = 2 * dpr
        ctx.strokeRect(X, Y, W, H)
        ctx.setLineDash([])
      }
    }
  }

  // redraw on prop changes
  useEffect(() => {
    draw()
  })
  // first fit + refit when the print size changes
  const lastWH = useRef('')
  useEffect(() => {
    const k = `${w}x${h}`
    if (lastWH.current !== k) {
      const prev = lastWH.current
      lastWH.current = k
      if (!prev || view.current.fit) fitView()
      else {
        // keep the same physical zoom/centre ratio when the print size changes
        const [pw, ph] = prev.split('x').map(Number)
        const v = view.current
        const [cw, ch] = size()
        const fx = (cw / 2 - v.x) / (pw * v.z)
        const fy = (ch / 2 - v.y) / (ph * v.z)
        const nz = v.z * (pw / w)
        view.current = { z: nz, x: cw / 2 - fx * w * nz, y: ch / 2 - fy * h * nz, fit: false }
        draw()
        report(null, null)
      }
    }
  }, [w, h, fitView])
  useEffect(() => {
    const el = wrap.current
    if (!el) return
    const ro = new ResizeObserver(() => (view.current.fit ? fitView() : draw()))
    ro.observe(el)
    return () => ro.disconnect()
  }, [fitView])

  // ------------------------------------------------------------ input
  const local = (e) => {
    const r = wrap.current.getBoundingClientRect()
    return [e.clientX - r.left, e.clientY - r.top]
  }
  const toPrint = (sx, sy) => {
    const v = view.current
    return [(sx - v.x) / v.z, (sy - v.y) / v.z]
  }
  useEffect(() => {
    const el = wrap.current
    if (!el) return
    const wheel = (e) => {
      e.preventDefault()
      const [sx, sy] = local(e)
      // pixel deltas (trackpad / pinch = ctrlKey) zoom smoothly; mouse notches step
      const dy = e.deltaMode === 1 ? e.deltaY * 33 : e.deltaMode === 2 ? e.deltaY * 400 : e.deltaY
      if (e.shiftKey && !e.ctrlKey) {
        // Shift + wheel: pan horizontally
        view.current = { ...view.current, x: view.current.x - dy, fit: false }
        draw()
        return
      }
      const f = Math.exp(-dy * (e.ctrlKey ? 0.01 : 0.0025))
      zoomAt(view.current.z * f, sx, sy)
    }
    el.addEventListener('wheel', wheel, { passive: false })
    return () => el.removeEventListener('wheel', wheel)
  }, [])

  useEffect(() => {
    const kd = (e) => {
      const tag = (e.target && e.target.tagName) || ''
      if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') return
      if (e.code === 'Space' && !space.current) {
        space.current = true
        setCursor('grab')
        e.preventDefault()
      }
      if (live.current.cropping && (e.key === 'Enter' || e.key === 'Escape')) {
        e.preventDefault()
        if (e.key === 'Enter') applyCrop()
        else cancelCrop()
        return
      }
      const mod = e.ctrlKey || e.metaKey
      if (!mod) return
      const [cx, cy] = center()
      if (e.key === '0') { e.preventDefault(); fitView() }
      else if (e.key === '1') { e.preventDefault(); zoomAt(1 / dpr(), cx, cy) }
      else if (e.key === '=' || e.key === '+') { e.preventDefault(); step(1, cx, cy) }
      else if (e.key === '-' || e.key === '_') { e.preventDefault(); step(-1, cx, cy) }
    }
    const ku = (e) => {
      if (e.code === 'Space') {
        space.current = false
        setCursor('grab')
      }
    }
    window.addEventListener('keydown', kd)
    window.addEventListener('keyup', ku)
    return () => {
      window.removeEventListener('keydown', kd)
      window.removeEventListener('keyup', ku)
    }
  }, [fitView])

  useEffect(() => setCursor(brush ? 'crosshair' : 'grab'), [cropping, !!brush])

  const nearDivider = (sx) => {
    if (mode !== 'split') return false
    const v = view.current
    return Math.abs(v.x + w * v.z * splitRef.current - sx) < 6
  }

  const down = (e) => {
    const [sx, sy] = local(e)
    const br = live.current.brush
    const pan = e.button === 1 || space.current || (!cropping && !br && e.button === 0 && !nearDivider(sx))
    if (e.button === 0 && nearDivider(sx) && !space.current) {
      drag.current = { kind: 'split' }
    } else if (br && e.button === 0 && !space.current) {
      const q = toPrint(sx, sy)
      drag.current = { kind: 'brush', stroke: { mode: br.mode, r: br.r, pts: [q] } }
      draw()
    } else if (pan) {
      e.preventDefault()
      drag.current = { kind: 'pan', sx, sy, x: view.current.x, y: view.current.y }
      setCursor('grabbing')
    } else if (cropping && e.button === 0) {
      const [px, py] = toPrint(sx, sy)
      const hit = cropHit(sx, sy)
      if (hit) {
        // move ('in') or resize at a handle
        drag.current = { kind: 'cropedit', hit, p0: [px, py], d0: [...cropDraft.current] }
      } else {
        const q = [Math.max(0, Math.min(w, px)), Math.max(0, Math.min(h, py))]
        drag.current = { kind: 'crop', q }
        cropDraft.current = [q[0], q[1], q[0], q[1]]
      }
    } else return
    const mv = (ev) => {
      const [mx, my] = local(ev)
      const d = drag.current
      if (!d) return
      if (d.kind === 'pan') {
        view.current = { ...view.current, x: d.x + mx - d.sx, y: d.y + my - d.sy, fit: false }
      } else if (d.kind === 'brush') {
        const q = toPrint(mx, my)
        const pts = d.stroke.pts
        const last = pts[pts.length - 1]
        // a point every ~1/4 brush radius (and at least every 2 screen px)
        const step = Math.max(d.stroke.r / 4, 2 / view.current.z)
        if (Math.hypot(q[0] - last[0], q[1] - last[1]) >= step) pts.push(q)
        hover.current = q
      } else if (d.kind === 'split') {
        const v = view.current
        splitRef.current = Math.max(0, Math.min(1, (mx - v.x) / (w * v.z)))
        setSplit(splitRef.current)
      } else if (d.kind === 'crop') {
        const [px, py] = toPrint(mx, my)
        const q = [Math.max(0, Math.min(w, px)), Math.max(0, Math.min(h, py))]
        cropDraft.current = [Math.min(d.q[0], q[0]), Math.min(d.q[1], q[1]), Math.max(d.q[0], q[0]), Math.max(d.q[1], q[1])]
      } else if (d.kind === 'cropedit') {
        const [px, py] = toPrint(mx, my)
        const dx = px - d.p0[0]
        const dy = py - d.p0[1]
        let [a, b, c2, e2] = d.d0
        const min = 8
        if (d.hit === 'in') {
          const bw = c2 - a
          const bh = e2 - b
          a = Math.max(0, Math.min(w - bw, a + dx))
          b = Math.max(0, Math.min(h - bh, b + dy))
          c2 = a + bw
          e2 = b + bh
        } else {
          if (d.hit.includes('w')) a = Math.max(0, Math.min(c2 - min, a + dx))
          if (d.hit.includes('e')) c2 = Math.min(w, Math.max(a + min, c2 + dx))
          if (d.hit.includes('n')) b = Math.max(0, Math.min(e2 - min, b + dy))
          if (d.hit.includes('s')) e2 = Math.min(h, Math.max(b + min, e2 + dy))
          // Shift on a corner: keep the original aspect ratio
          if (ev.shiftKey && d.hit.length === 2) {
            const r = (d.d0[2] - d.d0[0]) / (d.d0[3] - d.d0[1])
            const nw = c2 - a
            const nh = e2 - b
            if (nw / nh > r) {
              const tw = nh * r
              if (d.hit.includes('w')) a = c2 - tw
              else c2 = a + tw
            } else {
              const th = nw / r
              if (d.hit.includes('n')) b = e2 - th
              else e2 = b + th
            }
          }
        }
        cropDraft.current = [a, b, c2, e2]
      }
      report(mx, my)
      draw()
    }
    const up = () => {
      window.removeEventListener('mousemove', mv)
      window.removeEventListener('mouseup', up)
      const d = drag.current
      drag.current = null
      if (d && d.kind === 'brush') {
        painted.current = [...painted.current, d.stroke]
        live.current.brush?.onStroke(d.stroke.pts)
        draw()
        return
      }
      if (d && d.kind === 'crop' && cropDraft.current) {
        // a click without a drag keeps the previous box
        const c = cropDraft.current
        if (c[2] - c[0] < 8 || c[3] - c[1] < 8) cropDraft.current = live.current.crop ? [...live.current.crop] : [0, 0, w, h]
        draw()
      }
      setCursor('grab')
    }
    window.addEventListener('mousemove', mv)
    window.addEventListener('mouseup', up)
  }
  const move = (e) => {
    const [sx, sy] = local(e)
    if (!drag.current) {
      if (nearDivider(sx)) setCursor('ew-resize')
      else if (space.current) setCursor('grab')
      else if (cropping) setCursor(CROP_CURSOR[cropHit(sx, sy)] || 'crosshair')
      else if (brush) setCursor('crosshair')
      else setCursor('grab')
    }
    if (brush) {
      hover.current = toPrint(sx, sy)
      draw()
    }
    report(sx, sy)
  }
  const dbl = (e) => {
    const [sx, sy] = local(e)
    if (cropping) {
      if (cropHit(sx, sy) === 'in') applyCrop()
      return
    }
    if (brush) return
    if (Math.abs(view.current.z * dpr() - 1) < 1e-6) fitView()
    else zoomAt(1 / dpr(), sx, sy)
  }

  return (
    <div className="viewer canvas" ref={wrap} style={{ cursor }} onMouseDown={down} onMouseMove={move} onDoubleClick={dbl}
      onMouseLeave={() => { hover.current = null; draw(); report(null, null) }} onContextMenu={(e) => e.preventDefault()}>
      <canvas ref={cv} />
    </div>
  )
})

export default CanvasViewer
