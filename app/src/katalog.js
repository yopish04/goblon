// Ink catalogues for sablon: the standard Pantone screen-printing colours (built in) and
// brand colour charts imported from an .xlsx file (kept in the local settings only).
// An ink: { h: '#RRGGBB', n: name, c: code (factory / Pantone), p: nearest Pantone, s: series }

/** the basic Pantone colours most Indonesian screen printers stock (hex = approximate) */
const STD_ROWS = [
  ['Putih', 'White', '#FFFFFF'], ['Hitam', 'Black 6 C', '#101820'],
  ['Kuning Lemon', 'Yellow C', '#FEDD00'], ['Kuning Medium', '116 C', '#FFCD00'], ['Kuning Emas', '123 C', '#FFC72C'],
  ['Orange', '151 C', '#FF8200'], ['Orange Bata', '1665 C', '#DC4405'],
  ['Merah Terang', '485 C', '#DA291C'], ['Merah Cabe', '186 C', '#C8102E'], ['Merah Darah', '200 C', '#BA0C2F'],
  ['Maroon', '202 C', '#862633'], ['Maroon Tua', '7421 C', '#651D32'], ['Pink', '219 C', '#DA1884'],
  ['Pink Muda', '189 C', '#F8A3BC'], ['Magenta', 'Rhodamine Red C', '#E10098'],
  ['Ungu', '268 C', '#582C83'], ['Ungu Tua', '2685 C', '#330072'],
  ['Biru Benhur (Royal)', '286 C', '#0033A0'], ['Biru Reflex', 'Reflex Blue C', '#001489'], ['Biru Sedang', '2935 C', '#0057B8'],
  ['Biru Langit', '299 C', '#00A3E0'], ['Biru Muda (Baby Blue)', '284 C', '#6CACE4'], ['Biru Dongker (Navy)', '289 C', '#0C2340'],
  ['Tosca', '3125 C', '#00AEC7'], ['Teal', '320 C', '#009CA6'],
  ['Hijau Daun', '354 C', '#00B140'], ['Hijau Bendera', '347 C', '#009A44'], ['Hijau Botol', '349 C', '#046A38'],
  ['Hijau Lumut (Forest)', '343 C', '#115740'], ['Hijau Army', '574 C', '#4E5B31'], ['Hijau Pupus (Lime)', '375 C', '#97D700'],
  ['Coklat Susu', '469 C', '#693F23'], ['Coklat Tua', '4625 C', '#4F2C1D'], ['Krem', '468 C', '#DDCBA4'],
  ['Abu Muda', 'Cool Gray 3 C', '#C8C9C7'], ['Abu Sedang', 'Cool Gray 7 C', '#97999B'], ['Abu Tua', 'Cool Gray 11 C', '#53565A'],
  ['Kuning Stabilo (Neon)', '803 C', '#FFE900'], ['Orange Stabilo (Neon)', '804 C', '#FFAA4D'], ['Merah Coral (Neon)', '805 C', '#FF7276'],
  ['Pink Stabilo (Neon)', '806 C', '#FF3EB5'], ['Magenta Neon', '807 C', '#EA27C2'], ['Hijau Stabilo (Neon)', '802 C', '#44D62C'],
  ['Biru Neon', '801 C', '#009ACE'], ['Emas (Gold)', '871 C', '#84754E'], ['Perak (Silver)', '877 C', '#8A8D8F'],
]
export const STD_ID = 'std'
// colour families of the standard rows (the picker groups them)
const STD_GROUPS = [[2, 'Putih & Hitam'], [7, 'Kuning & Orange'], [15, 'Merah & Pink'], [17, 'Ungu'], [25, 'Biru'], [31, 'Hijau'], [34, 'Coklat & Krem'], [37, 'Abu-abu'], [44, 'Neon'], [46, 'Metalik']]
const stdGroup = (k) => STD_GROUPS.find(([end]) => k < end)[1]
export const STD = { id: STD_ID, name: 'Pantone sablon standar', en: 'Standard Pantone screen-print colours', inks: STD_ROWS.map(([n, c, h], k) => ({ h, n, c, s: stdGroup(k) })) }

/** 'metalik' | 'neon' | null: special inks are not chosen automatically (the preview
 * cannot show their shine / glow), only by hand */
export function inkKind(ink) {
  const t = `${ink.n || ''} ${ink.c || ''} ${ink.p || ''} ${ink.s || ''}`
  if (/metal|mirror|glitter|shimmer|\b8[7-8]\d\s*C\b/i.test(t)) return 'metalik'
  if (/neon|fluo|\bflo\b|stabilo|electric|glow|\b80[1-7]\s*C\b/i.test(t)) return 'neon'
  return null
}

export const hexToRgb8 = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16))
const sameRgb = (a, b) => a[0] === b[0] && a[1] === b[1] && a[2] === b[2]

export const ALL_ID = '*'
/** every catalogue as one (ink set "all catalogues"): the brand stays in each ink's name */
export function allCatalog(cats) {
  return { id: ALL_ID, name: 'Semua katalog', en: 'All catalogues', inks: cats.flatMap((c) => c.inks.map((i) => ({ ...i, from: c }))) }
}

/** the colours the engine may choose from (no neon / metallic, no duplicates) */
export function catalogPalette(cat) {
  const out = []
  for (const i of cat?.inks || []) {
    if (inkKind(i)) continue
    const c = hexToRgb8(i.h)
    if (!out.some((o) => sameRgb(o, c))) out.push(c)
  }
  return out
}

/** the ink's name: "P-OP 4430 Red", "Merah Terang"; with the brand for imported ones */
export function inkTitle(ink, cat) {
  const base = ink.c && cat?.id !== STD_ID && !String(ink.n).includes(ink.c) ? `${ink.c} ${ink.n}` : ink.n
  return cat && cat.id !== STD_ID ? `${cat.name} ${base}` : base
}
/** the Pantone reference shown next to it: "Pantone 485 C" */
export function inkRef(ink, cat) {
  if (cat?.id === STD_ID) return ink.c && ink.c !== 'White' ? `Pantone ${ink.c}` : ''
  return ink.p && !/^(white|putih|black|hitam|[-—–\s]*)$/i.test(ink.p) ? `~Pantone ${ink.p}` : ''
}
/** a stored ink line split again: { title, ref } ("ANT Ink P-OP 4430 Red", "~Pantone 485 C") */
export function splitLine(line) {
  const m = /^(.*) \((~?Pantone [^)]*)\)$/.exec(line || '')
  return m ? { title: m[1], ref: m[2] } : { title: line || '', ref: '' }
}
/** one line for films and the print order (ASCII only on the film itself) */
export const inkLine = (ink, cat) => {
  const r = inkRef(ink, cat)
  return r ? `${inkTitle(ink, cat)} (${r})` : inkTitle(ink, cat)
}

/** the catalogue ink with exactly this colour: the active catalogue first, then the
 * standard one, then the others. Returns { ink, cat } or null */
export function findInk(rgb, cats, activeId) {
  const order = [...cats.filter((c) => c.id === activeId), ...cats.filter((c) => c.id !== activeId)]
  for (const cat of order) {
    const ink = cat.inks.find((i) => sameRgb(hexToRgb8(i.h), rgb))
    if (ink) return { ink, cat }
  }
  return null
}

// ------------------------------------------------------------------ saved catalogues
const okStr = (v, n) => v === undefined || (typeof v === 'string' && v.length <= n)
const okInk = (i) => i && typeof i.h === 'string' && /^#[0-9A-Fa-f]{6}$/.test(i.h) && typeof i.n === 'string' && i.n.length <= 120 && okStr(i.c, 60) && okStr(i.p, 60) && okStr(i.s, 120)
/** imported catalogues from the settings file (damaged entries are left out) */
export function okCatalogs(v) {
  if (!Array.isArray(v)) return []
  return v.filter((c) => c && typeof c.id === 'string' && c.id !== STD_ID && c.id.length <= 80 && typeof c.name === 'string' && c.name.length <= 80 &&
    Array.isArray(c.inks) && c.inks.length > 0 && c.inks.length <= 2000 && c.inks.every(okInk)).slice(0, 100)
}

// ------------------------------------------------------------------ .xlsx import
/** the files of a zip archive (stored or deflated): name -> Uint8Array */
async function unzip(buf) {
  const b = new Uint8Array(buf)
  const v = new DataView(buf)
  let e = -1
  for (let i = b.length - 22; i >= Math.max(0, b.length - 65557); i--) {
    if (v.getUint32(i, true) === 0x06054b50) { e = i; break }
  }
  if (e < 0) throw new Error('bukan file .xlsx')
  const n = v.getUint16(e + 10, true)
  let p = v.getUint32(e + 16, true)
  const out = {}
  const dec = new TextDecoder()
  for (let k = 0; k < n; k++) {
    if (v.getUint32(p, true) !== 0x02014b50) throw new Error('file .xlsx rusak')
    const method = v.getUint16(p + 10, true)
    const size = v.getUint32(p + 20, true)
    const nl = v.getUint16(p + 28, true)
    const xl = v.getUint16(p + 30, true)
    const cl = v.getUint16(p + 32, true)
    const off = v.getUint32(p + 42, true)
    const name = dec.decode(b.subarray(p + 46, p + 46 + nl))
    p += 46 + nl + xl + cl
    if (!/^(xl\/(workbook\.xml|sharedStrings\.xml|_rels\/workbook\.xml\.rels|worksheets\/[^/]+\.xml))$/.test(name)) continue
    const start = off + 30 + v.getUint16(off + 26, true) + v.getUint16(off + 28, true)
    const data = b.subarray(start, start + size)
    if (method === 0) out[name] = data
    else if (method === 8) out[name] = new Uint8Array(await new Response(new Blob([data]).stream().pipeThrough(new DecompressionStream('deflate-raw'))).arrayBuffer())
  }
  return out
}

const tags = (el, n) => Array.from(el.getElementsByTagName(n))
const xml = (bytes) => new DOMParser().parseFromString(new TextDecoder().decode(bytes), 'application/xml')
const colNo = (ref) => {
  let c = 0
  for (const ch of ref.replace(/[0-9]/g, '')) c = c * 26 + ch.charCodeAt(0) - 64
  return c - 1
}

/** the sheets of an .xlsx file: [{ name, rows: string[][] }] */
export async function readXlsx(buf) {
  const files = await unzip(buf)
  if (!files['xl/workbook.xml']) throw new Error('bukan file .xlsx')
  const shared = files['xl/sharedStrings.xml']
    ? tags(xml(files['xl/sharedStrings.xml']), 'si').map((si) => tags(si, 't').map((t) => t.textContent).join(''))
    : []
  const rels = {}
  if (files['xl/_rels/workbook.xml.rels']) {
    for (const r of tags(xml(files['xl/_rels/workbook.xml.rels']), 'Relationship')) rels[r.getAttribute('Id')] = r.getAttribute('Target')
  }
  const sheets = []
  for (const sh of tags(xml(files['xl/workbook.xml']), 'sheet')) {
    const target = (rels[sh.getAttribute('r:id')] || '').replace(/^\/?(xl\/)?/, '')
    const f = files[`xl/${target}`]
    if (!f) continue
    const rows = []
    for (const row of tags(xml(f), 'row')) {
      const r = []
      for (const c of tags(row, 'c')) {
        const t = c.getAttribute('t')
        const vEl = c.getElementsByTagName('v')[0]
        const val = t === 's' ? shared[Number(vEl?.textContent)] ?? '' : t === 'inlineStr' ? tags(c, 't').map((x) => x.textContent).join('') : vEl?.textContent ?? ''
        r[colNo(c.getAttribute('r') || 'A')] = String(val).trim()
      }
      rows.push(r)
    }
    sheets.push({ name: sh.getAttribute('name') || 'Sheet', rows })
  }
  return sheets
}

/** catalogues from the sheets: a sheet with a Hex column; with a brand (Merek) column
 * one catalogue per brand, else one per sheet. Sheets equal to the standard catalogue
 * and rows without a hex colour are left out */
export function catalogsFromSheets(sheets) {
  const std = new Set(STD.inks.map((i) => i.h.toUpperCase()))
  const out = new Map()
  for (const { name, rows } of sheets) {
    const hi = rows.slice(0, 6).findIndex((r) => (r || []).some((c) => /^hex/i.test(c || '')))
    if (hi < 0) continue
    const head = rows[hi].map((c) => (c || '').toLowerCase())
    const col = (re, not) => head.findIndex((c) => re.test(c) && !(not && not.test(c)))
    const hex = col(/^hex/)
    const brand = col(/merek|brand|merk/)
    const series = col(/seri|series|kategori/)
    const code = col(/kode pabrik|factory|^kode$|^code$|^no\.? ?warna/)
    const nm = col(/nama|name/)
    const pan = col(/pantone/)
    for (const r of rows.slice(hi + 1)) {
      const m = /#?([0-9a-f]{6})\b/i.exec(r?.[hex] || '')
      if (!m) continue
      const n = (r[nm] || r[code] || '').slice(0, 120)
      if (!n) continue
      const b = (brand >= 0 && r[brand]) || name
      const id = `x:${b}`.slice(0, 80)
      // a brand column: one catalogue per brand (kept even when its colours equal
      // standard Pantone ones); without one: one per sheet
      if (!out.has(id)) out.set(id, { id, name: b.slice(0, 80), inks: [], sheet: brand < 0 })
      const ink = { h: `#${m[1].toUpperCase()}`, n }
      if (code >= 0 && r[code] && r[code] !== n) ink.c = r[code].slice(0, 60)
      if (pan >= 0 && r[pan]) {
        if (brand < 0 && code < 0) ink.c = r[pan].slice(0, 60)
        else ink.p = r[pan].slice(0, 60)
      }
      if (series >= 0 && r[series] && r[series] !== b) ink.s = r[series].slice(0, 120)
      const cat = out.get(id)
      if (cat.inks.length < 2000) cat.inks.push(ink)
    }
  }
  // a plain sheet equal to the built-in standard list (the user's own copy of it) is left out
  return [...out.values()].filter((c) => !(c.sheet && c.inks.every((i) => std.has(i.h)))).map(({ sheet, ...c }) => c).slice(0, 100)
}
