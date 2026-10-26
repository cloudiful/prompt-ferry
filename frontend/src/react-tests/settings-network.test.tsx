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
import type { RelayIpPolicyResponse } from '@/generated/admin-api'

const api = vi.hoisted(() => ({
  getRelayIpWhitelist: vi.fn(),
  setRelayIpWhitelist: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

import { messages } from '@/i18n'
import { getPendingActions, runAsyncAction } from '@/lib/async-action'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import {
  disposeSettingsNetwork,
  networkIdentityKey,
  setSettingsNetworkNotifier,
  useSettingsNetworkStore,
} from '@/lib/settings-network'
import {
  RELAY_IP_WHITELIST_KEY,
  createRelayIpWhitelistForm,
  mergeRelayIpWhitelistResponse,
  relayPolicyToWhitelistForm,
  relayWhitelistDirtyFields,
  relayWhitelistFormToRequest,
  relayWhitelistRefreshBlocked,
} from '@/lib/settings-network-model'
import { SettingsPage } from '@/pages/SettingsPage'
import { SettingsNetworkTab } from '@/components/settings/SettingsNetworkTab'

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

const policy: RelayIpPolicyResponse = {
  allowed_cidrs: ['10.0.0.0/8', '192.168.0.0/16'],
  trusted_proxy_cidrs: ['127.0.0.1/32'],
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
  return useSettingsNetworkStore.getState()
}

async function flushMicrotasks(times = 8) {
  for (let index = 0; index < times; index += 1) {
    await act(async () => {
      await Promise.resolve()
    })
  }
}

async function loadNetwork(client = queryClient()) {
  store().initializeIdentity(admin)
  await act(async () => {
    await store().refresh(client)
  })
  return client
}

function renderPage(path = '/settings/network') {
  const client = queryClient()
  return render(
    <QueryClientProvider client={client}>
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

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  api.getRelayIpWhitelist.mockReset().mockResolvedValue(policy)
  api.setRelayIpWhitelist.mockReset()
  useSettingsNetworkStore.getState().resetIdentity(null)
  setSettingsNetworkNotifier(null)
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
  disposeSettingsNetwork()
  useSettingsNetworkStore.getState().resetIdentity(null)
})

describe('network settings pure model', () => {
  it('maps a stored policy to one entry per line and back', () => {
    expect(relayPolicyToWhitelistForm(policy)).toEqual({
      allowed_cidrs_text: '10.0.0.0/8\n192.168.0.0/16',
      trusted_proxy_cidrs_text: '127.0.0.1/32',
    })
    expect(relayPolicyToWhitelistForm(null)).toEqual(
      createRelayIpWhitelistForm(),
    )

    expect(
      relayWhitelistFormToRequest({
        allowed_cidrs_text: '  10.0.0.0/8 \n\n 10.0.0.0/8\n192.168.0.0/16  ',
        trusted_proxy_cidrs_text: '',
      }),
    ).toEqual({
      allowed_cidrs: ['10.0.0.0/8', '10.0.0.0/8', '192.168.0.0/16'],
      trusted_proxy_cidrs: [],
    })
  })

  it('applies a saved policy only to fields that still hold the submitted text', () => {
    const submitted = {
      allowed_cidrs_text: '10.0.0.0/8',
      trusted_proxy_cidrs_text: '127.0.0.1/32',
    }
    expect(
      mergeRelayIpWhitelistResponse(
        { allowed_cidrs_text: '10.0.0.0/8', trusted_proxy_cidrs_text: '  ' },
        submitted,
        { allowed_cidrs: ['10.0.0.0/8'], trusted_proxy_cidrs: [] },
      ),
    ).toEqual({
      allowed_cidrs_text: '10.0.0.0/8',
      trusted_proxy_cidrs_text: '  ',
    })

    expect(
      mergeRelayIpWhitelistResponse(submitted, submitted, {
        allowed_cidrs: ['10.0.0.0/8'],
        trusted_proxy_cidrs: ['127.0.0.1/32'],
      }),
    ).toEqual(submitted)

    expect(
      relayWhitelistDirtyFields(
        { allowed_cidrs_text: '10.0.0.0/8', trusted_proxy_cidrs_text: '' },
        { allowed_cidrs_text: '10.0.0.0/8', trusted_proxy_cidrs_text: '' },
      ),
    ).toEqual([])
    expect(
      relayWhitelistDirtyFields(
        { allowed_cidrs_text: '10.1.0.0/16', trusted_proxy_cidrs_text: '' },
        { allowed_cidrs_text: '10.0.0.0/8', trusted_proxy_cidrs_text: '' },
      ),
    ).toEqual(['allowed_cidrs_text'])
  })

  it('blocks a scoped refresh until the tab is loaded, clean, unsaved-free and idle', () => {
    const base = {
      pendingActions: {},
      dirty: false,
      failed: false,
      loading: false,
      loaded: true,
    }
    expect(relayWhitelistRefreshBlocked(base)).toBe(false)
    expect(relayWhitelistRefreshBlocked({ ...base, loaded: false })).toBe(true)
    expect(relayWhitelistRefreshBlocked({ ...base, loading: true })).toBe(true)
    expect(relayWhitelistRefreshBlocked({ ...base, dirty: true })).toBe(true)
    expect(relayWhitelistRefreshBlocked({ ...base, failed: true })).toBe(true)
    expect(
      relayWhitelistRefreshBlocked({
        ...base,
        pendingActions: { [RELAY_IP_WHITELIST_KEY]: 'settings' },
      }),
    ).toBe(true)
  })
})

describe('network settings store', () => {
  it('reads once on load and writes nothing', async () => {
    await loadNetwork()

    expect(api.getRelayIpWhitelist).toHaveBeenCalledTimes(1)
    expect(api.setRelayIpWhitelist).not.toHaveBeenCalled()
    expect(store().loaded).toBe(true)
    expect(store().form).toEqual(relayPolicyToWhitelistForm(policy))
    expect(store().loadError).toBe(false)
  })

  it('saves both arrays and reports the normalized accepted policy once', async () => {
    await loadNetwork()
    const notifySaved = vi.fn()
    setSettingsNetworkNotifier({ notifyError: vi.fn(), notifySaved })
    api.setRelayIpWhitelist.mockResolvedValueOnce({
      allowed_cidrs: ['10.0.0.0/8'],
      trusted_proxy_cidrs: [],
    })

    store().setField('allowed_cidrs_text', ' 10.0.0.0/8 \n')
    store().setField('trusted_proxy_cidrs_text', '')
    await act(async () => {
      await store().save()
    })

    expect(api.setRelayIpWhitelist).toHaveBeenCalledTimes(1)
    expect(api.setRelayIpWhitelist.mock.calls[0]?.[0]?.body).toEqual({
      allowed_cidrs: ['10.0.0.0/8'],
      trusted_proxy_cidrs: [],
    })
    expect(notifySaved).toHaveBeenCalledTimes(1)
    expect(store().form).toEqual({
      allowed_cidrs_text: '10.0.0.0/8',
      trusted_proxy_cidrs_text: '',
    })
    expect(store().saveFailed).toBe(false)
  })

  it('sends one request and one confirmation for a double save', async () => {
    await loadNetwork()
    const notifySaved = vi.fn()
    setSettingsNetworkNotifier({ notifyError: vi.fn(), notifySaved })
    const write = deferred<RelayIpPolicyResponse>()
    api.setRelayIpWhitelist.mockReturnValueOnce(write.promise)

    const first = store().save()
    await flushMicrotasks(2)
    const second = await store().save()
    expect(second).toEqual({ executed: false })

    write.resolve(policy)
    await act(async () => {
      await first
    })

    expect(api.setRelayIpWhitelist).toHaveBeenCalledTimes(1)
    expect(notifySaved).toHaveBeenCalledTimes(1)
  })

  it('keeps the draft on a failed save, reports once and retries through save', async () => {
    await loadNetwork()
    const notifyError = vi.fn()
    const notifySaved = vi.fn()
    setSettingsNetworkNotifier({ notifyError, notifySaved })
    api.setRelayIpWhitelist
      .mockRejectedValueOnce(new Error('400 bad_request'))
      .mockResolvedValueOnce({
        allowed_cidrs: ['10.0.0.0/8'],
        trusted_proxy_cidrs: [],
      })

    store().setField('allowed_cidrs_text', '999.1.1.1')
    store().setField('trusted_proxy_cidrs_text', '')
    await act(async () => {
      await store().save()
    })

    expect(notifyError).toHaveBeenCalledTimes(1)
    expect(notifySaved).not.toHaveBeenCalled()
    expect(store().saveFailed).toBe(true)
    expect(store().form.allowed_cidrs_text).toBe('999.1.1.1')
    expect(Object.hasOwn(getPendingActions(), RELAY_IP_WHITELIST_KEY)).toBe(
      false,
    )

    store().setField('allowed_cidrs_text', '10.0.0.0/8')
    await act(async () => {
      await store().save()
    })

    expect(api.setRelayIpWhitelist).toHaveBeenCalledTimes(2)
    expect(notifySaved).toHaveBeenCalledTimes(1)
    expect(store().saveFailed).toBe(false)
    expect(store().form.allowed_cidrs_text).toBe('10.0.0.0/8')
  })

  it('retains an edit typed while the save was in flight', async () => {
    await loadNetwork()
    const write = deferred<RelayIpPolicyResponse>()
    api.setRelayIpWhitelist.mockReturnValueOnce(write.promise)

    store().setField('allowed_cidrs_text', '10.0.0.0/8')
    store().setField('trusted_proxy_cidrs_text', '')
    const pending = store().save()
    await flushMicrotasks(2)
    store().setField('allowed_cidrs_text', '10.9.0.0/16')

    write.resolve({ allowed_cidrs: ['10.0.0.0/8'], trusted_proxy_cidrs: [] })
    await act(async () => {
      await pending
    })

    expect(store().form.allowed_cidrs_text).toBe('10.9.0.0/16')
    expect(relayWhitelistDirtyFields(store().form, store().baseline)).toEqual([
      'allowed_cidrs_text',
    ])
  })

  it('does not block an unrelated settings key', async () => {
    await loadNetwork()
    api.setRelayIpWhitelist.mockResolvedValueOnce(policy)
    const release: Array<() => void> = []
    const held = runAsyncAction(
      'setting:rawRetention',
      () => new Promise<void>((resolve) => release.push(resolve)),
      'settings',
    )
    await flushMicrotasks(1)

    await act(async () => {
      await store().save()
    })

    expect(api.setRelayIpWhitelist).toHaveBeenCalledTimes(1)
    release.forEach((resolve) => resolve())
    await held
  })

  it('commits only the latest read and drops a read after an edit', async () => {
    store().initializeIdentity(admin)
    const client = queryClient()
    const resolvers: Array<(value: RelayIpPolicyResponse) => void> = []
    api.getRelayIpWhitelist.mockImplementation(
      () =>
        new Promise<RelayIpPolicyResponse>((resolve) => {
          resolvers.push(resolve)
        }),
    )

    void store().refresh(client)
    void store().refresh(client)
    await flushMicrotasks(4)
    const current = resolvers.at(-1) as (value: RelayIpPolicyResponse) => void
    current({ allowed_cidrs: ['10.0.0.0/8'], trusted_proxy_cidrs: [] })
    await flushMicrotasks(8)
    expect(store().form.allowed_cidrs_text).toBe('10.0.0.0/8')

    for (const stale of resolvers.slice(0, -1)) {
      stale({ allowed_cidrs: ['172.16.0.0/12'], trusted_proxy_cidrs: [] })
    }
    await flushMicrotasks(8)
    expect(store().form.allowed_cidrs_text).toBe('10.0.0.0/8')

    const slow = deferred<RelayIpPolicyResponse>()
    api.getRelayIpWhitelist.mockReturnValueOnce(slow.promise)
    void store().refresh(client)
    await flushMicrotasks(2)
    expect(store().loading).toBe(true)
    store().setField('allowed_cidrs_text', '10.2.0.0/16')
    expect(store().loading).toBe(false)
    slow.resolve({ allowed_cidrs: ['10.0.0.0/8'], trusted_proxy_cidrs: [] })
    await flushMicrotasks(8)
    expect(store().form.allowed_cidrs_text).toBe('10.2.0.0/16')
  })

  it('drops a read that belongs to a previous identity', async () => {
    store().initializeIdentity(admin)
    const client = queryClient()
    const pending = deferred<RelayIpPolicyResponse>()
    api.getRelayIpWhitelist.mockReturnValueOnce(pending.promise)

    const read = store().refresh(client)
    store().resetIdentity(operator)
    pending.resolve(policy)
    await act(async () => {
      await read
    })

    expect(store().identityKey).toBe(networkIdentityKey(operator))
    expect(store().loaded).toBe(false)
    expect(store().form).toEqual(createRelayIpWhitelistForm())
  })

  it('drops an in-flight read on unmount without leaking the payload', async () => {
    store().initializeIdentity(admin)
    const client = queryClient()
    const pending = deferred<RelayIpPolicyResponse>()
    api.getRelayIpWhitelist.mockReturnValueOnce(pending.promise)

    const read = store().refresh(client)
    await flushMicrotasks(2)
    disposeSettingsNetwork()
    expect(store().loading).toBe(false)

    pending.resolve(policy)
    await act(async () => {
      await read
    })

    expect(store().loaded).toBe(false)
    expect(store().form).toEqual(createRelayIpWhitelistForm())
  })

  it('refuses a refresh while the form holds an unsaved draft', async () => {
    const client = await loadNetwork()
    store().setField('allowed_cidrs_text', '10.0.0.0/8')
    await act(async () => {
      await store().refresh(client)
    })
    expect(api.getRelayIpWhitelist).toHaveBeenCalledTimes(1)
    expect(store().form.allowed_cidrs_text).toBe('10.0.0.0/8')
  })

  it('drops a read that started before the save', async () => {
    const client = await loadNetwork()
    const slowRead = deferred<RelayIpPolicyResponse>()
    api.getRelayIpWhitelist.mockReturnValueOnce(slowRead.promise)
    api.setRelayIpWhitelist.mockResolvedValueOnce({
      allowed_cidrs: ['10.0.0.0/8'],
      trusted_proxy_cidrs: [],
    })

    void store().refresh(client)
    await flushMicrotasks(2)
    expect(store().loading).toBe(true)
    await act(async () => {
      await store().save()
    })
    slowRead.resolve({
      allowed_cidrs: ['172.16.0.0/12'],
      trusted_proxy_cidrs: [],
    })
    await flushMicrotasks(8)

    expect(store().form.allowed_cidrs_text).toBe('10.0.0.0/8')
  })

  it('never refetches on an unrelated rerender', async () => {
    const client = await loadNetwork()
    await act(async () => {
      await store().ensureLoaded(client)
    })
    expect(api.getRelayIpWhitelist).toHaveBeenCalledTimes(1)
  })
})

describe('network settings tab', () => {
  it('renders the two labelled textareas and the save control without autosaving', async () => {
    renderPage()
    await flushMicrotasks()

    const section = document.querySelector('[data-foundation-route-id]')
    expect(section?.getAttribute('data-foundation-route-id')).toBe(
      'R-SETTINGS-NETWORK',
    )
    expect(section?.getAttribute('data-settings-tab')).toBe('network')
    expect(
      screen.getByRole('heading', { name: copy.settingsNetwork }),
    ).toBeTruthy()

    const allowed = document.getElementById(
      'settings-allowed-cidrs',
    ) as HTMLTextAreaElement
    const trusted = document.getElementById(
      'settings-trusted-proxy-cidrs',
    ) as HTMLTextAreaElement
    expect(allowed).not.toBeNull()
    expect(trusted).not.toBeNull()
    expect(allowed.name).toBe('settings-allowed-cidrs')
    expect(trusted.name).toBe('settings-trusted-proxy-cidrs')
    expect(allowed.rows).toBe(5)
    expect(trusted.rows).toBe(5)
    expect(allowed.value).toBe('10.0.0.0/8\n192.168.0.0/16')
    expect(trusted.value).toBe('127.0.0.1/32')
    expect(screen.getByLabelText(copy.allowedCidrs)).toBe(allowed)
    expect(screen.getByLabelText(copy.trustedProxyCidrs)).toBe(trusted)
    expect(screen.getByText(copy.allowedCidrsHelp)).toBeTruthy()
    expect(screen.getByText(copy.trustedProxyCidrsHelp)).toBeTruthy()

    fireEvent.change(allowed, { target: { value: '10.0.0.0/8\n' } })
    await flushMicrotasks()
    expect(api.setRelayIpWhitelist).not.toHaveBeenCalled()

    const saveButton = document.getElementById(
      'settings-relay-ip-whitelist-save',
    ) as HTMLButtonElement
    expect(saveButton.disabled).toBe(false)
    fireEvent.click(saveButton)
    await flushMicrotasks()
    expect(api.setRelayIpWhitelist).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(screen.getByText(copy.saved)).toBeTruthy())
  })

  it('disables the controls and the save button while the initial read is pending', async () => {
    const read = deferred<RelayIpPolicyResponse>()
    api.getRelayIpWhitelist.mockReturnValueOnce(read.promise)
    renderPage()
    await flushMicrotasks()

    expect(
      (document.getElementById('settings-allowed-cidrs') as HTMLTextAreaElement)
        .disabled,
    ).toBe(true)
    expect(
      (
        document.getElementById(
          'settings-relay-ip-whitelist-save',
        ) as HTMLButtonElement
      ).disabled,
    ).toBe(true)

    read.resolve(policy)
    await waitFor(() =>
      expect(
        (
          document.getElementById(
            'settings-allowed-cidrs',
          ) as HTMLTextAreaElement
        ).disabled,
      ).toBe(false),
    )
  })

  it('offers an explicit retry when the read fails', async () => {
    api.getRelayIpWhitelist
      .mockRejectedValueOnce(new Error('503'))
      .mockResolvedValueOnce(policy)
    renderPage()
    await flushMicrotasks()

    expect(screen.getByRole('alert')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: copy.retry }))
    await flushMicrotasks()

    expect(api.getRelayIpWhitelist).toHaveBeenCalledTimes(2)
    expect(
      (document.getElementById('settings-allowed-cidrs') as HTMLTextAreaElement)
        .value,
    ).toBe('10.0.0.0/8\n192.168.0.0/16')
  })

  it('blocks the scoped refresh while the draft is unsaved', async () => {
    renderPage()
    await flushMicrotasks()

    const refreshButton = document.getElementById(
      'settings-network-refresh',
    ) as HTMLButtonElement
    expect(refreshButton.disabled).toBe(false)

    const allowed = document.getElementById(
      'settings-allowed-cidrs',
    ) as HTMLTextAreaElement
    await act(async () => {
      fireEvent.change(allowed, { target: { value: '10.0.0.0/8' } })
    })
    expect(refreshButton.disabled).toBe(true)

    api.setRelayIpWhitelist.mockResolvedValueOnce({
      allowed_cidrs: ['10.0.0.0/8'],
      trusted_proxy_cidrs: ['127.0.0.1/32'],
    })
    await act(async () => {
      fireEvent.click(
        document.getElementById(
          'settings-relay-ip-whitelist-save',
        ) as HTMLButtonElement,
      )
    })
    await waitFor(() => expect(refreshButton.disabled).toBe(false))
    const readsBefore = api.getRelayIpWhitelist.mock.calls.length
    fireEvent.click(refreshButton)
    await flushMicrotasks()
    expect(api.getRelayIpWhitelist.mock.calls.length).toBe(readsBefore + 1)
    expect(api.setRelayIpWhitelist).toHaveBeenCalledTimes(1)
  })

  it('keeps an ordinary user out of the network route', async () => {
    useSessionStore.setState({ me: operator })
    renderPage()
    await flushMicrotasks()

    expect(
      document.querySelector('[data-foundation-route-id="R-SETTINGS-NETWORK"]'),
    ).toBeNull()
    expect(api.getRelayIpWhitelist).not.toHaveBeenCalled()
  })

  it('mounts the network tab inside the page shell without a second provider', async () => {
    render(
      <QueryClientProvider client={queryClient()}>
        <LocaleProvider>
          <NotifierProvider>
            <SettingsNetworkTab />
          </NotifierProvider>
        </LocaleProvider>
      </QueryClientProvider>,
    )
    await flushMicrotasks()

    // The tab is a card body; the read is owned by the page, so a bare tab
    // renders its empty controls rather than issuing its own request.
    expect(api.getRelayIpWhitelist).not.toHaveBeenCalled()
    expect(document.getElementById('settings-allowed-cidrs')).not.toBeNull()
  })
})
