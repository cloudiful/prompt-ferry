import { render } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { THEME_MODE_STORAGE_KEY, readStorage, writeStorage } from '@/storage'
import { ThemeProvider, resolveThemeMode } from '@/lib/theme'

afterEach(() => {
  vi.unstubAllGlobals()
  vi.restoreAllMocks()
  document.documentElement.classList.remove('dark')
  document.documentElement.style.colorScheme = ''
})

describe('blocked storage', () => {
  it('returns null and never throws when the store throws', () => {
    vi.stubGlobal('localStorage', {
      getItem() {
        throw new Error('storage blocked')
      },
      setItem() {
        throw new Error('quota exceeded')
      },
    })

    expect(readStorage(THEME_MODE_STORAGE_KEY)).toBeNull()
    expect(() => writeStorage(THEME_MODE_STORAGE_KEY, 'light')).not.toThrow()
  })

  it('returns null and never throws when the store is absent', () => {
    vi.stubGlobal('localStorage', undefined)

    expect(readStorage(THEME_MODE_STORAGE_KEY)).toBeNull()
    expect(() => writeStorage(THEME_MODE_STORAGE_KEY, 'light')).not.toThrow()
  })

  it('applies the dark default on a blocked store without writing', () => {
    const writes: Array<[string, string]> = []
    vi.stubGlobal('localStorage', {
      getItem() {
        throw new Error('storage blocked')
      },
      setItem: (key: string, value: string) => writes.push([key, value]),
    })

    expect(resolveThemeMode(null)).toBe('dark')

    const view = render(
      <ThemeProvider>
        <div />
      </ThemeProvider>,
    )

    expect(document.documentElement.classList.contains('dark')).toBe(true)
    expect(document.documentElement.style.colorScheme).toBe('dark')
    expect(writes).toEqual([])

    view.unmount()
  })
})
