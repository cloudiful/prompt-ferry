import { act, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { useLocale } from '@/lib/i18n'
import { LocaleProvider } from '@/lib/i18n'
import { ThemeProvider, resolveThemeMode, useThemeMode } from '@/lib/theme'
import {
  getDisplayTimeZoneSnapshot,
  setDisplayTimeZone,
  useDisplayTimeZone,
} from '@/lib/display-timezone'
import { formatDateOnly, formatDateTime } from '@/format/datetime-core'
import {
  chartThemeFor,
  DARK_CHART_THEME,
  LIGHT_CHART_THEME,
} from '@/theme/chart-tokens'

const BOUNDARY_INSTANT = '2026-03-04T23:30:00Z'

function PreferencesProbe() {
  const { locale, setLocale } = useLocale()
  const { mode, setMode } = useThemeMode()

  return (
    <div>
      <output data-testid="preferences">{`${locale} ${mode}`}</output>
      <button onClick={() => setLocale('zh-CN')} type="button">
        中文
      </button>
      <button onClick={() => setMode('light')} type="button">
        Light
      </button>
      <button onClick={() => setMode('dark')} type="button">
        Dark
      </button>
    </div>
  )
}

function RenderedPreferencesProbe() {
  const { locale, setLocale } = useLocale()
  const { mode, setMode } = useThemeMode()
  const { effective } = useDisplayTimeZone()

  return (
    <div>
      <output data-testid="rendered-date">
        {formatDateOnly(BOUNDARY_INSTANT, locale, undefined, effective)}
      </output>
      <output data-testid="rendered-time">
        {formatDateTime(
          BOUNDARY_INSTANT,
          locale,
          { hour: '2-digit', minute: '2-digit', hour12: false },
          effective,
        )}
      </output>
      <output data-testid="rendered-locale-date">
        {formatDateTime(
          BOUNDARY_INSTANT,
          locale,
          { year: 'numeric', month: '2-digit', day: '2-digit' },
          effective,
        )}
      </output>
      <output data-testid="rendered-theme">{chartThemeFor(mode).bg}</output>
      <button onClick={() => setLocale('zh-CN')} type="button">
        中文
      </button>
      <button onClick={() => setMode('light')} type="button">
        Light
      </button>
    </div>
  )
}

function renderProbe() {
  return render(
    <LocaleProvider>
      <ThemeProvider>
        <PreferencesProbe />
      </ThemeProvider>
    </LocaleProvider>,
  )
}

afterEach(() => {
  setDisplayTimeZone('auto')
  localStorage.clear()
  document.documentElement.classList.remove('dark')
  document.documentElement.lang = ''
  document.documentElement.style.colorScheme = ''
})

describe('React locale and theme providers', () => {
  it('restores and persists the existing locale and theme settings', () => {
    localStorage.setItem('prompt-ferry:locale', 'en-US')
    localStorage.setItem('prompt-ferry:theme-mode', 'dark')

    renderProbe()

    expect(screen.getByTestId('preferences').textContent).toBe('en-US dark')
    expect(document.documentElement.lang).toBe('en-US')
    expect(document.documentElement.classList.contains('dark')).toBe(true)

    fireEvent.click(screen.getByRole('button', { name: 'Light' }))
    fireEvent.click(screen.getByRole('button', { name: '中文' }))

    expect(localStorage.getItem('prompt-ferry:locale')).toBe('zh-CN')
    expect(document.documentElement.lang).toBe('zh-CN')
    expect(localStorage.getItem('prompt-ferry:theme-mode')).toBe('light')
    expect(document.documentElement.style.colorScheme).toBe('light')
    expect(document.documentElement.classList.contains('dark')).toBe(false)
  })

  it('defaults to dark and treats every other stored value as dark', () => {
    expect(resolveThemeMode(null)).toBe('dark')
    expect(resolveThemeMode('')).toBe('dark')
    expect(resolveThemeMode('system')).toBe('dark')
    expect(resolveThemeMode('dark')).toBe('dark')
    expect(resolveThemeMode('light')).toBe('light')

    renderProbe()
    expect(document.documentElement.classList.contains('dark')).toBe(true)
    expect(document.documentElement.style.colorScheme).toBe('dark')
  })

  it('keeps the document class and colour scheme in step through repeated switches', () => {
    renderProbe()

    for (const [button, mode] of [
      ['Light', 'light'],
      ['Dark', 'dark'],
      ['Light', 'light'],
      ['Dark', 'dark'],
    ] as const) {
      fireEvent.click(screen.getByRole('button', { name: button }))
      expect(document.documentElement.classList.contains('dark')).toBe(
        mode === 'dark',
      )
      expect(document.documentElement.style.colorScheme).toBe(mode)
      expect(localStorage.getItem('prompt-ferry:theme-mode')).toBe(mode)
      expect(screen.getByTestId('preferences').textContent).toContain(mode)
    }
  })

  it('repaints in place when the stored value changes between renders', () => {
    localStorage.setItem('prompt-ferry:theme-mode', 'light')
    const view = renderProbe()
    expect(document.documentElement.classList.contains('dark')).toBe(false)

    localStorage.setItem('prompt-ferry:theme-mode', 'dark')
    view.unmount()
    renderProbe()

    expect(document.documentElement.classList.contains('dark')).toBe(true)
    expect(document.documentElement.style.colorScheme).toBe('dark')
  })

  it.each([
    ['Light', 'light'],
    ['Dark', 'dark'],
  ] as const)(
    'keeps %s theme ownership through a locale rerender and remount',
    (button, mode) => {
      localStorage.setItem('prompt-ferry:locale', 'en-US')
      const view = renderProbe()

      fireEvent.click(screen.getByRole('button', { name: button }))
      expect(screen.getByTestId('preferences').textContent).toBe(
        `en-US ${mode}`,
      )
      expect(document.documentElement.classList.contains('dark')).toBe(
        mode === 'dark',
      )
      expect(document.documentElement.style.colorScheme).toBe(mode)
      expect(localStorage.getItem('prompt-ferry:theme-mode')).toBe(mode)

      fireEvent.click(screen.getByRole('button', { name: '中文' }))

      expect(screen.getByTestId('preferences').textContent).toBe(
        `zh-CN ${mode}`,
      )
      expect(document.documentElement.lang).toBe('zh-CN')
      expect(document.documentElement.classList.contains('dark')).toBe(
        mode === 'dark',
      )
      expect(document.documentElement.style.colorScheme).toBe(mode)
      expect(localStorage.getItem('prompt-ferry:theme-mode')).toBe(mode)

      view.unmount()
      renderProbe()

      expect(screen.getByTestId('preferences').textContent).toBe(
        `zh-CN ${mode}`,
      )
      expect(document.documentElement.classList.contains('dark')).toBe(
        mode === 'dark',
      )
      expect(document.documentElement.style.colorScheme).toBe(mode)
      expect(localStorage.getItem('prompt-ferry:theme-mode')).toBe(mode)
    },
  )

  it('renders the same instant under the effective zone, locale and theme', () => {
    const previousZone = getDisplayTimeZoneSnapshot().selected
    localStorage.setItem('prompt-ferry:locale', 'en-US')
    act(() => setDisplayTimeZone('UTC'))
    const view = render(
      <LocaleProvider>
        <ThemeProvider>
          <RenderedPreferencesProbe />
        </ThemeProvider>
      </LocaleProvider>,
    )

    try {
      expect(screen.getByTestId('rendered-date').textContent).toBe('2026-03-04')
      expect(screen.getByTestId('rendered-time').textContent).toBe('23:30')
      expect(screen.getByTestId('rendered-theme').textContent).toBe(
        DARK_CHART_THEME.bg,
      )

      act(() => setDisplayTimeZone('Asia/Shanghai'))

      expect(screen.getByTestId('rendered-date').textContent).toBe('2026-03-05')
      expect(screen.getByTestId('rendered-time').textContent).toBe('07:30')
      expect(screen.getByTestId('rendered-theme').textContent).toBe(
        DARK_CHART_THEME.bg,
      )

      const englishDate = screen.getByTestId('rendered-locale-date').textContent
      expect(englishDate).toBe(
        new Intl.DateTimeFormat('en-US', {
          year: 'numeric',
          month: '2-digit',
          day: '2-digit',
          timeZone: 'Asia/Shanghai',
        }).format(new Date(BOUNDARY_INSTANT)),
      )

      fireEvent.click(screen.getByRole('button', { name: '中文' }))

      const chineseDate = screen.getByTestId('rendered-locale-date').textContent
      expect(chineseDate).toBe(
        new Intl.DateTimeFormat('zh-CN', {
          year: 'numeric',
          month: '2-digit',
          day: '2-digit',
          timeZone: 'Asia/Shanghai',
        }).format(new Date(BOUNDARY_INSTANT)),
      )
      expect(chineseDate).not.toBe(englishDate)
      expect(screen.getByTestId('rendered-date').textContent).toBe('2026-03-05')
      expect(screen.getByTestId('rendered-time').textContent).toBe('07:30')
      expect(document.documentElement.lang).toBe('zh-CN')

      fireEvent.click(screen.getByRole('button', { name: 'Light' }))

      expect(screen.getByTestId('rendered-theme').textContent).toBe(
        LIGHT_CHART_THEME.bg,
      )
      expect(screen.getByTestId('rendered-date').textContent).toBe('2026-03-05')
      expect(screen.getByTestId('rendered-time').textContent).toBe('07:30')
    } finally {
      view.unmount()
      setDisplayTimeZone(previousZone)
    }
  })

  it('reconciles a stale first-paint class instead of trusting it', () => {
    // The inline first-paint script runs before the module entry, so the class
    // can only be stale when the stored value changed between the two. The
    // provider is the owner and must correct it on mount.
    document.documentElement.classList.add('dark')
    document.documentElement.style.colorScheme = 'dark'
    localStorage.setItem('prompt-ferry:theme-mode', 'light')

    renderProbe()

    expect(document.documentElement.classList.contains('dark')).toBe(false)
    expect(document.documentElement.style.colorScheme).toBe('light')
    expect(localStorage.getItem('prompt-ferry:theme-mode')).toBe('light')
    expect(
      document.documentElement.className.match(/\bdark\b/g) ?? [],
    ).toHaveLength(0)
  })
})
