import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { messages } from '@/i18n'
import { resolveSettingsTab } from '@/models/settings'
import {
  DISPLAY_TIME_ZONE_STORAGE_KEY,
  LOCALE_STORAGE_KEY,
  THEME_MODE_STORAGE_KEY,
} from '@/storage'
import {
  getDisplayTimeZone,
  getDisplayTimeZoneSnapshot,
  setDisplayTimeZone,
} from '@/lib/display-timezone'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { ThemeProvider } from '@/lib/theme'
import { useSessionStore } from '@/lib/session'
import { SettingsPage } from '@/pages/SettingsPage'
import { SettingsGeneralTab } from '@/components/settings/SettingsGeneralTab'

const copy = messages['en-US']

class TestResizeObserver implements ResizeObserver {
  readonly disconnect = vi.fn()
  readonly observe = vi.fn()
  readonly unobserve = vi.fn()

  constructor(_callback: ResizeObserverCallback) {}
}

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  localStorage.setItem('prompt-ferry:theme-mode', 'light')
  setDisplayTimeZone('auto')
  document.documentElement.classList.remove('dark')
  document.documentElement.lang = 'en-US'
  Element.prototype.scrollIntoView = vi.fn()
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: TestResizeObserver,
    })
  }
  useSessionStore.setState({
    bootstrapped: true,
    bootstrapping: false,
    error: '',
    me: {
      display_name: 'Fixture Operator',
      is_admin: false,
      login_name: 'operator',
      user_id: 2,
    },
  })
})

function renderTab() {
  return render(
    <LocaleProvider>
      <ThemeProvider>
        <SettingsGeneralTab />
      </ThemeProvider>
    </LocaleProvider>,
  )
}

function renderPage(path = '/settings/general') {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  return render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <ThemeProvider>
          <NotifierProvider>
            <MemoryRouter initialEntries={[path]}>
              <SettingsPage />
            </MemoryRouter>
          </NotifierProvider>
        </ThemeProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
}

describe('settings general tab', () => {
  it('renders the three preference cards without a save or refresh action', () => {
    renderTab()
    expect(screen.getByRole('heading', { name: copy.language })).toBeTruthy()
    expect(screen.getByRole('heading', { name: copy.theme })).toBeTruthy()
    expect(
      screen.getByRole('heading', { name: copy.displayTimeZone }),
    ).toBeTruthy()
    expect(document.getElementById('settings-locale')).toBeTruthy()
    expect(document.getElementById('settings-theme-mode')).toBeTruthy()
    expect(document.getElementById('settings-display-time-zone')).toBeTruthy()
    expect(screen.queryByRole('button', { name: copy.save })).toBeNull()
    expect(screen.queryByRole('button', { name: copy.refresh })).toBeNull()
    expect(screen.getByText(copy.displayTimeZoneEffect)).toBeTruthy()
  })

  it('applies a language change to the provider, the document and storage', async () => {
    renderTab()
    fireEvent.click(screen.getByRole('combobox', { name: copy.language }))
    fireEvent.click(screen.getByRole('option', { name: '中文' }))

    await waitFor(() => expect(document.documentElement.lang).toBe('zh-CN'))
    expect(localStorage.getItem(LOCALE_STORAGE_KEY)).toBe('zh-CN')
    expect(
      screen.getByRole('heading', { name: messages['zh-CN'].language }),
    ).toBeTruthy()
  })

  it('applies a theme change to the document class and storage', async () => {
    renderTab()
    expect(document.documentElement.classList.contains('dark')).toBe(false)
    fireEvent.click(screen.getByRole('combobox', { name: copy.theme }))
    fireEvent.click(screen.getByRole('option', { name: copy.darkMode }))

    await waitFor(() =>
      expect(document.documentElement.classList.contains('dark')).toBe(true),
    )
    expect(document.documentElement.style.colorScheme).toBe('dark')
    expect(localStorage.getItem(THEME_MODE_STORAGE_KEY)).toBe('dark')
  })

  it('shows the automatic zone first and keeps the manual zones reachable', () => {
    renderTab()
    const trigger = screen.getByRole('button', {
      name: copy.displayTimeZone,
    })
    expect(trigger.textContent).toContain('Automatic')
    fireEvent.click(trigger)
    const options = screen
      .getAllByRole('option')
      .map((item) => item.textContent)
    expect(options[0]).toContain('Automatic')
    expect(options).toContain('UTC')
  })

  it('filters the zone list without touching the stored preference', () => {
    renderTab()
    fireEvent.click(screen.getByRole('button', { name: copy.displayTimeZone }))
    const filter = screen.getByRole('combobox', { name: copy.filter })
    fireEvent.change(filter, { target: { value: 'shanghai' } })

    expect(screen.getAllByRole('option')).toHaveLength(2)
    expect(screen.getByRole('option', { name: 'Asia/Shanghai' })).toBeTruthy()
    expect(getDisplayTimeZone()).toBe('auto')

    fireEvent.change(filter, { target: { value: '' } })
    expect(screen.getAllByRole('option').length).toBeGreaterThan(2)
    expect(getDisplayTimeZone()).toBe('auto')
  })

  it('selects an explicit zone with the keyboard and persists it', async () => {
    renderTab()
    fireEvent.click(screen.getByRole('button', { name: copy.displayTimeZone }))
    const filter = screen.getByRole('combobox', { name: copy.filter })
    fireEvent.change(filter, { target: { value: 'tokyo' } })
    fireEvent.keyDown(filter, { key: 'Enter' })

    await waitFor(() => expect(getDisplayTimeZone()).toBe('Asia/Tokyo'))
    expect(getDisplayTimeZoneSnapshot().effective).toBe('Asia/Tokyo')
    expect(localStorage.getItem(DISPLAY_TIME_ZONE_STORAGE_KEY)).toBe(
      'Asia/Tokyo',
    )
    expect(
      screen.getByRole('button', { name: copy.displayTimeZone }).textContent,
    ).toContain('Asia/Tokyo')
  })

  it('closes with Escape and returns the focus to the trigger', () => {
    renderTab()
    const trigger = screen.getByRole('button', { name: copy.displayTimeZone })
    fireEvent.click(trigger)
    const filter = screen.getByRole('combobox', { name: copy.filter })
    fireEvent.keyDown(filter, { key: 'Escape' })

    expect(screen.queryByRole('listbox')).toBeNull()
    expect(document.activeElement).toBe(trigger)
    expect(getDisplayTimeZone()).toBe('auto')
  })

  it('follows the shared zone store instead of keeping its own copy', async () => {
    renderTab()
    setDisplayTimeZone('Europe/Berlin')

    await waitFor(() =>
      expect(
        screen.getByRole('button', { name: copy.displayTimeZone }).textContent,
      ).toContain('Europe/Berlin'),
    )
  })
})

describe('settings page', () => {
  it('mounts the general route for an ordinary user with its route identity', () => {
    renderPage()
    const section = document.querySelector('[data-foundation-route-id]')
    expect(section?.getAttribute('data-foundation-route-id')).toBe(
      'R-SETTINGS-GENERAL',
    )
    expect(section?.getAttribute('data-settings-tab')).toBe('general')
    expect(
      screen.getByRole('heading', { name: copy.settingsGeneral }),
    ).toBeTruthy()
  })

  it('mounts the archive tab for an admin and falls back for an unknown tab', () => {
    useSessionStore.setState({
      me: {
        display_name: 'Fixture Administrator',
        is_admin: true,
        login_name: 'fixture-admin',
        user_id: 1,
      },
    })
    const adminView = renderPage('/settings/config-archive')
    const section = document.querySelector('[data-foundation-route-id]')
    expect(section?.getAttribute('data-foundation-route-id')).toBe(
      'R-SETTINGS-CONFIG-ARCHIVE',
    )
    expect(section?.getAttribute('data-settings-tab')).toBe('config-archive')
    expect(
      screen.getByRole('heading', { name: copy.settingsConfigArchive }),
    ).toBeTruthy()
    adminView.unmount()

    // An unknown tab is not a route: the resolver normalizes it to general, and
    // the page renders no foundation section rather than inventing a
    // placeholder for it.
    expect(resolveSettingsTab('not-a-settings-tab', true)).toBe('general')
    renderPage('/settings/not-a-settings-tab')
    expect(document.querySelector('[data-foundation-route-id]')).toBeNull()
  })

  it('opens the requests tab for an admin and keeps an ordinary user out', () => {
    useSessionStore.setState({
      me: {
        display_name: 'Fixture Administrator',
        is_admin: true,
        login_name: 'fixture-admin',
        user_id: 1,
      },
    })
    const adminView = renderPage('/settings/requests')
    const section = document.querySelector('[data-foundation-route-id]')
    expect(section?.getAttribute('data-foundation-route-id')).toBe(
      'R-SETTINGS-REQUESTS',
    )
    expect(section?.getAttribute('data-settings-tab')).toBe('requests')
    expect(
      screen.getByRole('heading', { name: copy.settingsUsage }),
    ).toBeTruthy()
    adminView.unmount()

    useSessionStore.setState({ me: null })
    renderPage('/settings/requests')
    expect(document.querySelector('[data-foundation-route-id]')).toBeNull()
  })
})
