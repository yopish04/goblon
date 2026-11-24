// Updates: a few seconds after start the app asks the public update feed (signed
// latest.json on GitHub, see tauri.conf.json > plugins.updater) whether there is a newer
// version. If so a bar offers "Update sekarang": download with progress, signature
// check, install (Windows: passive installer), restart. Nothing installs without a click.
import React, { useCallback, useEffect, useRef, useState } from 'react'
import { check } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { getVersion } from '@tauri-apps/api/app'
import { L } from './i18n.js'

/** plain-language reason for an update error */
function why(e) {
  const m = String(e?.message || e)
  if (/signature/i.test(m)) return L('file update tidak sah (tanda tangan tidak cocok), tidak dipasang', 'invalid update file (signature mismatch), not installed')
  if (/network|connect|dns|timed? ?out|request|error sending|offline/i.test(m)) return L('tidak ada koneksi ke server update', 'no connection to the update server')
  if (/404|not found/i.test(m)) return L('belum ada versi yang diterbitkan', 'no release published yet')
  return m
}

export function useUpdater() {
  const [version, setVersion] = useState('')
  const [st, setSt] = useState({ phase: 'idle' }) // idle | checking | ada | unduh | pasang | terbaru | gagal
  const upd = useRef(null)
  const [hidden, setHidden] = useState(false)

  const run = useCallback(async (manual) => {
    setSt({ phase: 'checking', manual })
    try {
      const u = await check()
      if (u) {
        upd.current = u
        setHidden(false)
        setSt({ phase: 'ada', version: u.version, notes: u.body || '' })
      } else setSt({ phase: manual ? 'terbaru' : 'idle' })
    } catch (e) {
      // automatic check: stay quiet (offline, no release yet)
      setSt(manual ? { phase: 'gagal', msg: L(`Tidak bisa cek update: ${why(e)}`, `Cannot check for updates: ${why(e)}`) } : { phase: 'idle' })
    }
  }, [])

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {})
    const t = setTimeout(() => run(false), 3000)
    return () => clearTimeout(t)
  }, [run])

  const install = async () => {
    const u = upd.current
    if (!u) return
    setHidden(false) // show the progress (also when started from the version line)
    let got = 0
    let total = 0
    setSt((o) => ({ ...o, phase: 'unduh', got: 0, total: 0 }))
    try {
      await u.downloadAndInstall((ev) => {
        if (ev.event === 'Started') total = ev.data.contentLength || 0
        else if (ev.event === 'Progress') got += ev.data.chunkLength
        else if (ev.event === 'Finished') {
          setSt((o) => ({ ...o, phase: 'pasang' }))
          return
        }
        setSt((o) => ({ ...o, got, total }))
      })
      setSt((o) => ({ ...o, phase: 'pasang' }))
      await relaunch()
    } catch (e) {
      setSt((o) => ({ ...o, phase: 'gagal', msg: L(`Update gagal: ${why(e)}`, `Update failed: ${why(e)}`) }))
    }
  }

  return { version, st, hidden, hide: () => setHidden(true), check: () => run(true), install }
}

/** small floating bar shown when an update is available / downloading / failed */
export function UpdateBar({ u }) {
  const { st } = u
  if (u.hidden || !['ada', 'unduh', 'pasang', 'gagal'].includes(st.phase) || (st.phase === 'gagal' && !st.version)) return null
  const pct = st.total ? Math.round((st.got / st.total) * 100) : null
  return (
    <div className="updbar" role="status">
      {st.phase === 'ada' && (
        <>
          <span>
            <b>{L(`Versi ${st.version} tersedia`, `Version ${st.version} available`)}</b> ({L('sekarang', 'now')} {u.version}).
            {st.notes ? <span className="small"> {st.notes.split('\n')[0].slice(0, 140)}</span> : null}
          </span>
          <button className="btn primary" type="button" onClick={u.install}>{L('Update sekarang', 'Update now')}</button>
          <button className="btn" type="button" onClick={u.hide}>{L('Nanti', 'Later')}</button>
        </>
      )}
      {st.phase === 'unduh' && (
        <span>
          {L(`Mengunduh versi ${st.version}…`, `Downloading version ${st.version}…`)} {pct != null ? `${pct} %` : `${(st.got / 1e6).toFixed(1)} MB`}
          <span className="pbar"><span style={{ width: `${pct ?? 0}%` }} /></span>
        </span>
      )}
      {st.phase === 'pasang' && <span>{L(`Memasang versi ${st.version}… aplikasi akan terbuka lagi sendiri.`, `Installing version ${st.version}… the app will restart by itself.`)}</span>}
      {st.phase === 'gagal' && (
        <>
          <span className="err">{st.msg}</span>
          <button className="btn" type="button" onClick={u.install}>{L('Coba lagi', 'Retry')}</button>
          <button className="btn" type="button" onClick={u.hide}>{L('Tutup', 'Close')}</button>
        </>
      )}
    </div>
  )
}

/** "StudioZea v0.1.0 · Cek update" line with the result of a manual check */
export function VersionLine({ u }) {
  const { st } = u
  return (
    <p className="small">
      StudioZea {u.version ? `v${u.version}` : ''} ·{' '}
      <button className="link" type="button" disabled={st.phase === 'checking' || st.phase === 'unduh'} onClick={u.check}>
        {st.phase === 'checking' ? L('Mengecek…', 'Checking…') : L('Cek update', 'Check for updates')}
      </button>
      {st.phase === 'terbaru' && L(' · Sudah versi terbaru.', ' · Up to date.')}
      {st.phase === 'gagal' && !st.version && <span className="err"> · {st.msg}</span>}
      {st.phase === 'ada' && u.hidden && (
        <>
          {' · '}
          <button className="link" type="button" onClick={u.install}>{L('Update ke', 'Update to')} v{st.version}</button>
        </>
      )}
    </p>
  )
}
