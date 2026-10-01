export const LOCALE_STORAGE_KEY = 'prompt-ferry:locale'
export const THEME_MODE_STORAGE_KEY = 'prompt-ferry:theme-mode'
export const LOGIN_NAME_STORAGE_KEY = 'prompt-ferry:login-name'

// Firefox-family hardening (LibreWolf `dom.storage.enabled=false`, partitioned
// or blocked origins) can make any localStorage access throw. Callers read as
// "nothing persisted" and write as "cannot persist", so a throw must never
// escape into module initialization or a click handler.
export function readStorage(key: string): string | null {
  try {
    return localStorage.getItem(key)
  } catch {
    return null
  }
}

export function writeStorage(key: string, value: string): void {
  try {
    localStorage.setItem(key, value)
  } catch {
    // Persistence is best effort; the in-memory value stays authoritative.
  }
}
