// Colour theme of the app window: "gelap", "terang" or "otomatis" (follows Windows).
// Only the interface changes; the preview stays neutral grey and the engine never
// sees the theme. The choice is kept in localStorage (read by index.html before the
// first paint, so the window does not flash) and in the app config.

export const THEMES = [
  ['otomatis', 'Ikuti Windows', 'Follow Windows'],
  ['terang', 'Terang', 'Light'],
  ['gelap', 'Gelap', 'Dark'],
]

const media = () => (typeof matchMedia === 'function' ? matchMedia('(prefers-color-scheme: dark)') : null)

/** "dark" or "light" for a choice */
export function resolveTheme(choice) {
  if (choice === 'gelap') return 'dark'
  if (choice === 'terang') return 'light'
  return media()?.matches === false ? 'light' : 'dark'
}

/** apply a choice to the document and remember it; returns "dark" | "light" */
export function applyTheme(choice) {
  const t = resolveTheme(choice)
  document.documentElement.dataset.theme = t
  try {
    localStorage.setItem('sz-theme', choice)
  } catch {
    // no storage: the config file still keeps the choice
  }
  window.dispatchEvent(new Event('sz-theme'))
  return t
}

/** call `f` when Windows switches between light and dark; returns the unsubscribe */
export function onSystemTheme(f) {
  const m = media()
  if (!m) return () => {}
  m.addEventListener('change', f)
  return () => m.removeEventListener('change', f)
}

/** current value of a CSS colour variable (for canvas drawing) */
export function cssVar(name, fallback) {
  const v = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return v || fallback
}
