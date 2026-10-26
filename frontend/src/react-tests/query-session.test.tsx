//! Session boundaries of the Worker query client.
//!
//! The application root keeps one query client for the login page and for the
//! relay host, which are not bound to a Worker identity. Everything behind the
//! Worker auth gate is private to one authenticated session: a new principal, a
//! new role, a new login of the same principal and the end of a session each
//! start a new session epoch with a new client of their own.
//!
//! These cases exercise the real provider, the real gate and the real session
//! store. Nothing about the boundary is mocked: the client that a session uses
//! is the one the application gives it.

import {
  QueryClient,
  QueryClientProvider,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { useEffect } from 'react'
import { MemoryRouter, Outlet, Route, Routes, useNavigate } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { ClientKeyPageResponse, MeResponse } from '@/generated/admin-api'
import {
  authLogin,
  authLogout,
  authMe,
  bridgeStatus,
  meListClientKeys,
  relayAuthLogout,
} from '@/generated/admin-api'
import { AppShell } from '@/layouts/AppShell'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { createAppQueryClient } from '@/lib/query'
import { AdminGuard, WorkerAuthGate } from '@/lib/router'
import { useRelaySessionStore, useSessionStore } from '@/lib/session'
import { ThemeProvider } from '@/lib/theme'
import { ApiKeysPage } from '@/pages/ApiKeysPage'

vi.mock('@/generated/admin-api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/generated/admin-api')>()
  return {
    ...actual,
    authLogin: vi.fn(),
    authLogout: vi.fn(),
    authMe: vi.fn(),
    bridgeStatus: vi.fn(),
    meListClientKeys: vi.fn(),
    relayAuthLogout: vi.fn(),
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
  relayAuthLogout: vi.mocked(relayAuthLogout) as unknown as ReturnType<
    typeof vi.fn
  >,
}

const ADMIN: MeResponse = {
  display_name: 'Probe Administrator',
  is_admin: true,
  login_name: 'probe-admin',
  user_id: 7,
}

const OPERATOR: MeResponse = {
  display_name: 'Probe Administrator',
  is_admin: false,
  login_name: 'probe-admin',
  user_id: 7,
}

const PRIVATE_NAMESPACE = ['probe-private-namespace', 'value'] as const
const ADMIN_NAMESPACE = ['probe-admin-namespace', 'rows'] as const

/**
 * The identity the mocked server answers with.
 */
let serverMe: MeResponse | null = ADMIN

/**
 * The value the private namespace answers with, so a fresh read of the
 * namespace can be told apart from a carried-over one.
 */
let privateValue = 'probe-private-value-1'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (cause: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, reject, resolve }
}

/**
 * Records the query client of every session the gated subtree renders.
 */
function QueryClientSpy({ clients }: { clients: QueryClient[] }) {
  const client = useQueryClient()
  useEffect(() => {
    clients.push(client)
  }, [client, clients])
  return null
}

function GatedScope({ clients }: { clients: QueryClient[] }) {
  return (
    <>
      <QueryClientSpy clients={clients} />
      <Outlet />
    </>
  )
}

/**
 * A read in an arbitrary namespace, of the kind every Worker page owns.
 */
function PrivateNamespaceProbe() {
  const query = useQuery({
    queryKey: PRIVATE_NAMESPACE,
    queryFn: async () => privateValue,
  })
  return <span data-probe-private="value">{query.data ?? 'pending'}</span>
}

/**
 * The sign-in the gate sends an ended session to, reduced to the one action
 * the session boundary depends on.
 */
function LoginStub() {
  const me = useSessionStore((state) => state.me)
  const login = useSessionStore((state) => state.login)
  const navigate = useNavigate()
  useEffect(() => {
    if (me) navigate('/probe-private', { replace: true })
  }, [me, navigate])
  return (
    <button
      onClick={() =>
        void login({
          login_name: 'probe-admin',
          password: 'probe-synthetic-password',
        })
      }
      type="button"
    >
      Probe sign in
    </button>
  )
}

/**
 * A read behind an admin-only guard.
 */
function AdminNamespaceProbe() {
  const query = useQuery({
    queryKey: ADMIN_NAMESPACE,
    queryFn: async () => ({ rows: ['probe-admin-row'] }),
  })
  return (
    <span data-probe-admin="rows">{(query.data?.rows ?? []).join(',')}</span>
  )
}

/**
 * Reads that belong to the root client, the way the relay host's own reads
 * do.
 */
function RootNamespaceProbe({ inflight }: { inflight: Promise<string> }) {
  const resolved = useQuery({
    queryKey: ['probe-root-resolved'],
    queryFn: async () => 'probe-root-value',
  })
  const pending = useQuery({
    queryKey: ['probe-root-inflight'],
    queryFn: () => inflight,
  })
  return (
    <>
      <span data-probe-root="resolved">{resolved.data ?? 'pending'}</span>
      <span data-probe-root="inflight">{pending.data ?? 'pending'}</span>
    </>
  )
}

function renderSession(options?: {
  inflight?: Promise<string>
  initialPath?: string
}) {
  const rootClient = createAppQueryClient()
  const clients: QueryClient[] = []
  const element = (
    <QueryClientProvider client={rootClient}>
      <LocaleProvider>
        <ThemeProvider>
          <NotifierProvider>
            <MemoryRouter
              initialEntries={[options?.initialPath ?? '/probe-private']}
            >
              {options?.inflight ? (
                <RootNamespaceProbe inflight={options.inflight} />
              ) : null}
              <Routes>
                <Route element={<LoginStub />} path="/login" />
                <Route element={<WorkerAuthGate />}>
                  <Route element={<GatedScope clients={clients} />}>
                    <Route element={<AppShell />}>
                      <Route
                        element={<PrivateNamespaceProbe />}
                        path="/probe-private"
                      />
                      <Route
                        element={
                          <AdminGuard>
                            <AdminNamespaceProbe />
                          </AdminGuard>
                        }
                        path="/probe-admin"
                      />
                      <Route element={<ApiKeysPage />} path="/api-keys" />
                    </Route>
                  </Route>
                </Route>
              </Routes>
            </MemoryRouter>
          </NotifierProvider>
        </ThemeProvider>
      </LocaleProvider>
    </QueryClientProvider>
  )
  const view = render(element)
  return { ...view, clients, element, rootClient }
}

function rootValue(name: 'resolved' | 'inflight'): string {
  return (
    document.querySelector(`[data-probe-root="${name}"]`)?.textContent ?? ''
  )
}

beforeEach(() => {
  Element.prototype.scrollIntoView = () => undefined
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
    error: '',
    epoch: 0,
  })
  useRelaySessionStore.setState({
    authenticated: false,
    bootstrapped: false,
    bootstrapping: false,
    error: '',
  })
  serverMe = ADMIN
  privateValue = 'probe-private-value-1'
  sdkMocks.authMe.mockImplementation(async () => {
    if (!serverMe) {
      throw { error: { code: 'unauthorized', message: 'unauthorized' } }
    }
    return { data: serverMe }
  })
  sdkMocks.authLogout.mockImplementation(async () => {
    serverMe = null
    return { data: undefined }
  })
  sdkMocks.authLogin.mockImplementation(async () => {
    serverMe = ADMIN
    return { data: undefined }
  })
  sdkMocks.bridgeStatus.mockResolvedValue({
    data: {
      configured_relays: 0,
      connected_relays: 0,
      relays: [],
      snapshot_version: 0,
    },
  })
  sdkMocks.meListClientKeys.mockResolvedValue({
    data: { first: 0, keys: [], rows: 10, total: 0 } as ClientKeyPageResponse,
  })
  sdkMocks.relayAuthLogout.mockResolvedValue({ data: undefined })
})

afterEach(() => {
  localStorage.clear()
  serverMe = null
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
    error: '',
    epoch: 0,
  })
  useRelaySessionStore.setState({
    authenticated: false,
    bootstrapped: false,
    bootstrapping: false,
    error: '',
  })
})

describe('Worker query session boundary', () => {
  it('gives a role change its own client', async () => {
    const { clients } = renderSession()
    await screen.findByText('probe-private-value-1')
    await waitFor(() => expect(clients.length).toBe(1))
    const firstClient = clients[0]
    expect(
      firstClient.getQueryCache().find({ queryKey: PRIVATE_NAMESPACE }),
    ).toBeDefined()

    // The same principal, a different role.
    privateValue = 'probe-private-value-2'
    serverMe = OPERATOR
    await act(async () => {
      await useSessionStore.getState().bootstrapAuth(true)
    })

    await waitFor(() => expect(clients.length).toBe(2))
    const secondClient = clients[1]
    expect(secondClient).not.toBe(firstClient)
    // The new session read the namespace again instead of inheriting it.
    expect(
      secondClient.getQueryCache().find({ queryKey: PRIVATE_NAMESPACE })?.state
        .data,
    ).toBe('probe-private-value-2')
    expect(firstClient.getQueryCache().getAll()).toHaveLength(0)
    expect(useSessionStore.getState().me?.is_admin).toBe(false)
  })

  it('denies an admin page once the role no longer allows it', async () => {
    const { clients } = renderSession({ initialPath: '/probe-admin' })
    await screen.findByText('probe-admin-row')
    await waitFor(() => expect(clients.length).toBe(1))
    const firstClient = clients[0]
    expect(
      firstClient.getQueryCache().find({ queryKey: ADMIN_NAMESPACE }),
    ).toBeDefined()

    serverMe = OPERATOR
    await act(async () => {
      await useSessionStore.getState().bootstrapAuth(true)
    })

    await waitFor(() => expect(clients.length).toBe(2))
    await waitFor(() =>
      expect(screen.queryByText('probe-admin-row')).toBeNull(),
    )
    expect(
      clients[1]?.getQueryCache().find({ queryKey: ADMIN_NAMESPACE }),
    ).toBeUndefined()
    expect(firstClient.getQueryCache().getAll()).toHaveLength(0)
  })

  it('keeps an arbitrary private namespace out of the next session', async () => {
    const { clients, rootClient } = renderSession()
    await screen.findByText('probe-private-value-1')
    await waitFor(() => expect(clients.length).toBe(1))
    expect(
      clients[0]?.getQueryCache().find({ queryKey: PRIVATE_NAMESPACE }),
    ).toBeDefined()
    expect(
      rootClient.getQueryCache().find({ queryKey: PRIVATE_NAMESPACE }),
    ).toBeUndefined()

    privateValue = 'probe-private-value-2'
    await act(async () => {
      await useSessionStore.getState().logout()
    })
    const signIn = await screen.findByRole('button', { name: 'Probe sign in' })
    await act(async () => {
      fireEvent.click(signIn)
    })

    await waitFor(() => expect(clients.length).toBe(2))
    await screen.findByText('probe-private-value-2')
    // The new session read the namespace again instead of inheriting it.
    expect(
      clients[1]?.getQueryCache().find({ queryKey: PRIVATE_NAMESPACE })?.state
        .data,
    ).toBe('probe-private-value-2')
    expect(clients[0]?.getQueryCache().getAll()).toHaveLength(0)
    expect(
      rootClient.getQueryCache().find({ queryKey: PRIVATE_NAMESPACE }),
    ).toBeUndefined()
  })

  it('leaves the root relay client alone when the worker session ends', async () => {
    const inflight = deferred<string>()
    const { clients } = renderSession({ inflight: inflight.promise })
    await waitFor(() => expect(rootValue('resolved')).toBe('probe-root-value'))
    useRelaySessionStore.setState({ authenticated: true })

    await act(async () => {
      await useSessionStore.getState().logout()
    })

    expect(rootValue('resolved')).toBe('probe-root-value')
    expect(useRelaySessionStore.getState().authenticated).toBe(true)
    inflight.resolve('probe-root-late-value')
    await waitFor(() =>
      expect(rootValue('inflight')).toBe('probe-root-late-value'),
    )
    expect(clients.at(-1)?.getQueryCache().getAll()).toHaveLength(0)
  })

  it('keeps the worker client and session when the relay session ends', async () => {
    const { clients, element, rerender } = renderSession()
    await screen.findByText('probe-private-value-1')
    await waitFor(() => expect(clients.length).toBe(1))
    const workerClient = clients[0]
    const epoch = useSessionStore.getState().epoch
    useRelaySessionStore.setState({ authenticated: true })

    await act(async () => {
      await useRelaySessionStore.getState().logout()
    })

    expect(useRelaySessionStore.getState().authenticated).toBe(false)
    expect(useSessionStore.getState().epoch).toBe(epoch)
    expect(useSessionStore.getState().me).not.toBeNull()
    expect(clients.length).toBe(1)
    expect(clients[0]).toBe(workerClient)
    expect(
      workerClient.getQueryCache().find({ queryKey: PRIVATE_NAMESPACE }),
    ).toBeDefined()

    // An ordinary re-render of the same session keeps the same client.
    rerender(element)
    expect(clients.length).toBe(1)
    expect(clients[0]).toBe(workerClient)
  })
})
