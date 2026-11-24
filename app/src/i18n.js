// Interface language: English (default) or Indonesian. Every text is written as
// L('Indonesia', 'English') where it is used, so nothing can get out of step. Saved
// values (shirt "hitam", shape "bulat", ...) are codes and never translated, so old
// projects and presets keep working. The choice is kept like the theme (localStorage
// + config) and sent to the backend for its messages.
import { invoke } from '@tauri-apps/api/core'

export const LANGS = [
  ['en', 'English'],
  ['id', 'Bahasa Indonesia'],
]

const saved = () => {
  try {
    const v = localStorage.getItem('sz-lang')
    return v === 'id' || v === 'en' ? v : 'en'
  } catch {
    return 'en'
  }
}

let lang = saved()

export const getLang = () => lang

/** the text in the current language */
export const L = (id, en) => (lang === 'id' ? id : en)

/** switch the language (the caller re-renders the app) */
export function setLang(l) {
  lang = l === 'id' ? 'id' : 'en'
  document.documentElement.lang = lang
  try {
    localStorage.setItem('sz-lang', lang)
  } catch {
    // no storage: the config file keeps the choice
  }
  invoke('set_lang', { lang }).catch(() => {})
  // canvases draw some text themselves: let them repaint
  window.dispatchEvent(new Event('sz-theme'))
}

setLang(lang)
