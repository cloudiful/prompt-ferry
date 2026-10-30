import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  ManagedRelay,
  ManagedRelayListResponse,
  ManagedRelayRequest,
  ManagedRelaySecretPatch,
} from '@/generated/admin-api'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { RelaysPage } from '@/pages/RelaysPage'
import { AppShell } from '@/layouts/AppShell'
import { MemoryRouter, Route, Routes } from 'react-router'

const api = vi.hoisted(() => ({
  createRelay: vi.fn(),
  deleteRelay: vi.fn(),
  listRelays: vi.fn(),
  reconnectRelay: vi.fn(),
  updateRelay: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

function managedRelay(
  index: number,
  overrides: Partial<ManagedRelay> = {},
): ManagedRelay {
  return {
    bridge_encryption_mode: index === 1 || index % 2 === 0 ? 'required' : 'off',
    connected: index === 1,
    created_at: '2026-10-20T12:00:00Z',
    enabled: index !== 2,
    has_bridge_key: index === 1 || index % 2 === 0,
    has_client_cert: index === 1,
    has_client_key: index === 1,
    has_relay_ca: index === 1,
    last_connected_at: index === 1 ? '2026-10-24T12:00:00Z' : null,
    last_disconnected_at: index === 1 ? null : '2026-10-23T12:00:00Z',
    last_error: index === 1 ? null : 'fixture relay disconnected',
    last_snapshot_version: index === 1 ? 12 : null,
    name: `Relay ${index}`,
    relay_id: `relay-${index}`,
    relay_url: `${index === 2 ? 'ws' : 'wss'}://relay-${index}.example.test/bridge`,
    tls_mode: index === 1 ? 'mtls' : index === 2 ? 'off' : 'server',
    updated_at: '2026-10-24T12:00:00Z',
    ...overrides,
  }
}

function listResponse(
  relays: ManagedRelay[],
  first = 0,
  rows = 10,
): ManagedRelayListResponse {
  return {
    connected_count: relays.filter((relay) => relay.connected).length,
    enabled_count: relays.filter((relay) => relay.enabled).length,
    first,
    relays: relays.slice(first, first + rows),
    rows,
    total: relays.length,
  }
}

function applySecretPatch(
  exists: boolean,
  patch: ManagedRelaySecretPatch | null | undefined,
): boolean {
  if (!patch || patch.mode === 'keep') return exists
  return patch.mode === 'replace' && Boolean(patch.value.trim())
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (cause: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, reject, resolve }
}

let relays: ManagedRelay[]
let failNextList: boolean
let pendingList: ReturnType<typeof deferred<ManagedRelayListResponse>> | null

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { gcTime: 0, retry: false } },
  })
  const view = render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <NotifierProvider>
          <MemoryRouter initialEntries={['/relays']}>
            <Routes>
              <Route element={<AppShell />}>
                <Route path="/relays" element={<RelaysPage />} />
              </Route>
            </Routes>
          </MemoryRouter>
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
  return { ...view, queryClient }
}

async function chooseOption(triggerName: string, optionName: string) {
  fireEvent.click(screen.getByRole('combobox', { name: triggerName }))
  fireEvent.click(await screen.findByRole('option', { name: optionName }))
}

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn()
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  relays = Array.from({ length: 12 }, (_, index) => managedRelay(index + 1))
  failNextList = false
  pendingList = null

  api.listRelays.mockImplementation(
    async ({ query }: { query: { first: number; rows: number } }) => {
      if (pendingList) return pendingList.promise
      if (failNextList) {
        failNextList = false
        throw new Error('fixture list refresh failed')
      }
      return listResponse(relays, query.first, query.rows)
    },
  )
  api.createRelay.mockImplementation(
    async ({ body }: { body: ManagedRelayRequest }) => {
      const relay = managedRelay(relays.length + 1, {
        bridge_encryption_mode: body.bridge_encryption_mode,
        connected: false,
        enabled: body.enabled !== false,
        has_bridge_key: body.bridge_encryption_key?.mode === 'replace',
        has_client_cert: body.client_cert_pem?.mode === 'replace',
        has_client_key: body.client_key_pem?.mode === 'replace',
        has_relay_ca: body.relay_ca_pem?.mode === 'replace',
        name: body.name,
        relay_id: `created-${relays.length + 1}`,
        relay_url: body.relay_url,
        tls_mode: body.tls_mode,
      })
      relays = [...relays, relay]
      return relay
    },
  )
  api.updateRelay.mockImplementation(
    async ({
      path,
      body,
    }: {
      path: { relay_id: string }
      body: Partial<ManagedRelayRequest>
    }) => {
      const current = relays.find((relay) => relay.relay_id === path.relay_id)
      if (!current) throw new Error('missing relay fixture')
      const updated = managedRelay(1, {
        ...current,
        bridge_encryption_mode:
          body.bridge_encryption_mode ?? current.bridge_encryption_mode,
        enabled: body.enabled ?? current.enabled,
        has_bridge_key: applySecretPatch(
          current.has_bridge_key,
          body.bridge_encryption_key,
        ),
        has_client_cert: applySecretPatch(
          current.has_client_cert,
          body.client_cert_pem,
        ),
        has_client_key: applySecretPatch(
          current.has_client_key,
          body.client_key_pem,
        ),
        has_relay_ca: applySecretPatch(current.has_relay_ca, body.relay_ca_pem),
        name: body.name ?? current.name,
        relay_url: body.relay_url ?? current.relay_url,
        tls_mode: body.tls_mode ?? current.tls_mode,
      })
      relays = relays.map((relay) =>
        relay.relay_id === path.relay_id ? updated : relay,
      )
      return updated
    },
  )
  api.deleteRelay.mockImplementation(
    async ({ path }: { path: { relay_id: string } }) => {
      relays = relays.filter((relay) => relay.relay_id !== path.relay_id)
      return undefined
    },
  )
  api.reconnectRelay.mockImplementation(
    async ({ path }: { path: { relay_id: string } }) => {
      const relay = relays.find((item) => item.relay_id === path.relay_id)
      if (!relay) throw new Error('missing relay fixture')
      if (!relay.enabled) throw new Error('relay is disabled')
      const reconnected = { ...relay, connected: true, last_error: null }
      relays = relays.map((item) =>
        item.relay_id === path.relay_id ? reconnected : item,
      )
      return reconnected
    },
  )
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('React Relay management page', () => {
  it('renders the summary, raw modes, secret flags, pagination, and stored page size', async () => {
    renderPage()

    expect(await screen.findByText('Relay 1')).toBeTruthy()
    expect(
      screen.getByRole('button', { name: 'New relay' }).closest('header'),
    ).toBeTruthy()
    expect(
      screen.getByRole('button', { name: 'Refresh' }).closest('header'),
    ).toBeTruthy()
    expect(screen.getByText('1/11').closest('header')).toBeTruthy()
    expect(screen.getByText('mtls')).toBeTruthy()
    expect(screen.getAllByText('required').length).toBeGreaterThan(0)
    expect(screen.getByText('Snapshot 12')).toBeTruthy()
    expect(screen.getByText('CA: stored')).toBeTruthy()
    expect(screen.getByText('Key: stored')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Next page' }))
    expect(await screen.findByText('Relay 11')).toBeTruthy()
    expect(api.listRelays).toHaveBeenLastCalledWith({
      query: { first: 10, rows: 10 },
      responseStyle: 'data',
    })

    fireEvent.click(screen.getByRole('combobox', { name: 'Rows per page' }))
    fireEvent.click(await screen.findByRole('option', { name: '20' }))
    expect(await screen.findByText('Relay 12')).toBeTruthy()
    await waitFor(() =>
      expect(localStorage.getItem('prompt-ferry:table-page-size:relays')).toBe(
        '20',
      ),
    )
  })

  it('keeps hidden conditional secret values when mode fields are toggled', async () => {
    renderPage()
    await screen.findByText('Relay 1')
    fireEvent.click(screen.getByRole('button', { name: 'Edit Relay 1' }))

    const cert = screen.getByLabelText('Client cert PEM') as HTMLTextAreaElement
    const bridge = screen.getByLabelText('Bridge key') as HTMLTextAreaElement
    fireEvent.change(cert, { target: { value: 'synthetic cert fixture' } })
    fireEvent.change(bridge, { target: { value: 'synthetic bridge fixture' } })

    await chooseOption('TLS mode', 'Off')
    expect(screen.queryByLabelText('Client cert PEM')).toBeNull()
    expect(screen.queryByLabelText('Client key PEM')).toBeNull()
    await chooseOption('Bridge encryption', 'Off')
    expect(screen.queryByLabelText('Bridge key')).toBeNull()

    await chooseOption('TLS mode', 'mTLS')
    await chooseOption('Bridge encryption', 'Required')
    expect(
      (screen.getByLabelText('Client cert PEM') as HTMLTextAreaElement).value,
    ).toBe('synthetic cert fixture')
    expect(
      (screen.getByLabelText('Bridge key') as HTMLTextAreaElement).value,
    ).toBe('synthetic bridge fixture')
  })

  it('creates a relay without secret selectors and identifies it by name after widening the page', async () => {
    renderPage()
    await screen.findByText('Relay 1')
    fireEvent.click(screen.getByRole('button', { name: 'New relay' }))

    expect(
      screen.queryByRole('combobox', { name: 'Relay CA PEM action' }),
    ).toBeNull()
    fireEvent.change(screen.getByLabelText('Name'), {
      target: { value: '  New fixture relay  ' },
    })
    fireEvent.change(screen.getByLabelText('Relay URL'), {
      target: { value: ' ws://new-relay.example.test/bridge/ ' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(api.createRelay).toHaveBeenCalledTimes(1))
    expect(api.createRelay.mock.calls[0][0].body).toMatchObject({
      bridge_encryption_mode: 'off',
      name: 'New fixture relay',
      relay_url: 'ws://new-relay.example.test/bridge/',
      tls_mode: 'off',
    })
    expect(api.createRelay.mock.calls[0][0].body.relay_ca_pem).toBeUndefined()
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())

    fireEvent.click(screen.getByRole('combobox', { name: 'Rows per page' }))
    fireEvent.click(await screen.findByRole('option', { name: '20' }))
    expect(await screen.findByText('New fixture relay')).toBeTruthy()
  })

  it('sends all four independent edit patches and clears regardless of textarea text', async () => {
    renderPage()
    await screen.findByText('Relay 1')
    fireEvent.click(screen.getByRole('button', { name: 'Edit Relay 1' }))

    fireEvent.change(screen.getByLabelText('Relay CA PEM'), {
      target: { value: 'ignored CA value' },
    })
    await chooseOption('Relay CA PEM action', 'Clear')
    fireEvent.change(screen.getByLabelText('Client cert PEM'), {
      target: { value: '  replacement certificate  ' },
    })
    await chooseOption('Client key PEM action', 'Replace')
    await chooseOption('Bridge key action', 'Clear')

    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(api.updateRelay).toHaveBeenCalledTimes(1))

    const body = api.updateRelay.mock.calls[0][0].body
    expect(body).toMatchObject({
      bridge_encryption_key: { mode: 'clear' },
      client_cert_pem: { mode: 'replace', value: 'replacement certificate' },
      client_key_pem: { mode: 'keep' },
      relay_ca_pem: { mode: 'clear' },
    })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())

    fireEvent.click(screen.getByRole('button', { name: 'Edit Relay 1' }))
    expect(
      screen.getByRole('combobox', { name: 'Relay CA PEM action' }).textContent,
    ).toBe('Replace')
  })

  it('retains the draft and reports failure when a successful create cannot refresh the list', async () => {
    renderPage()
    await screen.findByText('Relay 1')
    fireEvent.click(screen.getByRole('button', { name: 'New relay' }))
    fireEvent.change(screen.getByLabelText('Name'), {
      target: { value: 'Retry fixture relay' },
    })
    fireEvent.change(screen.getByLabelText('Relay URL'), {
      target: { value: 'ws://retry.example.test/bridge' },
    })
    failNextList = true
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(api.createRelay).toHaveBeenCalledTimes(1))
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect((screen.getByLabelText('Name') as HTMLInputElement).value).toBe(
      'Retry fixture relay',
    )
    expect(
      screen.getByRole('button', { name: 'Save' }).hasAttribute('disabled'),
    ).toBe(false)
    expect(screen.queryByText('Saved')).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(api.createRelay).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })

  it('holds the create guard through refresh and freezes its dialog', async () => {
    renderPage()
    await screen.findByText('Relay 1')
    fireEvent.click(screen.getByRole('button', { name: 'New relay' }))
    fireEvent.change(screen.getByLabelText('Name'), {
      target: { value: 'Guarded fixture relay' },
    })
    fireEvent.change(screen.getByLabelText('Relay URL'), {
      target: { value: 'ws://guarded.example.test/bridge' },
    })
    pendingList = deferred<ManagedRelayListResponse>()
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(api.createRelay).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(api.listRelays).toHaveBeenCalledTimes(2))
    expect(
      screen
        .getByRole('button', { name: 'New relay', hidden: true })
        .hasAttribute('disabled'),
    ).toBe(true)
    expect(
      screen.getByRole('button', { name: 'Save' }).hasAttribute('disabled'),
    ).toBe(true)
    expect(
      screen.getByRole('button', { name: 'Cancel' }).hasAttribute('disabled'),
    ).toBe(true)

    fireEvent.keyDown(document, { key: 'Escape' })
    expect(screen.getByRole('dialog')).toBeTruthy()

    const deferredList = pendingList
    if (!deferredList) throw new Error('missing pending list')
    await act(async () => {
      deferredList.resolve(listResponse(relays, 0, 10))
      await deferredList.promise
    })

    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })

  it('shares a row guard across mutations while leaving another row operable', async () => {
    renderPage()
    await screen.findByText('Relay 1')
    pendingList = deferred<ManagedRelayListResponse>()

    fireEvent.click(screen.getByRole('button', { name: 'Reconnect Relay 1' }))
    await waitFor(() => expect(api.reconnectRelay).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(api.listRelays).toHaveBeenCalledTimes(2))

    expect(
      screen
        .getByRole('button', { name: 'Edit Relay 1' })
        .hasAttribute('disabled'),
    ).toBe(true)
    expect(
      screen
        .getByRole('button', { name: 'Delete Relay 1' })
        .hasAttribute('disabled'),
    ).toBe(true)
    expect(
      screen
        .getByRole('button', { name: 'Reconnect Relay 3' })
        .hasAttribute('disabled'),
    ).toBe(false)

    fireEvent.click(screen.getByRole('button', { name: 'Reconnect Relay 3' }))
    await waitFor(() => expect(api.reconnectRelay).toHaveBeenCalledTimes(2))

    const deferredList = pendingList
    if (!deferredList) throw new Error('missing pending list')
    await act(async () => {
      deferredList.resolve(listResponse(relays, 0, 10))
      await deferredList.promise
    })

    await waitFor(() =>
      expect(
        screen
          .getByRole('button', { name: 'Reconnect Relay 1' })
          .hasAttribute('disabled'),
      ).toBe(false),
    )
    expect(
      screen
        .getByRole('button', { name: 'Reconnect Relay 3' })
        .hasAttribute('disabled'),
    ).toBe(false)
  })

  it('does not call disabled reconnect, cancels deletion without a request, and deletes after confirmation', async () => {
    const confirm = vi.spyOn(window, 'confirm').mockReturnValue(false)
    renderPage()
    await screen.findByText('Relay 1')

    expect(
      screen
        .getByRole('button', { name: 'Reconnect Relay 2' })
        .hasAttribute('disabled'),
    ).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect Relay 2' }))
    expect(api.reconnectRelay).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'Delete Relay 1' }))
    expect(confirm).toHaveBeenCalledWith('Delete relay: Relay 1?')
    expect(api.deleteRelay).not.toHaveBeenCalled()

    confirm.mockReturnValue(true)
    fireEvent.click(screen.getByRole('button', { name: 'Delete Relay 1' }))
    await waitFor(() => expect(api.deleteRelay).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(screen.queryByText('Relay 1')).toBeNull())
  })

  it('corrects to the preceding page after deleting the final listed row', async () => {
    relays = relays.slice(0, 11)
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    renderPage()
    await screen.findByText('Relay 1')
    fireEvent.click(screen.getByRole('button', { name: 'Next page' }))
    await screen.findByText('Relay 11')
    fireEvent.click(screen.getByRole('button', { name: 'Delete Relay 11' }))

    await waitFor(() => expect(api.deleteRelay).toHaveBeenCalledTimes(1))
    expect(await screen.findByText('Relay 10')).toBeTruthy()
    expect(screen.queryByText('Relay 11')).toBeNull()
    expect(api.listRelays.mock.calls.at(-1)?.[0].query).toEqual({
      first: 0,
      rows: 10,
    })
  })
})
