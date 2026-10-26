//! Cross-identity isolation probe for the shared-SPA query cache.
//!
//! The application creates one query client above routing, so the client — and
//! everything it caches — survives a logout and the next login inside the same
//! SPA. The API-key page keys its own reads by a self-scope sentinel that is
//! the same for every identity, so the question this file answers is concrete:
//! after user A logs out and user B logs in without a reload, can B see A's
//! keys, A's key secret, or A's secret in B's clipboard while B's own read is
//! still pending or has failed?
//!
//! The probe drives the real session lifecycle — the real logout button in the
//! real app shell, the real login form, the real auth gate, the real API-key
//! page — over one persistent query client built by the application's own
//! client factory with the application's own default cache settings. Only the
//! generated SDK's network functions are replaced; the page, the session
//! store, the routing, the providers and the cache are the application's own.
//!
//! A failing case here is the finding, not a broken test: the file asserts the
//! isolation the product needs and reports the exact leak instead of hiding it.

import {
  QueryClient,
  QueryClientProvider,
  useQueryClient,
} from '@tanstack/react-query'
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { useEffect } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  ClientKey,
  ClientKeyPageResponse,
  MeResponse,
} from '@/generated/admin-api'
import {
  authLogin,
  authLogout,
  authMe,
  bridgeStatus,
  meListClientKeys,
} from '@/generated/admin-api'
import { AppShell } from '@/layouts/AppShell'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { createAppQueryClient } from '@/lib/query'
import { WorkerAuthGate } from '@/lib/router'
import { useSessionStore } from '@/lib/session'
import { ThemeProvider } from '@/lib/theme'
import { ApiKeysPage } from '@/pages/ApiKeysPage'
import { LoginPage } from '@/pages/LoginPage'

vi.mock('@/generated/admin-api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/generated/admin-api')>()
  return {
    ...actual,
    authLogin: vi.fn(),
    authLogout: vi.fn(),
    authMe: vi.fn(),
    bridgeStatus: vi.fn(),
    meListClientKeys: vi.fn(),
  }
})

const sdkMocks = {
  authLogin: vi.mocked(authLogin) as unknown as ReturnType<typeof vi.fn>,
  authLogout: vi.mocked(authLogout) as unknown as ReturnType<typeof vi.fn>,
  authMe: vi.mocked(authMe) as unknown as ReturnType<typeof vi.fn>,
  bridgeStatus: vi.mocked(bridgeStatus) as unknown as ReturnType<typeof vi.fn>,
  meListClientKeys: vi.mocked(meListClientKeys) as unknown as ReturnType<
    typeof vi.fn
  >,
}

const USER_A: MeResponse = {
  display_name: 'Probe User A',
  is_admin: false,
  login_name: 'probe-user-a',
  user_id: 101,
}

const USER_B: MeResponse = {
  display_name: 'Probe User B',
  is_admin: false,
  login_name: 'probe-user-b',
  user_id: 202,
}

const A_KEY_ID = 'probe-key-user-a'
const B_KEY_ID = 'probe-key-user-b'
const A_LABEL = 'probe-user-a-key'
const B_LABEL = 'probe-user-b-key'
const A_SECRET = 'pf_probe_secret_user_a_9f2c'
const B_SECRET = 'pf_probe_secret_user_b_4d71'

const writeText = vi.fn().mockResolvedValue(undefined)

/**
 * The server-side session the mocked SDK answers from.
 *
 * Logging out ends it and logging in starts a new one, which is what makes
 * `authMe` answer with an unauthorized error after the logout and with the
 * new identity after the login.
 */
let serverSession: MeResponse | null = null

function makeKey(
  keyId: string,
  label: string,
  userId: number,
  secret: string,
): ClientKey {
  return {
    created_at: '2026-10-26T09:00:00.000Z',
    enabled: true,
    key_id: keyId,
    key_prefix: 'pf_probe_',
    label,
    last_used_at: null,
    secret,
    user_id: userId,
  }
}

function keyPage(keys: ClientKey[]): ClientKeyPageResponse {
  return { first: 0, keys, rows: 10, total: keys.length }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (cause: unknown) => void
  let settled = false
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = (value) => {
      settled = true
      resolvePromise(value)
    }
    reject = (cause) => {
      settled = true
      rejectPromise(cause)
    }
  })
  return {
    get settled() {
      return settled
    },
    promise,
    reject,
    resolve,
  }
}

/**
 * Records the query client of every session the tree renders, so a test can
 * look at the client that is live now and at the ones that were retired.
 */
function QueryClientSpy({ clients }: { clients: QueryClient[] }) {
  const client = useQueryClient()
  useEffect(() => {
    clients.push(client)
  }, [client, clients])
  return null
}

/**
 * The application's own providers and route structure, over one query client
 * that is created once and kept for the whole case, exactly as the shell
 * keeps it across a logout and a login.
 */
function renderApp(options?: { clients?: QueryClient[] }) {
  const queryClient = createAppQueryClient()
  const clients = options?.clients
  const routeElement = clients ? (
    <>
      <ApiKeysPage />
      <QueryClientSpy clients={clients} />
    </>
  ) : (
    <ApiKeysPage />
  )
  const view = render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <ThemeProvider>
          <NotifierProvider>
            <MemoryRouter initialEntries={['/api-keys']}>
              <Routes>
                <Route element={<LoginPage />} path="/login" />
                <Route element={<WorkerAuthGate />}>
                  <Route element={<AppShell />}>
                    <Route element={routeElement} path="/api-keys" />
                  </Route>
                </Route>
              </Routes>
            </MemoryRouter>
          </NotifierProvider>
        </ThemeProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
  return { ...view, queryClient }
}

function userARows(): Element[] {
  return Array.from(document.querySelectorAll(`[data-key-row="${A_KEY_ID}"]`))
}

function userBRows(): Element[] {
  return Array.from(document.querySelectorAll(`[data-key-row="${B_KEY_ID}"]`))
}

/**
 * Confirm that user A's own view really holds the secret and that its reveal
 * and copy controls work, so a later absence cannot be mistaken for a
 * fixture that never carried one.
 */
async function confirmUserARead(): Promise<void> {
  const rows = userARows()
  expect(rows.length).toBeGreaterThan(0)
  const row = within(rows[0] as HTMLElement)
  fireEvent.click(row.getByRole('button', { name: 'Show secret' }))
  expect(screen.queryAllByText(A_SECRET).length).toBeGreaterThan(0)
  fireEvent.click(row.getByRole('button', { name: 'Copy' }))
  await waitFor(() => expect(writeText).toHaveBeenCalledWith(A_SECRET))
  writeText.mockClear()
}

/**
 * Leave user A through the shell's own logout button and come back through
 * the login form, without replacing the query client.
 */
async function logoutAndLoginAs(loginName: string): Promise<void> {
  fireEvent.click(screen.getAllByRole('button', { name: 'Logout' })[0])
  await waitFor(() => expect(sdkMocks.authLogout).toHaveBeenCalledTimes(1))
  fireEvent.change(await screen.findByLabelText('Login name'), {
    target: { value: loginName },
  })
  fireEvent.change(screen.getByLabelText('Password'), {
    target: { value: 'probe-synthetic-password' },
  })
  fireEvent.click(screen.getByRole('button', { name: 'Login' }))
}

async function logoutAndLoginAsUserB(): Promise<void> {
  await logoutAndLoginAs(USER_B.login_name)
}

/**
 * Whether a client's cache holds a value anywhere.
 */
function cacheHolds(client: QueryClient | undefined, value: string): boolean {
  return (client?.getQueryCache().getAll() ?? []).some((query) =>
    JSON.stringify(query.state.data ?? null).includes(value),
  )
}

/**
 * What user B can reach, read out of the rendered page.
 *
 * The reveal and the copy are attempted on user A's own row when it is there,
 * because "A's row is visible" and "A's secret can be copied" are different
 * findings and the second one is the serious one.
 */
async function observeUserBView(): Promise<Record<string, unknown>> {
  const aRows = userARows()
  let aRevealUsed = false
  let aCopyUsed = false
  if (aRows.length > 0) {
    const row = within(aRows[0] as HTMLElement)
    const reveal = row.queryByRole('button', { name: 'Show secret' })
    if (reveal) {
      fireEvent.click(reveal)
      aRevealUsed = true
    }
    const copy = row.queryByRole('button', { name: 'Copy' })
    if (copy) {
      fireEvent.click(copy)
      aCopyUsed = true
    }
  }
  await waitFor(() =>
    expect(writeText.mock.calls.length).toBe(aCopyUsed ? 1 : 0),
  )
  const body = document.body.textContent ?? ''
  return {
    a_copy_control_used: aCopyUsed,
    a_key_rows_in_dom: aRows.length,
    a_label_inputs: screen.queryAllByDisplayValue(A_LABEL).length,
    a_reveal_control_used: aRevealUsed,
    a_secret_revealed_in_dom: body.includes(A_SECRET),
    alerts: screen
      .queryAllByRole('alert')
      .map((node) => node.textContent ?? ''),
    b_key_rows_in_dom: userBRows().length,
    b_label_inputs: screen.queryAllByDisplayValue(B_LABEL).length,
    clipboard_values: writeText.mock.calls.map((call) => String(call[0])),
    self_read_calls: sdkMocks.meListClientKeys.mock.calls.length,
  }
}

function report(observation: Record<string, unknown>): string {
  return `user B must not reach user A's keys. observation=${JSON.stringify(observation)}`
}

beforeEach(() => {
  Element.prototype.scrollIntoView = () => undefined
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText },
  })
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
    error: '',
  })
  serverSession = USER_A
  sdkMocks.authMe.mockImplementation(async () => {
    if (!serverSession) {
      throw { error: { code: 'unauthorized', message: 'unauthorized' } }
    }
    return { data: serverSession }
  })
  sdkMocks.authLogout.mockImplementation(async () => {
    serverSession = null
    return { data: undefined }
  })
  sdkMocks.authLogin.mockImplementation(
    async (options: { body?: { login_name?: string } }) => {
      serverSession =
        options?.body?.login_name === USER_A.login_name ? USER_A : USER_B
      return { data: undefined }
    },
  )
  sdkMocks.bridgeStatus.mockResolvedValue({
    data: {
      configured_relays: 0,
      connected_relays: 0,
      relays: [],
      snapshot_version: 0,
    },
  })
})

afterEach(() => {
  localStorage.clear()
  serverSession = null
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
    error: '',
  })
})

describe('API key identity isolation across a shared session', () => {
  it('keeps user A out of user B view while B own read is pending', async () => {
    const bRead = deferred<{ data: ClientKeyPageResponse }>()
    sdkMocks.meListClientKeys
      .mockResolvedValueOnce({
        data: keyPage([makeKey(A_KEY_ID, A_LABEL, USER_A.user_id, A_SECRET)]),
      })
      .mockReturnValueOnce(bRead.promise)

    renderApp()
    await screen.findAllByDisplayValue(A_LABEL)
    await confirmUserARead()

    await logoutAndLoginAsUserB()
    await waitFor(() =>
      expect(sdkMocks.meListClientKeys).toHaveBeenCalledTimes(2),
    )
    expect(bRead.settled).toBe(false)

    const observation = await observeUserBView()
    expect(observation.a_key_rows_in_dom, report(observation)).toBe(0)
    expect(observation.a_label_inputs, report(observation)).toBe(0)
    expect(observation.a_secret_revealed_in_dom, report(observation)).toBe(
      false,
    )
    expect(observation.clipboard_values, report(observation)).not.toContain(
      A_SECRET,
    )
  })

  it('keeps user A out of user B view when B own read fails', async () => {
    sdkMocks.meListClientKeys
      .mockResolvedValueOnce({
        data: keyPage([makeKey(A_KEY_ID, A_LABEL, USER_A.user_id, A_SECRET)]),
      })
      .mockRejectedValueOnce({
        error: {
          code: 'probe_self_read_failed',
          message: 'probe self read failed',
        },
      })
    sdkMocks.meListClientKeys.mockResolvedValue({
      data: keyPage([makeKey(B_KEY_ID, B_LABEL, USER_B.user_id, B_SECRET)]),
    })

    renderApp()
    await screen.findAllByDisplayValue(A_LABEL)
    await confirmUserARead()

    await logoutAndLoginAsUserB()
    await waitFor(() =>
      expect(sdkMocks.meListClientKeys).toHaveBeenCalledTimes(2),
    )
    const alerts = await screen.findAllByRole('alert')
    expect(alerts.length).toBeGreaterThan(0)
    expect(
      screen.getAllByRole('button', { name: 'Refresh' }).length,
    ).toBeGreaterThan(0)

    const observation = await observeUserBView()
    expect(observation.a_key_rows_in_dom, report(observation)).toBe(0)
    expect(observation.a_label_inputs, report(observation)).toBe(0)
    expect(observation.a_secret_revealed_in_dom, report(observation)).toBe(
      false,
    )
    expect(observation.clipboard_values, report(observation)).not.toContain(
      A_SECRET,
    )

    fireEvent.click(screen.getAllByRole('button', { name: 'Refresh' })[0])
    await screen.findAllByDisplayValue(B_LABEL)
    const retried = await observeUserBView()
    expect(retried.a_key_rows_in_dom, report(retried)).toBe(0)
    expect(retried.b_key_rows_in_dom, report(retried)).toBeGreaterThan(0)
    expect(retried.clipboard_values, report(retried)).not.toContain(A_SECRET)
  })

  it('shows only user B own keys once B own read resolves', async () => {
    sdkMocks.meListClientKeys
      .mockResolvedValueOnce({
        data: keyPage([makeKey(A_KEY_ID, A_LABEL, USER_A.user_id, A_SECRET)]),
      })
      .mockResolvedValueOnce({
        data: keyPage([makeKey(B_KEY_ID, B_LABEL, USER_B.user_id, B_SECRET)]),
      })

    renderApp()
    await screen.findAllByDisplayValue(A_LABEL)
    await confirmUserARead()

    await logoutAndLoginAsUserB()
    await waitFor(() =>
      expect(sdkMocks.meListClientKeys).toHaveBeenCalledTimes(2),
    )
    await screen.findAllByDisplayValue(B_LABEL)

    const observation = await observeUserBView()
    expect(observation.a_key_rows_in_dom, report(observation)).toBe(0)
    expect(observation.a_label_inputs, report(observation)).toBe(0)
    expect(observation.a_secret_revealed_in_dom, report(observation)).toBe(
      false,
    )
    expect(observation.b_key_rows_in_dom, report(observation)).toBeGreaterThan(
      0,
    )
    expect(observation.clipboard_values, report(observation)).not.toContain(
      A_SECRET,
    )
  })

  it('keeps user A late read out of user B cache and view', async () => {
    const clients: QueryClient[] = []
    const aRead = deferred<{ data: ClientKeyPageResponse }>()
    sdkMocks.meListClientKeys
      .mockReturnValueOnce(aRead.promise)
      .mockResolvedValueOnce({
        data: keyPage([makeKey(B_KEY_ID, B_LABEL, USER_B.user_id, B_SECRET)]),
      })

    renderApp({ clients })
    await waitFor(() =>
      expect(sdkMocks.meListClientKeys).toHaveBeenCalledTimes(1),
    )
    await waitFor(() => expect(clients.length).toBe(1))

    await logoutAndLoginAsUserB()
    await waitFor(() =>
      expect(sdkMocks.meListClientKeys).toHaveBeenCalledTimes(2),
    )
    await screen.findAllByDisplayValue(B_LABEL)
    await waitFor(() => expect(clients.length).toBe(2))

    // User A's read only now settles, long after user B signed in.
    aRead.resolve({
      data: keyPage([makeKey(A_KEY_ID, A_LABEL, USER_A.user_id, A_SECRET)]),
    })
    await waitFor(() => expect(aRead.settled).toBe(true))

    const observation = await observeUserBView()
    expect(observation.a_key_rows_in_dom, report(observation)).toBe(0)
    expect(observation.a_label_inputs, report(observation)).toBe(0)
    expect(observation.a_secret_revealed_in_dom, report(observation)).toBe(
      false,
    )
    expect(observation.clipboard_values, report(observation)).not.toContain(
      A_SECRET,
    )
    expect(cacheHolds(clients.at(-1), A_SECRET)).toBe(false)
    expect(cacheHolds(clients.at(-1), A_LABEL)).toBe(false)
    expect(clients[0]?.getQueryCache().getAll()).toHaveLength(0)
  })

  it('starts a new session for the same user without the previous keys', async () => {
    const clients: QueryClient[] = []
    const secondRead = deferred<{ data: ClientKeyPageResponse }>()
    sdkMocks.meListClientKeys
      .mockResolvedValueOnce({
        data: keyPage([makeKey(A_KEY_ID, A_LABEL, USER_A.user_id, A_SECRET)]),
      })
      .mockReturnValueOnce(secondRead.promise)

    renderApp({ clients })
    await screen.findAllByDisplayValue(A_LABEL)
    await confirmUserARead()
    await waitFor(() => expect(clients.length).toBe(1))

    await logoutAndLoginAs(USER_A.login_name)
    await waitFor(() =>
      expect(sdkMocks.meListClientKeys).toHaveBeenCalledTimes(2),
    )
    await waitFor(() => expect(clients.length).toBe(2))

    // The new session's own read is pending, so the previous session's rows
    // and secret must not be on the page.
    expect(screen.queryAllByDisplayValue(A_LABEL)).toHaveLength(0)
    expect(screen.queryAllByText(A_SECRET)).toHaveLength(0)
    expect(document.body.textContent ?? '').not.toContain(A_SECRET)
    expect(cacheHolds(clients.at(-1), A_SECRET)).toBe(false)

    secondRead.reject({
      error: {
        code: 'probe_self_read_failed',
        message: 'probe self read failed',
      },
    })
    await screen.findAllByRole('alert')
    expect(screen.queryAllByDisplayValue(A_LABEL)).toHaveLength(0)
    expect(document.body.textContent ?? '').not.toContain(A_SECRET)
    expect(cacheHolds(clients.at(-1), A_SECRET)).toBe(false)
    expect(clients[0]?.getQueryCache().getAll()).toHaveLength(0)
  })
})
