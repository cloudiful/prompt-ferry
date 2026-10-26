import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  RelayConfigView,
  RelayHostResponse,
  RelaySettingsResponse,
  RelayStatusResponse,
} from '@/generated/admin-api'
import { RelayHostPage } from '@/pages/RelayHostPage'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useRelaySessionStore, useSessionStore } from '@/lib/session'

const api = vi.hoisted(() => ({
  authLogin: vi.fn(),
  authLogout: vi.fn(),
  authMe: vi.fn(),
  bridgeStatus: vi.fn(),
  relayAuthLogin: vi.fn(),
  relayAuthLogout: vi.fn(),
  relayAuthMe: vi.fn(),
  relayGetSettings: vi.fn(),
  relayHost: vi.fn(),
  relayRequestRestart: vi.fn(),
  relaySetHostRole: vi.fn(),
  relaySetSettings: vi.fn(),
  relayStatus: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

const HOST_KEY = ['relay-host']
const STATUS_KEY = ['relay-status']
const SETTINGS_KEY = ['relay-settings']
const RUNNING_BIND = '127.0.0.1:9444'

function relayConfig(
  overrides: Partial<RelayConfigView> = {},
): RelayConfigView {
  return {
    admin_bind: RUNNING_BIND,
    bind: '0.0.0.0:9443',
    bridge_encryption_required: true,
    client_token_configured: true,
    request_timeout_seconds: 300,
    tls_enabled: true,
    worker_bind: '127.0.0.1:8788',
    worker_heartbeat_timeout_seconds: 45,
    worker_tls_enabled: false,
    worker_token_configured: true,
    ...overrides,
  }
}

function relayHostResponse(
  overrides: Partial<RelayHostResponse> = {},
): RelayHostResponse {
  return {
    pending_role: null,
    restart_required: false,
    role: 'integrated',
    ...overrides,
  }
}

function relayStatusResponse(
  overrides: Partial<RelayStatusResponse> = {},
): RelayStatusResponse {
  return {
    relay: relayConfig(),
    relay_ready: true,
    restart_required: false,
    role: 'integrated',
    worker: { config_version: 7, connected: false, connected_workers: 0 },
    ...overrides,
  }
}

function relaySettingsResponse(
  overrides: Partial<RelaySettingsResponse> = {},
): RelaySettingsResponse {
  return {
    relay: relayConfig(),
    restart_required: false,
    ...overrides,
  }
}

function apiFailure(message: string) {
  return { error: { code: 'fixture_error', message } }
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

let currentHost: RelayHostResponse
let currentStatus: RelayStatusResponse
let currentSettings: RelaySettingsResponse

function renderPage(authenticated = true) {
  useRelaySessionStore.setState({
    authenticated,
    bootstrapped: true,
    bootstrapping: false,
    error: '',
  })
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { gcTime: 0, retry: false },
    },
  })
  const view = render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <NotifierProvider>
          <RelayHostPage />
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
  return { ...view, queryClient }
}

function cardWithTitle(title: string): HTMLElement {
  const element = screen
    .getByText(title, { exact: true })
    .closest('[data-slot="card"]')
  if (!(element instanceof HTMLElement))
    throw new Error(`missing card: ${title}`)
  return element
}

function setInputValue(input: HTMLInputElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(
    HTMLInputElement.prototype,
    'value',
  )?.set
  setter?.call(input, value)
  fireEvent.input(input, { target: { value } })
  fireEvent.change(input, { target: { value } })
}

beforeEach(() => {
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  currentHost = relayHostResponse()
  currentStatus = relayStatusResponse()
  currentSettings = relaySettingsResponse()

  api.authLogin.mockResolvedValue(undefined)
  api.authLogout.mockResolvedValue(undefined)
  api.authMe.mockResolvedValue(null)
  api.bridgeStatus.mockResolvedValue(null)
  api.relayAuthLogin.mockResolvedValue(undefined)
  api.relayAuthLogout.mockResolvedValue(undefined)
  api.relayAuthMe.mockResolvedValue({ authenticated: false })
  api.relayHost.mockImplementation(async () => currentHost)
  api.relayStatus.mockImplementation(async () => currentStatus)
  api.relayGetSettings.mockImplementation(async () => currentSettings)
  api.relaySetHostRole.mockImplementation(
    async (options: { body: { role: 'integrated' | 'worker' | 'relay' } }) => {
      const role = options.body.role
      currentHost = relayHostResponse({
        pending_role: role === 'integrated' ? null : role,
        restart_required:
          role !== 'integrated' || currentSettings.restart_required,
        role,
      })
      currentStatus = relayStatusResponse({
        ...currentStatus,
        restart_required: currentHost.restart_required,
        role,
      })
      return currentHost
    },
  )
  api.relaySetSettings.mockImplementation(
    async (options: { body: { admin_bind?: string } }) => {
      if (options.body.admin_bind !== undefined) {
        currentSettings = relaySettingsResponse({
          ...currentSettings,
          restart_required: options.body.admin_bind !== RUNNING_BIND,
        })
      }
      return currentSettings
    },
  )
  api.relayRequestRestart.mockResolvedValue({
    message: 'restart requested',
    restart_requested: true,
  })

  useRelaySessionStore.setState({
    authenticated: false,
    bootstrapped: false,
    bootstrapping: false,
    error: '',
  })
  useSessionStore.setState({
    me: null,
    bridgeStatus: null,
    bootstrapped: false,
    bootstrapping: false,
    error: '',
    bridgeStatusError: '',
  })
})

afterEach(() => {
  cleanup()
})

describe('Relay host page', () => {
  it('gates reads behind relay authentication, loads all three resources once, and clears only relay data on logout', async () => {
    api.relayAuthMe
      .mockResolvedValueOnce({ authenticated: true })
      .mockResolvedValue({ authenticated: false })
    api.relayAuthLogin.mockResolvedValue(undefined)
    useSessionStore.setState({
      me: {
        display_name: 'Worker account',
        is_admin: true,
        login_name: 'worker-admin',
        user_id: 1,
      },
    })
    const { queryClient } = renderPage(false)

    const tokenInput = await screen.findByLabelText('Enter management token')
    expect(api.relayHost).not.toHaveBeenCalled()
    expect(api.relayStatus).not.toHaveBeenCalled()
    expect(api.relayGetSettings).not.toHaveBeenCalled()

    fireEvent.change(tokenInput, { target: { value: 'relay-fixture-token' } })
    fireEvent.click(screen.getByRole('button', { name: 'Login' }))

    const bindInput = await screen.findByLabelText('Management bind')
    await waitFor(() => {
      expect(api.relayHost).toHaveBeenCalledTimes(1)
      expect(api.relayStatus).toHaveBeenCalledTimes(1)
      expect(api.relayGetSettings).toHaveBeenCalledTimes(1)
    })
    expect(bindInput).toHaveProperty('value', RUNNING_BIND)
    expect(
      screen.getByText('Worker disconnected', { exact: true }),
    ).toBeTruthy()
    expect(screen.getByLabelText('Worker: 0')).toBeTruthy()
    const integratedCard = cardWithTitle('Integrated')
    expect(
      within(integratedCard).getByRole('button', { name: 'In use' }),
    ).toHaveProperty('disabled', true)
    expect(
      within(cardWithTitle('Worker only')).getByRole('button', {
        name: 'Use this mode',
      }),
    ).toHaveProperty('disabled', false)
    expect(await screen.findByText('Saved', { exact: true })).toBeTruthy()

    fireEvent.focus(window)
    fireEvent(document, new Event('visibilitychange'))
    await act(async () => new Promise((resolve) => setTimeout(resolve, 25)))
    expect(api.relayHost).toHaveBeenCalledTimes(1)
    expect(api.relayStatus).toHaveBeenCalledTimes(1)
    expect(api.relayGetSettings).toHaveBeenCalledTimes(1)
    expect(queryClient.getQueryData(HOST_KEY)).toBeTruthy()
    expect(queryClient.getQueryData(STATUS_KEY)).toBeTruthy()
    expect(queryClient.getQueryData(SETTINGS_KEY)).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Logout' }))
    await screen.findByLabelText('Enter management token')
    await waitFor(() => {
      expect(queryClient.getQueryData(HOST_KEY)).toBeUndefined()
      expect(queryClient.getQueryData(STATUS_KEY)).toBeUndefined()
      expect(queryClient.getQueryData(SETTINGS_KEY)).toBeUndefined()
    })
    expect(screen.getByLabelText('Enter management token')).toHaveProperty(
      'value',
      '',
    )
    expect(useRelaySessionStore.getState().authenticated).toBe(false)
    expect(useSessionStore.getState().me?.login_name).toBe('worker-admin')
  })

  it('retries role writes, applies the response without refetching, and preserves a newer bind edit', async () => {
    renderPage()
    await screen.findByLabelText('Management bind')
    const workerCard = cardWithTitle('Worker only')
    const workerButton = within(workerCard).getByRole('button', {
      name: 'Use this mode',
    })
    api.relaySetHostRole.mockRejectedValueOnce(
      apiFailure('role save unavailable'),
    )

    fireEvent.click(workerButton)
    expect(await screen.findByText(/role save unavailable/)).toBeTruthy()
    expect(workerButton).toHaveProperty('disabled', false)
    expect(api.relaySetHostRole.mock.calls[0]?.[0]).toMatchObject({
      body: { role: 'worker' },
    })

    const pendingRole = deferred<RelayHostResponse>()
    api.relaySetHostRole.mockReturnValueOnce(pendingRole.promise)
    fireEvent.click(workerButton)
    await waitFor(() => expect(workerButton).toHaveProperty('disabled', true))
    const bindInput = screen.getByLabelText(
      'Management bind',
    ) as HTMLInputElement
    expect(bindInput.disabled).toBe(true)
    setInputValue(bindInput, '127.0.0.1:9550')

    await act(async () => {
      pendingRole.resolve(
        relayHostResponse({
          pending_role: 'worker',
          restart_required: true,
          role: 'worker',
        }),
      )
    })

    await waitFor(() =>
      expect(
        within(cardWithTitle('Worker only')).getByRole('button', {
          name: 'In use',
        }),
      ).toHaveProperty('disabled', true),
    )
    expect(bindInput).toHaveProperty('value', '127.0.0.1:9550')
    expect(
      within(cardWithTitle('Worker only')).getByText('Current'),
    ).toBeTruthy()
    expect(
      within(cardWithTitle('Worker only')).getByText('Pending restart'),
    ).toBeTruthy()
    expect(api.relayHost).toHaveBeenCalledTimes(1)
    expect(api.relayStatus).toHaveBeenCalledTimes(1)
    expect(api.relayGetSettings).toHaveBeenCalledTimes(1)
    expect(api.relaySetHostRole).toHaveBeenCalledTimes(2)
    expect(
      await screen.findByText('Host role saved; restart to take effect'),
    ).toBeTruthy()
    expect(screen.getByText('Restart required', { exact: true })).toBeTruthy()
  })

  it('trims settings, keeps failed drafts, holds the guard through rereads, and separates restart origins', async () => {
    const { queryClient } = renderPage()
    const bindInput = (await screen.findByLabelText(
      'Management bind',
    )) as HTMLInputElement
    const settingsCard = cardWithTitle('Local relay settings')
    const saveButton = within(settingsCard).getByRole('button', {
      name: 'Save',
    })

    fireEvent.change(bindInput, { target: { value: '   ' } })
    fireEvent.click(saveButton)
    await waitFor(() => expect(api.relaySetSettings).toHaveBeenCalledTimes(1))
    expect(api.relaySetSettings.mock.calls[0]?.[0]).toMatchObject({ body: {} })
    expect(bindInput).toHaveProperty('value', RUNNING_BIND)
    expect(screen.queryByText('Restart required', { exact: true })).toBeNull()

    api.relaySetSettings.mockRejectedValueOnce(
      apiFailure('settings save unavailable'),
    )
    fireEvent.change(bindInput, {
      target: { value: '  127.0.0.1:9550  ' },
    })
    fireEvent.click(saveButton)
    expect(await screen.findByText(/settings save unavailable/)).toBeTruthy()
    expect(bindInput).toHaveProperty('value', '  127.0.0.1:9550  ')
    expect(api.relaySetSettings.mock.calls[1]?.[0]).toMatchObject({
      body: { admin_bind: '127.0.0.1:9550' },
    })

    const savedSettings = deferred<RelaySettingsResponse>()
    const rereadHost = deferred<RelayHostResponse>()
    const rereadStatus = deferred<RelayStatusResponse>()
    api.relaySetSettings.mockReturnValueOnce(savedSettings.promise)
    api.relayHost.mockReturnValueOnce(rereadHost.promise)
    api.relayStatus.mockReturnValueOnce(rereadStatus.promise)
    fireEvent.click(saveButton)
    await waitFor(() => expect(saveButton).toHaveProperty('disabled', true))
    expect(saveButton.querySelector('.animate-spin')).toBeTruthy()
    const duringSaveRoleButton = within(cardWithTitle('Worker only')).getByRole(
      'button',
      { name: 'Use this mode' },
    )
    expect(duringSaveRoleButton).toHaveProperty('disabled', true)
    setInputValue(bindInput, '127.0.0.1:9660')

    const pendingSettings = relaySettingsResponse({ restart_required: true })
    const pendingHost = relayHostResponse({ restart_required: true })
    const pendingStatus = relayStatusResponse({ restart_required: true })
    await act(async () => {
      savedSettings.resolve(pendingSettings)
    })
    await waitFor(() => {
      expect(api.relayHost).toHaveBeenCalledTimes(3)
      expect(api.relayStatus).toHaveBeenCalledTimes(3)
    })
    expect(saveButton).toHaveProperty('disabled', true)
    expect(duringSaveRoleButton).toHaveProperty('disabled', true)

    await act(async () => {
      rereadHost.resolve(pendingHost)
      rereadStatus.resolve(pendingStatus)
    })
    await waitFor(() => expect(saveButton).toHaveProperty('disabled', false))
    expect(bindInput).toHaveProperty('value', '127.0.0.1:9660')
    expect(api.relaySetSettings.mock.calls[2]?.[0]).toMatchObject({
      body: { admin_bind: '127.0.0.1:9550' },
    })
    expect(queryClient.getQueryData(SETTINGS_KEY)).toEqual(pendingSettings)
    await waitFor(() =>
      expect(
        screen.getAllByText('Saved', { exact: true }).length,
      ).toBeGreaterThanOrEqual(2),
    )
    expect(screen.getByText('Restart required', { exact: true })).toBeTruthy()

    currentHost = pendingHost
    currentStatus = pendingStatus
    currentSettings = pendingSettings
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => {
      expect(api.relayHost).toHaveBeenCalledTimes(4)
      expect(api.relayStatus).toHaveBeenCalledTimes(4)
      expect(api.relayGetSettings).toHaveBeenCalledTimes(2)
    })
    await waitFor(() => expect(bindInput).toHaveProperty('value', RUNNING_BIND))

    api.relayRequestRestart.mockRejectedValueOnce(
      apiFailure('restart request unavailable'),
    )
    const banner = screen
      .getByText('Restart required', { exact: true })
      .closest('[role="alert"]') as HTMLElement
    const bannerRestart = within(banner).getByRole('button', {
      name: 'Restart now',
    })
    fireEvent.click(bannerRestart)
    expect(await screen.findByText(/restart request unavailable/)).toBeTruthy()

    const bannerRestartPending = deferred<{
      message: string
      restart_requested: boolean
    }>()
    api.relayRequestRestart.mockReturnValueOnce(bannerRestartPending.promise)
    fireEvent.click(bannerRestart)
    await waitFor(() => expect(bannerRestart).toHaveProperty('disabled', true))
    expect(bannerRestart.querySelector('.animate-spin')).toBeTruthy()
    const settingsRestart = within(settingsCard).getByRole('button', {
      name: 'Restart now',
    })
    expect(settingsRestart).toHaveProperty('disabled', true)
    expect(settingsRestart.querySelector('.animate-spin')).toBeNull()
    await act(async () => {
      bannerRestartPending.resolve({
        message: 'restart requested',
        restart_requested: true,
      })
    })
    expect(await screen.findByText('Host restart requested')).toBeTruthy()

    const settingsRestartPending = deferred<{
      message: string
      restart_requested: boolean
    }>()
    api.relayRequestRestart.mockReturnValueOnce(settingsRestartPending.promise)
    fireEvent.click(settingsRestart)
    await waitFor(() =>
      expect(settingsRestart).toHaveProperty('disabled', true),
    )
    expect(settingsRestart.querySelector('.animate-spin')).toBeTruthy()
    expect(bannerRestart.querySelector('.animate-spin')).toBeNull()
    await act(async () => {
      settingsRestartPending.resolve({
        message: 'restart requested',
        restart_requested: true,
      })
    })
    expect(api.relayRequestRestart).toHaveBeenCalledTimes(3)
    expect(api.relaySetSettings).toHaveBeenCalledTimes(3)
    expect(api.relaySetHostRole).not.toHaveBeenCalled()
  })
})
