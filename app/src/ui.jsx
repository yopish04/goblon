// Small shared UI pieces (side panel cards, rows, fields, segmented buttons).
import React, { useEffect, useRef, useState } from 'react'
import { rgbaToCanvas } from './CanvasViewer.jsx'
import Icon from './icons.jsx'
import { getLang } from './i18n.js'

/** raw IPC image: [u32 w][u32 h][u32 gen][u32 rows-complete] then RGBA8 */
export function unpack(buf) {
  const b = buf instanceof ArrayBuffer ? buf : new Uint8Array(buf).buffer
  const hd = new Uint32Array(b, 0, 4)
  const [w, h, gen, rows] = [hd[0], hd[1], hd[2], hd[3]]
  if (!w || !h) return null
  return { canvas: rgbaToCanvas(new Uint8ClampedArray(b, 16, w * h * 4), w, h), w, h, gen, rows }
}

export function Card({ n, title, help, children }) {
  return (
    <section className="card">
      <h3>
        {n != null && <span className="num">{n}</span>}
        {title}
        <Help text={help} />
      </h3>
      {children}
    </section>
  )
}

/** small (?) icon; the explanation shows when the mouse rests on it */
export function Help({ text }) {
  if (!text) return null
  return (
    <span className="help" title={text} aria-label={text}>
      <Icon name="help" size={13} />
    </span>
  )
}

// a div, not a <label>: clicking the row text must not press the first button.
// `hint` is an explanation behind a (?) icon; `note` is a short line that stays visible
export function Row({ label, children, hint, note }) {
  return (
    <div className="row">
      <span className="lbl">
        {label}
        <Help text={hint} />
      </span>
      <span className="ctl">{children}</span>
      {note && <span className="hint">{note}</span>}
    </div>
  )
}

/** button with a drop-down list (closes on outside click / Esc) */
export function Menu({ icon, label, title, children, align = 'left' }) {
  const [open, setOpen] = useState(false)
  const ref = useRef(null)
  useEffect(() => {
    if (!open) return undefined
    const close = (e) => {
      if (e.type === 'keydown' ? e.key === 'Escape' : !ref.current?.contains(e.target)) setOpen(false)
    }
    window.addEventListener('mousedown', close)
    window.addEventListener('keydown', close)
    return () => {
      window.removeEventListener('mousedown', close)
      window.removeEventListener('keydown', close)
    }
  }, [open])
  return (
    <div className="menu" ref={ref}>
      <button type="button" className={open ? 'tb on' : 'tb'} title={open ? undefined : title} onClick={() => setOpen((o) => !o)}>
        {icon && <Icon name={icon} />}
        {label && <span>{label}</span>}
        <Icon name="chevron" size={12} />
      </button>
      {open && (
        <div className={`menupop ${align}`} onClick={(e) => e.target.closest('[data-close]') && setOpen(false)}>
          {children}
        </div>
      )}
    </div>
  )
}

/** number field that commits on Enter / leaving the field (not on every keystroke),
 *  clamped to [min, max]; empty commits null when `allowEmpty` */
export function NumField({ value, onCommit, min = -Infinity, max = Infinity, step = 'any', className, placeholder, allowEmpty = false, digits }) {
  const show = (v) => (v == null || v === '' ? '' : digits != null ? Number(v).toFixed(digits) : String(v))
  const [text, setText] = useState(show(value))
  const focused = useRef(false)
  useEffect(() => {
    if (!focused.current) setText(show(value))
  }, [value])
  const commit = () => {
    const t = text.trim().replace(',', '.')
    if (t === '') {
      if (allowEmpty) onCommit(null)
      else setText(show(value))
      return
    }
    const v = Number(t)
    if (!Number.isFinite(v)) {
      setText(show(value))
      return
    }
    const c = Math.min(max, Math.max(min, digits === 0 ? Math.round(v) : v))
    setText(show(c))
    if (c !== Number(value)) onCommit(c)
  }
  return (
    <input type="number" className={className} min={min} max={max} step={step} placeholder={placeholder} value={text}
      onFocus={() => (focused.current = true)}
      onChange={(e) => setText(e.target.value)}
      onBlur={() => { focused.current = false; commit() }}
      onKeyDown={(e) => { if (e.key === 'Enter') { e.currentTarget.blur() } else if (e.key === 'Escape') { setText(show(value)); e.currentTarget.blur() } }} />
  )
}

/** range input: shows the value while dragging, commits on release / keyboard (one render
 *  per change instead of one per pixel dragged). toSlider/fromSlider map value <-> position */
export function Slider({ value, min, max, step, onCommit, toSlider = (v) => v, fromSlider = (t) => t, format }) {
  const [t, setT] = useState(toSlider(value))
  const dragging = useRef(false)
  useEffect(() => {
    if (!dragging.current) setT(toSlider(value))
  }, [value])
  const commit = (tv) => {
    dragging.current = false
    const v = fromSlider(Number(tv))
    if (v !== value) onCommit(v)
  }
  return (
    <>
      <input type="range" min={min} max={max} step={step} value={t}
        onPointerDown={(e) => {
          dragging.current = true
          // releasing outside the slider must still commit
          e.currentTarget.setPointerCapture?.(e.pointerId)
        }}
        onChange={(e) => setT(Number(e.target.value))}
        onPointerUp={(e) => commit(e.currentTarget.value)}
        onKeyUp={(e) => commit(e.currentTarget.value)} />
      {format && <span className="small mono">{format(fromSlider(t))}</span>}
    </>
  )
}

/** short number: decimal comma in Indonesian, point in English */
export const fmt = (v, d = 1) => {
  const t = Number(v).toFixed(d).replace(/\.0+$/, '')
  return getLang() === 'id' ? t.replace('.', ',') : t
}

export function Seg({ value, options, onChange }) {
  return (
    <div className="seg">
      {options.map(([v, l]) => (
        <button key={v} className={v === value ? 'on' : ''} onClick={() => onChange(v)} type="button">
          {l}
        </button>
      ))}
    </div>
  )
}
