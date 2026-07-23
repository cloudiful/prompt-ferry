export const LOCALE_STORAGE_KEY = 'prompt-ferry:locale'
export const THEME_MODE_STORAGE_KEY = 'prompt-ferry:theme-mode'
export const CLIENT_KEY_SECRETS_STORAGE_KEY = 'prompt-ferry:client-key-secrets'
export const LOGIN_NAME_STORAGE_KEY = 'prompt-ferry:login-name'

export function readStorage(key: string): string | null {
  return localStorage.getItem(key)
}

export function writeStorage(key: string, value: string): void {
  localStorage.setItem(key, value)
}
