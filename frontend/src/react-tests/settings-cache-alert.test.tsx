import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { CacheAlertSettingsResponse } from '@/generated/admin-api'

const api = vi.hoisted(() => ({
  getCacheAlertSetting: vi.fn(),
  setCacheAlertSetting: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

import { messages } from '@/i18n'
import { getPendingActions, runAsyncAction } from '@/lib/async-action'
import {
  cacheAlertFormToRequest,
  cacheAlertToForm,
  CACHE_ALERT_DEFAULTS,
} from '@/admin-mappers/forms/cache-alert'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import {
  cacheAlertIdentityKey,
  disposeSettingsCacheAlert,
  setSettingsCacheAlertNotifier,
  useSettingsCacheAlertStore,
} from '@/lib/settings-cache-alert'
import {
  CACHE_ALERT_BOUNDS,
  CACHE_ALERT_CAPABILITY_CODE,
  CACHE_ALERT_KEY,
  cacheAlertDirty,
  cacheAlertRefreshBlocked,
  clearSavedCacheAlertSecret,
  createCacheAlertForm,
  formFromCacheAlertResponse,
  isCacheAlertCapabilityUnavailable,
  mergeCacheAlertResponse,
  parseCacheAlertInteger,
  parseCacheAlertNumber,
  setCacheAlertSecretDraft,
} from '@/lib/settings-cache-alert-model'
import { SettingsPage } from '@/pages/SettingsPage'
import { SettingsCacheAlertTab } from '@/components/settings/SettingsCacheAlertTab'

const copy = messages['en-US']

const admin = {
  display_name: 'Fixture Administrator',
  is_admin: true,
  login_name: 'fixture-admin',
  user_id: 1,
}

const operator = {
  display_name: 'Fixture Operator',
  is_admin: false,
  login_name: 'fixture-operator',
  user_id: 2,
}

const stored: CacheAlertSettingsResponse = {
  cooldown_minutes: 15,
  dingtalk_secret: '',
  dingtalk_webhook_url:
    'https://oapi.dingtalk.com/robot/send?access_token=fixture',
  enabled: true,
  has_dingtalk_secret: true,
  min_turns: 8,
  threshold: 0.85,
  window_minutes: 30,
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle
    reject = fail
  })
  return { promise, reject, resolve }
}

function queryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
}

function store() {
  return useSettingsCacheAlertStore.getState()
}

async function flushMicrotasks(times = 8) {
  for (let index = 0; index < times; index += 1) {
    await act(async () => {
      await Promise.resolve()
    })
  }
}

async function loadCacheAlert(client = queryClient()) {
  store().initializeIdentity(admin)
  await act(async () => {
    await store().refresh(client)
  })
  return client
}

function renderPage(path = '/settings/cache-alert') {
  return render(
    <QueryClientProvider client={queryClient()}>
      <LocaleProvider>
        <NotifierProvider>
          <MemoryRouter initialEntries={[path]}>
            <SettingsPage />
          </MemoryRouter>
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
}

function capabilityError() {
  return {
    error: {
      code: CACHE_ALERT_CAPABILITY_CODE,
      message: 'this Admin API capability is not available with SQLite yet',
    },
  }
}

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  api.getCacheAlertSetting.mockReset().mockResolvedValue(stored)
  api.setCacheAlertSetting.mockReset()
  useSettingsCacheAlertStore.getState().resetIdentity(null)
  setSettingsCacheAlertNotifier(null)
  useSessionStore.setState({
    bootstrapped: true,
    bootstrapping: false,
    error: '',
    me: admin,
  })
  Element.prototype.scrollIntoView = vi.fn()
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: class {
        readonly disconnect = vi.fn()
        readonly observe = vi.fn()
        readonly unobserve = vi.fn()
        constructor(_callback: ResizeObserverCallback) {}
      },
    })
  }
})

afterEach(() => {
  cleanup()
  disposeSettingsCacheAlert()
  useSettingsCacheAlertStore.getState().resetIdentity(null)
})

describe('cache alert pure model and mapper', () => {
  it('starts from the backend defaults with the secret on keep', () => {
    expect(createCacheAlertForm()).toEqual({
      enabled: CACHE_ALERT_DEFAULTS.enabled,
      window_minutes: CACHE_ALERT_DEFAULTS.window_minutes,
      min_turns: CACHE_ALERT_DEFAULTS.min_turns,
      threshold: CACHE_ALERT_DEFAULTS.threshold,
      cooldown_minutes: CACHE_ALERT_DEFAULTS.cooldown_minutes,
      dingtalk_webhook_url: '',
      has_dingtalk_secret: false,
      secret: { mode: 'keep' },
    })
  })

  it('copies presence from the server and never derives it from a draft', () => {
    const form = cacheAlertToForm({ ...stored, has_dingtalk_secret: false })
    expect(form.has_dingtalk_secret).toBe(false)
    expect(form.dingtalk_webhook_url).toBe(stored.dingtalk_webhook_url)

    const typed = setCacheAlertSecretDraft(form, 'synthetic-draft')
    expect(typed.secret).toEqual({
      mode: 'replace',
      value: 'synthetic-draft',
    })
    expect(typed.has_dingtalk_secret).toBe(false)

    const blank = setCacheAlertSecretDraft(typed, '   ')
    expect(blank.secret.mode).toBe('keep')
    expect(blank.has_dingtalk_secret).toBe(false)
  })

  it('sends every field, trims the entries and never the derived flag', () => {
    const form = setCacheAlertSecretDraft(
      {
        ...createCacheAlertForm(),
        dingtalk_webhook_url: '  https://oapi.dingtalk.com/robot/send  ',
        enabled: true,
      },
      '  synthetic-secret  ',
    )
    const body = cacheAlertFormToRequest(form)
    expect(Object.keys(body).sort()).toEqual([
      'cooldown_minutes',
      'dingtalk_secret',
      'dingtalk_webhook_url',
      'enabled',
      'min_turns',
      'threshold',
      'window_minutes',
    ])
    expect(body.dingtalk_webhook_url).toBe(
      'https://oapi.dingtalk.com/robot/send',
    )
    expect(body.dingtalk_secret).toBe('synthetic-secret')
    expect(body.enabled).toBe(true)

    const keepBody = cacheAlertFormToRequest(createCacheAlertForm())
    expect(keepBody.dingtalk_secret).toBe('')
  })

  it('parses the float threshold exactly and rejects out-of-range integers', () => {
    expect(parseCacheAlertNumber('0', CACHE_ALERT_BOUNDS.threshold)).toBe(0)
    expect(parseCacheAlertNumber('1', CACHE_ALERT_BOUNDS.threshold)).toBe(1)
    expect(parseCacheAlertNumber('0.85', CACHE_ALERT_BOUNDS.threshold)).toBe(
      0.85,
    )
    expect(
      parseCacheAlertNumber('1.01', CACHE_ALERT_BOUNDS.threshold),
    ).toBeNull()
    expect(
      parseCacheAlertNumber('-0.01', CACHE_ALERT_BOUNDS.threshold),
    ).toBeNull()
    expect(parseCacheAlertNumber('', CACHE_ALERT_BOUNDS.threshold)).toBeNull()
    expect(
      parseCacheAlertNumber('abc', CACHE_ALERT_BOUNDS.threshold),
    ).toBeNull()

    expect(parseCacheAlertInteger('5', CACHE_ALERT_BOUNDS.window_minutes)).toBe(
      5,
    )
    expect(
      parseCacheAlertInteger('1440', CACHE_ALERT_BOUNDS.window_minutes),
    ).toBe(1440)
    expect(
      parseCacheAlertInteger('4', CACHE_ALERT_BOUNDS.window_minutes),
    ).toBeNull()
    expect(
      parseCacheAlertInteger('1441', CACHE_ALERT_BOUNDS.window_minutes),
    ).toBeNull()
    expect(
      parseCacheAlertInteger('5.5', CACHE_ALERT_BOUNDS.window_minutes),
    ).toBeNull()
    expect(parseCacheAlertInteger('1', CACHE_ALERT_BOUNDS.min_turns)).toBeNull()
  })

  it('merges only untouched leaves and always takes the server presence', () => {
    const submitted = formFromCacheAlertResponse(stored)
    const current = {
      ...submitted,
      window_minutes: 45,
      secret: { mode: 'replace' as const, value: 'synthetic-draft' },
    }
    const merged = mergeCacheAlertResponse(current, submitted, {
      ...stored,
      window_minutes: 60,
      threshold: 0.5,
      has_dingtalk_secret: false,
    })
    expect(merged.window_minutes).toBe(45)
    expect(merged.threshold).toBe(0.5)
    expect(merged.has_dingtalk_secret).toBe(false)
    expect(merged.secret).toEqual({
      mode: 'replace',
      value: 'synthetic-draft',
    })
  })

  it('clears only the secret draft a save consumed', () => {
    const submitted = setCacheAlertSecretDraft(
      createCacheAlertForm(),
      'submitted-secret',
    )
    expect(clearSavedCacheAlertSecret(submitted, submitted).secret).toEqual({
      mode: 'keep',
    })

    const newer = setCacheAlertSecretDraft(submitted, 'newer-secret')
    expect(clearSavedCacheAlertSecret(newer, submitted).secret).toEqual({
      mode: 'replace',
      value: 'newer-secret',
    })
  })

  it('treats any leaf or secret edit as dirty and blocks a refresh accordingly', () => {
    const baseline = createCacheAlertForm()
    expect(cacheAlertDirty(baseline, baseline)).toBe(false)
    expect(cacheAlertDirty({ ...baseline, threshold: 0.9 }, baseline)).toBe(
      true,
    )
    expect(
      cacheAlertDirty(
        setCacheAlertSecretDraft(baseline, 'synthetic-draft'),
        baseline,
      ),
    ).toBe(true)

    const base = {
      pendingActions: {},
      dirty: false,
      failed: false,
      loading: false,
      loaded: true,
    }
    expect(cacheAlertRefreshBlocked(base)).toBe(false)
    expect(cacheAlertRefreshBlocked({ ...base, loaded: false })).toBe(true)
    expect(cacheAlertRefreshBlocked({ ...base, loading: true })).toBe(true)
    expect(cacheAlertRefreshBlocked({ ...base, dirty: true })).toBe(true)
    expect(cacheAlertRefreshBlocked({ ...base, failed: true })).toBe(true)
    expect(
      cacheAlertRefreshBlocked({
        ...base,
        pendingActions: { [CACHE_ALERT_KEY]: 'cacheAlert' },
      }),
    ).toBe(true)
  })

  it('reads the structured capability code, never an error message', () => {
    expect(isCacheAlertCapabilityUnavailable(capabilityError())).toBe(true)
    expect(
      isCacheAlertCapabilityUnavailable(new Error(CACHE_ALERT_CAPABILITY_CODE)),
    ).toBe(false)
    expect(
      isCacheAlertCapabilityUnavailable({
        error: { code: 'bad_request', message: 'x' },
      }),
    ).toBe(false)
  })
})

describe('cache alert store', () => {
  it('reads once and writes nothing', async () => {
    await loadCacheAlert()
    expect(api.getCacheAlertSetting).toHaveBeenCalledTimes(1)
    expect(api.setCacheAlertSetting).not.toHaveBeenCalled()
    expect(store().loaded).toBe(true)
    expect(store().form.window_minutes).toBe(30)
    expect(store().form.threshold).toBe(0.85)
    expect(store().form.min_turns).toBe(8)
    expect(store().form.has_dingtalk_secret).toBe(true)
    expect(store().form.secret).toEqual({ mode: 'keep' })
  })

  it('refuses to save before a successful read', async () => {
    store().initializeIdentity(admin)
    expect(await store().save()).toEqual({ executed: false })
    expect(api.setCacheAlertSetting).not.toHaveBeenCalled()
  })

  it('saves the whole body with the keep rule and confirms once', async () => {
    await loadCacheAlert()
    const notifySaved = vi.fn()
    setSettingsCacheAlertNotifier({
      notifyApiError: vi.fn(),
      notifySaved,
    })
    api.setCacheAlertSetting.mockResolvedValueOnce({
      ...stored,
      threshold: 0.5,
    })

    store().setField('threshold', 0.5)
    await act(async () => {
      await store().save()
    })

    expect(api.setCacheAlertSetting).toHaveBeenCalledTimes(1)
    const body = api.setCacheAlertSetting.mock.calls[0]?.[0]?.body
    expect(body).toEqual({
      enabled: true,
      window_minutes: 30,
      min_turns: 8,
      threshold: 0.5,
      cooldown_minutes: 15,
      dingtalk_webhook_url: stored.dingtalk_webhook_url,
      dingtalk_secret: '',
    })
    expect(Object.hasOwn(body, 'has_dingtalk_secret')).toBe(false)
    expect(notifySaved).toHaveBeenCalledTimes(1)
    expect(store().baseline.threshold).toBe(0.5)
  })

  it('sends a trimmed replace and clears the consumed draft', async () => {
    await loadCacheAlert()
    api.setCacheAlertSetting.mockResolvedValueOnce(stored)

    store().setSecretDraft('  synthetic-browser-secret  ')
    expect(store().form.secret.mode).toBe('replace')
    await act(async () => {
      await store().save()
    })

    const body = api.setCacheAlertSetting.mock.calls[0]?.[0]?.body
    expect(body.dingtalk_secret).toBe('synthetic-browser-secret')
    expect(store().form.secret).toEqual({ mode: 'keep' })
    expect(store().form.has_dingtalk_secret).toBe(true)
  })

  it('keeps a secret typed during the write and stays dirty', async () => {
    await loadCacheAlert()
    const write = deferred<CacheAlertSettingsResponse>()
    api.setCacheAlertSetting.mockReturnValueOnce(write.promise)

    store().setSecretDraft('submitted-secret')
    const pending = store().save()
    await flushMicrotasks(2)
    store().setSecretDraft('newer-secret')

    write.resolve(stored)
    await act(async () => {
      await pending
    })

    expect(store().form.secret).toEqual({
      mode: 'replace',
      value: 'newer-secret',
    })
    expect(cacheAlertDirty(store().form, store().baseline)).toBe(true)
  })

  it('reports one error, keeps the draft and retries through Save', async () => {
    await loadCacheAlert()
    const notifyApiError = vi.fn()
    setSettingsCacheAlertNotifier({
      notifyApiError,
      notifySaved: vi.fn(),
    })
    api.setCacheAlertSetting
      .mockRejectedValueOnce(new Error('503 service unavailable'))
      .mockResolvedValueOnce({ ...stored, window_minutes: 90 })

    store().setField('window_minutes', 90)
    await act(async () => {
      await store().save()
    })
    expect(notifyApiError).toHaveBeenCalledTimes(1)
    expect(store().saveFailed).toBe(true)
    expect(store().form.window_minutes).toBe(90)

    await act(async () => {
      await store().save()
    })
    expect(api.setCacheAlertSetting).toHaveBeenCalledTimes(2)
    expect(store().saveFailed).toBe(false)
    expect(store().baseline.window_minutes).toBe(90)
  })

  it('sends one request and one confirmation for a double save', async () => {
    await loadCacheAlert()
    const notifySaved = vi.fn()
    setSettingsCacheAlertNotifier({
      notifyApiError: vi.fn(),
      notifySaved,
    })
    const write = deferred<CacheAlertSettingsResponse>()
    api.setCacheAlertSetting.mockReturnValueOnce(write.promise)

    const first = store().save()
    await flushMicrotasks(2)
    expect(await store().save()).toEqual({ executed: false })
    write.resolve(stored)
    await act(async () => {
      await first
    })

    expect(api.setCacheAlertSetting).toHaveBeenCalledTimes(1)
    expect(notifySaved).toHaveBeenCalledTimes(1)
  })

  it('does not block an unrelated settings key', async () => {
    await loadCacheAlert()
    api.setCacheAlertSetting.mockResolvedValueOnce(stored)
    const release: Array<() => void> = []
    const held = runAsyncAction(
      'setting:storage',
      () => new Promise<void>((resolve) => release.push(resolve)),
      'storage',
    )
    await flushMicrotasks(1)

    await act(async () => {
      await store().save()
    })
    expect(api.setCacheAlertSetting).toHaveBeenCalledTimes(1)
    release.forEach((resolve) => resolve())
    await held
  })

  it('reports the capability state explicitly and retries the read', async () => {
    store().initializeIdentity(admin)
    const client = queryClient()
    api.getCacheAlertSetting
      .mockRejectedValueOnce(capabilityError())
      .mockResolvedValueOnce(stored)

    await act(async () => {
      await store().refresh(client)
    })
    expect(store().unavailable).toBe(true)
    expect(store().unavailableMessage).toBe(
      `${CACHE_ALERT_CAPABILITY_CODE}: this Admin API capability is not available with SQLite yet`,
    )
    expect(store().loaded).toBe(false)
    expect(await store().save()).toEqual({ executed: false })
    expect(api.setCacheAlertSetting).not.toHaveBeenCalled()

    await act(async () => {
      await store().refresh(client)
    })
    expect(store().unavailable).toBe(false)
    expect(store().loaded).toBe(true)
  })

  it('treats a generic read failure as retryable, not a capability answer', async () => {
    store().initializeIdentity(admin)
    const client = queryClient()
    api.getCacheAlertSetting.mockRejectedValueOnce(new Error('500 internal'))

    await act(async () => {
      await store().refresh(client)
    })
    expect(store().unavailable).toBe(false)
    expect(store().loadError).toBe(true)
  })

  it('commits only the latest read, drops a pre-edit read and a stale identity', async () => {
    store().initializeIdentity(admin)
    const client = queryClient()
    const resolvers: Array<(value: CacheAlertSettingsResponse) => void> = []
    api.getCacheAlertSetting.mockImplementation(
      () =>
        new Promise<CacheAlertSettingsResponse>((resolve) => {
          resolvers.push(resolve)
        }),
    )

    void store().refresh(client)
    void store().refresh(client)
    await flushMicrotasks(4)
    const current = resolvers.at(-1) as (
      value: CacheAlertSettingsResponse,
    ) => void
    current({ ...stored, window_minutes: 120 })
    await flushMicrotasks(8)
    expect(store().form.window_minutes).toBe(120)
    for (const stale of resolvers.slice(0, -1)) {
      stale({ ...stored, window_minutes: 7 })
    }
    await flushMicrotasks(8)
    expect(store().form.window_minutes).toBe(120)

    const slow = deferred<CacheAlertSettingsResponse>()
    api.getCacheAlertSetting.mockReturnValueOnce(slow.promise)
    void store().refresh(client)
    await flushMicrotasks(2)
    store().setField('window_minutes', 45)
    expect(store().loading).toBe(false)
    slow.resolve({ ...stored, window_minutes: 10 })
    await flushMicrotasks(8)
    expect(store().form.window_minutes).toBe(45)

    const identityRead = deferred<CacheAlertSettingsResponse>()
    api.getCacheAlertSetting.mockReturnValueOnce(identityRead.promise)
    const read = store().refresh(client)
    store().resetIdentity(operator)
    identityRead.resolve(stored)
    await act(async () => {
      await read
    })
    expect(store().identityKey).toBe(cacheAlertIdentityKey(operator))
    expect(store().loaded).toBe(false)
    expect(store().form.secret).toEqual({ mode: 'keep' })
  })

  it('refuses a refresh while dirty and never refetches on rerender', async () => {
    const client = await loadCacheAlert()
    store().setField('threshold', 0.9)
    await act(async () => {
      await store().refresh(client)
    })
    expect(api.getCacheAlertSetting).toHaveBeenCalledTimes(1)

    store().resetIdentity(admin)
    const fresh = await loadCacheAlert(queryClient())
    await act(async () => {
      await store().ensureLoaded(fresh)
    })
    expect(api.getCacheAlertSetting).toHaveBeenCalledTimes(2)
  })

  it('clears the secret draft on unmount', async () => {
    const client = await loadCacheAlert()
    store().setSecretDraft('draft-secret')
    disposeSettingsCacheAlert()
    expect(store().form.secret).toEqual({ mode: 'keep' })
    expect(store().loaded).toBe(false)

    await act(async () => {
      await store().ensureLoaded(client)
    })
    expect(api.getCacheAlertSetting).toHaveBeenCalledTimes(2)
  })
})

describe('cache alert tab', () => {
  it('renders the loaded policy from one read with no write', async () => {
    renderPage()
    await flushMicrotasks()

    const section = document.querySelector('[data-foundation-route-id]')
    expect(section?.getAttribute('data-foundation-route-id')).toBe(
      'R-SETTINGS-CACHE-ALERT',
    )
    expect(section?.getAttribute('data-settings-tab')).toBe('cache-alert')
    expect(
      screen.getByRole('heading', {
        level: 1,
        name: copy.settingsCacheAlert,
      }),
    ).toBeTruthy()
    expect(
      document
        .getElementById('settings-cache-alert-enabled')
        ?.getAttribute('data-state'),
    ).toBe('checked')
    expect(
      (
        document.getElementById(
          'settings-cache-alert-window-minutes',
        ) as HTMLInputElement | null
      )?.value,
    ).toBe('30')
    expect(
      (
        document.getElementById(
          'settings-cache-alert-min-turns',
        ) as HTMLInputElement | null
      )?.value,
    ).toBe('8')
    expect(
      (
        document.getElementById(
          'settings-cache-alert-threshold',
        ) as HTMLInputElement | null
      )?.value,
    ).toBe('0.85')
    expect(
      (
        document.getElementById(
          'settings-cache-alert-cooldown-minutes',
        ) as HTMLInputElement | null
      )?.value,
    ).toBe('15')
    expect(
      (
        document.getElementById(
          'settings-cache-alert-webhook-url',
        ) as HTMLInputElement | null
      )?.value,
    ).toBe(stored.dingtalk_webhook_url)
    expect(
      (
        document.getElementById(
          'settings-cache-alert-save',
        ) as HTMLButtonElement
      ).disabled,
    ).toBe(false)
    expect(screen.getByText(copy.cacheAlertHint)).toBeTruthy()
    expect(api.setCacheAlertSetting).not.toHaveBeenCalled()
  })

  it('shows both badges: server presence and the local draft mode', async () => {
    renderPage()
    await flushMicrotasks()

    const presence = () =>
      document
        .querySelector('[data-secret-present]')
        ?.getAttribute('data-secret-present')
    const mode = () =>
      document
        .querySelector('[data-secret-mode]')
        ?.getAttribute('data-secret-mode')
    expect(presence()).toBe('true')
    expect(mode()).toBe('keep')
    expect(screen.getByText(copy.cacheAlertSecretConfigured)).toBeTruthy()
    expect(screen.getByText(copy.cacheAlertSecretKeeping)).toBeTruthy()

    const secret = document.getElementById(
      'settings-cache-alert-secret',
    ) as HTMLInputElement
    expect(secret.getAttribute('type')).toBe('password')
    await act(async () => {
      fireEvent.change(secret, { target: { value: 'typed-secret' } })
    })
    expect(mode()).toBe('replace')
    expect(screen.getByText(copy.cacheAlertSecretReplacing)).toBeTruthy()
    expect(presence()).toBe('true')

    await act(async () => {
      fireEvent.change(secret, { target: { value: '   ' } })
    })
    expect(mode()).toBe('keep')
    expect(presence()).toBe('true')
  })

  it('never renders a stored secret and offers no clear action', async () => {
    renderPage()
    await flushMicrotasks()

    expect(
      document.body.textContent?.includes('fixture-synthetic-dingtalk'),
    ).toBe(false)
    for (const node of document.querySelectorAll('[title]')) {
      expect(node.getAttribute('title') ?? '').not.toContain('synthetic')
    }
    const storedValues = Object.keys(localStorage).map((key) =>
      String(localStorage.getItem(key)),
    )
    expect(storedValues.join('\n')).not.toContain('dingtalk-secret')
    expect(
      [...document.querySelectorAll('button')].some((node) =>
        /^(clear|清除)$/i.test((node.textContent ?? '').trim()),
      ),
    ).toBe(false)
    expect(
      (
        document.getElementById(
          'settings-cache-alert-secret',
        ) as HTMLInputElement | null
      )?.value,
    ).toBe('')
  })

  it('keeps the last accepted number when the editor text is invalid', async () => {
    renderPage()
    await flushMicrotasks()

    const windowField = document.getElementById(
      'settings-cache-alert-window-minutes',
    ) as HTMLInputElement
    await act(async () => {
      fireEvent.change(windowField, { target: { value: '2000' } })
    })
    expect(windowField.value).toBe('2000')
    expect(store().form.window_minutes).toBe(30)

    await act(async () => {
      fireEvent.blur(windowField)
    })
    expect(windowField.value).toBe('30')

    const thresholdField = document.getElementById(
      'settings-cache-alert-threshold',
    ) as HTMLInputElement
    await act(async () => {
      fireEvent.change(thresholdField, { target: { value: '0' } })
    })
    expect(store().form.threshold).toBe(0)
    await act(async () => {
      fireEvent.change(thresholdField, { target: { value: '1' } })
    })
    expect(store().form.threshold).toBe(1)
  })

  it('shows the capability warning with its retry and never the form', async () => {
    api.getCacheAlertSetting
      .mockRejectedValueOnce(capabilityError())
      .mockResolvedValueOnce(stored)
    renderPage()
    await flushMicrotasks()

    expect(screen.getByText(copy.cacheAlertUnavailable)).toBeTruthy()
    expect(screen.getByText(/capability_unavailable/)).toBeTruthy()
    expect(
      document.getElementById('settings-cache-alert-window-minutes'),
    ).toBeNull()
    expect(
      (
        document.getElementById(
          'settings-cache-alert-save',
        ) as HTMLButtonElement
      ).disabled,
    ).toBe(true)

    fireEvent.click(
      document.getElementById(
        'settings-cache-alert-retry',
      ) as HTMLButtonElement,
    )
    await flushMicrotasks()
    expect(api.getCacheAlertSetting).toHaveBeenCalledTimes(2)
    await waitFor(() =>
      expect(
        document.getElementById('settings-cache-alert-window-minutes'),
      ).not.toBeNull(),
    )
  })

  it('blocks the scoped refresh while the form is dirty', async () => {
    renderPage()
    await flushMicrotasks()

    const refreshButton = document.getElementById(
      'settings-cache-alert-refresh',
    ) as HTMLButtonElement
    expect(refreshButton.disabled).toBe(false)

    await act(async () => {
      store().setField('threshold', 0.6)
    })
    expect(refreshButton.disabled).toBe(true)
  })

  it('keeps an ordinary user out of the cache alert route', async () => {
    useSessionStore.setState({ me: operator })
    renderPage()
    await flushMicrotasks()
    expect(
      document.querySelector(
        '[data-foundation-route-id="R-SETTINGS-CACHE-ALERT"]',
      ),
    ).toBeNull()
    expect(api.getCacheAlertSetting).not.toHaveBeenCalled()
  })

  it('renders the tab card without issuing its own read', async () => {
    render(
      <QueryClientProvider client={queryClient()}>
        <LocaleProvider>
          <NotifierProvider>
            <SettingsCacheAlertTab />
          </NotifierProvider>
        </LocaleProvider>
      </QueryClientProvider>,
    )
    await flushMicrotasks()
    expect(api.getCacheAlertSetting).not.toHaveBeenCalled()
    expect(
      document.getElementById('settings-cache-alert-enabled'),
    ).not.toBeNull()
    expect(document.getElementById('settings-cache-alert-save')).not.toBeNull()
    expect(Object.keys(getPendingActions())).toEqual([])
  })
})
