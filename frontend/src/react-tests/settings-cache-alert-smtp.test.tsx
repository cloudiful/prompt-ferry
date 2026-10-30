import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
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
import {
  cacheAlertFormToRequest,
  cacheAlertToForm,
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
  cacheAlertDirty,
  cacheAlertSmtpIssues,
  createCacheAlertForm,
  formFromCacheAlertResponse,
  mergeCacheAlertResponse,
  setCacheAlertSmtpPasswordDraft,
} from '@/lib/settings-cache-alert-model'
import { SettingsPage } from '@/pages/SettingsPage'

const copy = messages['en-US']

const admin = {
  display_name: 'Fixture Administrator',
  is_admin: true,
  login_name: 'fixture-admin',
  user_id: 1,
}

/** A response from a server that never knew the SMTP channel. */
const legacyStored: CacheAlertSettingsResponse = {
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

/** A response with a stored SMTP channel and a stored password. */
const smtpStored: CacheAlertSettingsResponse = {
  ...legacyStored,
  smtp: {
    enabled: true,
    from: 'alerts@example.com',
    has_smtp_password: true,
    host: 'smtp.example.com',
    password: '',
    port: 587,
    tls: 'start_tls',
    to: ['ops@example.com', 'oncall@example.com'],
    username: 'alert-user',
  },
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
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

async function loadCacheAlert(response: CacheAlertSettingsResponse) {
  api.getCacheAlertSetting.mockResolvedValue(response)
  store().initializeIdentity(admin)
  await act(async () => {
    await store().refresh(queryClient())
  })
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

function input(id: string): HTMLInputElement {
  const element = document.getElementById(id)
  if (!element) throw new Error(`missing control: ${id}`)
  return element as HTMLInputElement
}

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  api.getCacheAlertSetting.mockReset().mockResolvedValue(legacyStored)
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

describe('cache alert SMTP model', () => {
  it('projects an old response without the channel and keeps it out of the write', () => {
    const form = cacheAlertToForm(legacyStored)
    expect(form.smtp.enabled).toBe(false)
    expect(form.smtp.host).toBe('')
    expect(form.smtp.port).toBe(465)
    expect(form.smtp.tls).toBe('implicit_tls')
    expect(form.smtp.has_password).toBe(false)
    expect(form.smtp.configured).toBe(false)
    expect(form.smtp.password).toEqual({ mode: 'keep' })

    const body = cacheAlertFormToRequest(form)
    expect(Object.hasOwn(body, 'smtp')).toBe(false)
  })

  it('projects a stored channel with the server-owned facts', () => {
    const form = cacheAlertToForm(smtpStored)
    expect(form.smtp.enabled).toBe(true)
    expect(form.smtp.host).toBe('smtp.example.com')
    expect(form.smtp.port).toBe(587)
    expect(form.smtp.tls).toBe('start_tls')
    expect(form.smtp.to).toEqual(['ops@example.com', 'oncall@example.com'])
    expect(form.smtp.username).toBe('alert-user')
    expect(form.smtp.has_password).toBe(true)
    expect(form.smtp.stored_username).toBe('alert-user')
    expect(form.smtp.configured).toBe(true)
  })

  it('sends the channel, its transport and the recipients as an array', () => {
    const form = cacheAlertToForm(smtpStored)
    const body = cacheAlertFormToRequest(form)
    expect(body.smtp).toEqual({
      enabled: true,
      from: 'alerts@example.com',
      host: 'smtp.example.com',
      password: '',
      port: 587,
      to: ['ops@example.com', 'oncall@example.com'],
      tls: 'start_tls',
      username: 'alert-user',
    })
  })

  it('drops the empty recipient entries and trims the addresses', () => {
    const form = cacheAlertToForm(smtpStored)
    const body = cacheAlertFormToRequest({
      ...form,
      smtp: {
        ...form.smtp,
        to: [' ops@example.com ', '', 'oncall@example.com'],
      },
    })
    expect(body.smtp?.to).toEqual(['ops@example.com', 'oncall@example.com'])
  })

  it('carries the transport choice of the form', () => {
    const form = cacheAlertToForm(smtpStored)
    const body = cacheAlertFormToRequest({
      ...form,
      smtp: { ...form.smtp, tls: 'implicit_tls' },
    })
    expect(body.smtp?.tls).toBe('implicit_tls')
  })

  it('keeps a blank password and sends a typed one', () => {
    const form = cacheAlertToForm(smtpStored)
    expect(cacheAlertFormToRequest(form).smtp?.password).toBe('')

    const typed = setCacheAlertSmtpPasswordDraft(form, '  synthetic-secret  ')
    expect(typed.smtp.password).toEqual({
      mode: 'replace',
      value: '  synthetic-secret  ',
    })
    expect(cacheAlertFormToRequest(typed).smtp?.password).toBe(
      'synthetic-secret',
    )

    const blank = setCacheAlertSmtpPasswordDraft(typed, '   ')
    expect(blank.smtp.password.mode).toBe('keep')
    expect(cacheAlertFormToRequest(blank).smtp?.password).toBe('')
  })

  it('reports the same credential rule the server applies', () => {
    const form = cacheAlertToForm(smtpStored)
    expect(cacheAlertSmtpIssues(form)).toEqual([])

    // The stored password goes with the stored username, so a changed
    // username leaves the channel without one.
    const renamed = { ...form, smtp: { ...form.smtp, username: 'other-user' } }
    expect(cacheAlertSmtpIssues(renamed)).toEqual([
      { code: 'credentials_paired', field: 'credentials' },
    ])

    const withPassword = setCacheAlertSmtpPasswordDraft(
      renamed,
      'synthetic-secret',
    )
    expect(cacheAlertSmtpIssues(withPassword)).toEqual([])

    // A username without any password available is unpaired, exactly as the
    // server sees it after it drops a stored password for a changed username.
    const withoutStored = {
      ...form,
      smtp: { ...form.smtp, has_password: false },
    }
    expect(cacheAlertSmtpIssues(withoutStored)).toEqual([
      { code: 'credentials_paired', field: 'credentials' },
    ])

    // An empty username pairs with no password, so the server accepts it and
    // drops the stored one.
    const noAuth = {
      ...form,
      smtp: { ...form.smtp, username: '' },
    }
    expect(cacheAlertSmtpIssues(noAuth)).toEqual([])
  })

  it('reports the required fields and the invalid addresses', () => {
    const form = cacheAlertToForm(smtpStored)
    const enabled = { ...form, smtp: { ...form.smtp, enabled: true } }

    const missing = {
      ...enabled,
      smtp: { ...enabled.smtp, host: '', from: '' },
    }
    expect(cacheAlertSmtpIssues(missing).map((issue) => issue.code)).toEqual([
      'host_required',
      'from_required',
    ])

    const invalid = {
      ...enabled,
      smtp: {
        ...enabled.smtp,
        host: 'smtp://smtp.example.com',
        from: 'not-an-address',
        to: ['ops@example.com', 'nope'],
      },
    }
    expect(cacheAlertSmtpIssues(invalid)).toEqual([
      { code: 'host_invalid', field: 'host' },
      { code: 'from_invalid', field: 'from' },
      { code: 'recipient_invalid', field: 'to', index: 1 },
    ])
  })

  it('keeps a newer edit of the channel across a save', () => {
    const submitted = formFromCacheAlertResponse(smtpStored)
    const current = {
      ...submitted,
      smtp: { ...submitted.smtp, host: 'smtp.typed.example.com' },
    }
    const merged = mergeCacheAlertResponse(current, submitted, {
      ...smtpStored,
      smtp: { ...smtpStored.smtp!, host: 'smtp.stored.example.com' },
    })
    expect(merged.smtp.host).toBe('smtp.typed.example.com')
    expect(merged.smtp.has_password).toBe(true)
    expect(
      cacheAlertDirty(merged, formFromCacheAlertResponse(smtpStored)),
    ).toBe(true)
  })
})

describe('cache alert SMTP store', () => {
  it('saves the channel and clears the consumed password draft', async () => {
    await loadCacheAlert(smtpStored)
    api.setCacheAlertSetting.mockResolvedValueOnce(smtpStored)

    store().setSmtpField('host', 'smtp.typed.example.com')
    store().setSmtpPasswordDraft('  synthetic-secret  ')
    await act(async () => {
      await store().save()
    })

    const body = api.setCacheAlertSetting.mock.calls[0]?.[0]?.body
    expect(body.smtp).toEqual({
      enabled: true,
      from: 'alerts@example.com',
      host: 'smtp.typed.example.com',
      password: 'synthetic-secret',
      port: 587,
      to: ['ops@example.com', 'oncall@example.com'],
      tls: 'start_tls',
      username: 'alert-user',
    })
    expect(store().form.smtp.password).toEqual({ mode: 'keep' })
    expect(store().form.smtp.has_password).toBe(true)
  })

  it('keeps a password typed during the write', async () => {
    await loadCacheAlert(smtpStored)
    const write = deferred<CacheAlertSettingsResponse>()
    api.setCacheAlertSetting.mockReturnValueOnce(write.promise)

    store().setSmtpPasswordDraft('submitted-secret')
    const pending = store().save()
    await flushMicrotasks(2)
    store().setSmtpPasswordDraft('newer-secret')

    write.resolve(smtpStored)
    await act(async () => {
      await pending
    })

    expect(store().form.smtp.password).toEqual({
      mode: 'replace',
      value: 'newer-secret',
    })
    expect(cacheAlertDirty(store().form, store().baseline)).toBe(true)
  })

  it('keeps the draft after a failed save and retries through Save', async () => {
    await loadCacheAlert(smtpStored)
    const notifyApiError = vi.fn()
    setSettingsCacheAlertNotifier({
      notifyApiError,
      notifySaved: vi.fn(),
    })
    api.setCacheAlertSetting
      .mockRejectedValueOnce(new Error('503 service unavailable'))
      .mockResolvedValueOnce(smtpStored)

    store().setSmtpField('host', 'smtp.retry.example.com')
    await act(async () => {
      await store().save()
    })
    expect(notifyApiError).toHaveBeenCalledTimes(1)
    expect(store().saveFailed).toBe(true)
    expect(store().form.smtp.host).toBe('smtp.retry.example.com')

    await act(async () => {
      await store().save()
    })
    expect(api.setCacheAlertSetting).toHaveBeenCalledTimes(2)
    expect(store().saveFailed).toBe(false)
  })

  it('drops the channel and the draft when the identity changes', async () => {
    await loadCacheAlert(smtpStored)
    store().setSmtpPasswordDraft('synthetic-secret')

    store().resetIdentity(null)
    expect(store().identityKey).toBe('')
    expect(store().form.smtp.password).toEqual({ mode: 'keep' })
    expect(store().form.smtp.has_password).toBe(false)
    expect(store().loaded).toBe(false)

    store().initializeIdentity(admin)
    expect(store().identityKey).toBe(cacheAlertIdentityKey(admin))
  })
})

describe('cache alert SMTP tab', () => {
  it('renders the channel with accessible names and a hidden password', async () => {
    await loadCacheAlert(smtpStored)
    renderPage()
    await flushMicrotasks()

    expect(
      document.getElementById('settings-cache-alert-smtp-enabled'),
    ).not.toBeNull()
    expect(screen.getByLabelText(copy.cacheAlertSmtpEnabled)).toBeTruthy()
    expect(screen.getByLabelText(copy.cacheAlertSmtpHost)).toBeTruthy()
    expect(screen.getByLabelText(copy.cacheAlertSmtpTls)).toBeTruthy()
    expect(screen.getByLabelText(copy.cacheAlertSmtpFrom)).toBeTruthy()
    expect(
      screen.getByLabelText(
        copy.cacheAlertSmtpRecipient.replace('{index}', '1'),
      ),
    ).toBeTruthy()
    expect(
      screen.getByLabelText(
        copy.cacheAlertSmtpRecipient.replace('{index}', '2'),
      ),
    ).toBeTruthy()
    expect(screen.getByLabelText(copy.cacheAlertSmtpUsername)).toBeTruthy()
    expect(screen.getByLabelText(copy.cacheAlertSmtpPassword)).toBeTruthy()

    expect(input('settings-cache-alert-smtp-host').value).toBe(
      'smtp.example.com',
    )
    expect(input('settings-cache-alert-smtp-port').value).toBe('587')
    expect(input('settings-cache-alert-smtp-from').value).toBe(
      'alerts@example.com',
    )
    expect(input('settings-cache-alert-smtp-username').value).toBe('alert-user')
    expect(input('settings-cache-alert-smtp-to-0').value).toBe(
      'ops@example.com',
    )

    const password = input('settings-cache-alert-smtp-password')
    expect(password.getAttribute('type')).toBe('password')
    expect(password.value).toBe('')
    expect(
      document
        .querySelector('[data-smtp-password-present]')
        ?.getAttribute('data-smtp-password-present'),
    ).toBe('true')
    expect(document.body.textContent).not.toContain('synthetic-secret')
  })

  it('adds a recipient, sends the array and never echoes the password', async () => {
    await loadCacheAlert(legacyStored)
    api.setCacheAlertSetting.mockResolvedValueOnce({
      ...legacyStored,
      smtp: {
        ...smtpStored.smtp!,
        enabled: true,
        to: ['ops@example.com'],
        username: '',
        has_smtp_password: false,
      },
    })
    renderPage()
    await flushMicrotasks()

    await act(async () => {
      fireEvent.click(input('settings-cache-alert-smtp-enabled'))
    })
    await act(async () => {
      fireEvent.change(input('settings-cache-alert-smtp-host'), {
        target: { value: 'smtp.example.com' },
      })
    })
    await act(async () => {
      fireEvent.change(input('settings-cache-alert-smtp-from'), {
        target: { value: 'alerts@example.com' },
      })
    })
    await act(async () => {
      fireEvent.change(input('settings-cache-alert-smtp-to-0'), {
        target: { value: 'ops@example.com' },
      })
    })
    await act(async () => {
      fireEvent.click(
        document.getElementById(
          'settings-cache-alert-smtp-add-recipient',
        ) as HTMLButtonElement,
      )
    })
    expect(input('settings-cache-alert-smtp-to-1')).not.toBeNull()

    await act(async () => {
      fireEvent.change(input('settings-cache-alert-smtp-to-1'), {
        target: { value: 'oncall@example.com' },
      })
    })
    await act(async () => {
      fireEvent.click(
        document.getElementById(
          'settings-cache-alert-save',
        ) as HTMLButtonElement,
      )
    })
    await flushMicrotasks()

    const body = api.setCacheAlertSetting.mock.calls[0]?.[0]?.body
    expect(body.smtp.to).toEqual(['ops@example.com', 'oncall@example.com'])
    expect(body.smtp.enabled).toBe(true)
    expect(body.smtp.password).toBe('')
  })

  it('reports the invalid address and blocks the save', async () => {
    await loadCacheAlert(smtpStored)
    renderPage()
    await flushMicrotasks()

    await act(async () => {
      fireEvent.change(input('settings-cache-alert-smtp-to-0'), {
        target: { value: 'not-an-address' },
      })
    })

    expect(screen.getByText(copy.cacheAlertSmtpRecipientInvalid)).toBeTruthy()
    expect(
      (
        document.getElementById(
          'settings-cache-alert-save',
        ) as HTMLButtonElement | null
      )?.disabled,
    ).toBe(true)
    expect(api.setCacheAlertSetting).not.toHaveBeenCalled()
  })

  it('reports the unpaired credentials and blocks the save', async () => {
    await loadCacheAlert(smtpStored)
    renderPage()
    await flushMicrotasks()

    await act(async () => {
      fireEvent.change(input('settings-cache-alert-smtp-username'), {
        target: { value: 'other-user' },
      })
    })

    expect(screen.getByText(copy.cacheAlertSmtpCredentialsPaired)).toBeTruthy()
    expect(
      (
        document.getElementById(
          'settings-cache-alert-save',
        ) as HTMLButtonElement | null
      )?.disabled,
    ).toBe(true)
  })

  it('keeps a DingTalk-only workflow untouched', async () => {
    await loadCacheAlert(legacyStored)
    api.setCacheAlertSetting.mockResolvedValueOnce(legacyStored)
    renderPage()
    await flushMicrotasks()

    await act(async () => {
      fireEvent.change(input('settings-cache-alert-window-minutes'), {
        target: { value: '45' },
      })
    })
    await act(async () => {
      fireEvent.click(
        document.getElementById(
          'settings-cache-alert-save',
        ) as HTMLButtonElement,
      )
    })
    await flushMicrotasks()

    const body = api.setCacheAlertSetting.mock.calls[0]?.[0]?.body
    expect(Object.hasOwn(body, 'smtp')).toBe(false)
    expect(body.window_minutes).toBe(45)
  })
})
