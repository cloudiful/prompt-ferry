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
import type { McpServer } from '@/generated/admin-api'
import { McpOAuthPanel } from '@/components/mcp/McpOAuthPanel'
import { McpPage } from '@/pages/McpPage'
import { LocaleProvider } from '@/lib/i18n'
import { mcpOAuthStatusQueryKey } from '@/lib/mcp'
import { NotifierProvider } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'

const api = vi.hoisted(() => ({
  createMcpServer: vi.fn(),
  deleteMcpServer: vi.fn(),
  disconnectMcpOauth: vi.fn(),
  getMcpCatalog: vi.fn(),
  getMcpOauthStatus: vi.fn(),
  listMcpProviders: vi.fn(),
  listMcpServers: vi.fn(),
  listUserOptions: vi.fn(),
  startMcpOauth: vi.fn(),
  testMcpServer: vi.fn(),
  updateMcpServer: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => ({
  createMcpServer: api.createMcpServer,
  deleteMcpServer: api.deleteMcpServer,
  disconnectMcpOauth: api.disconnectMcpOauth,
  getMcpCatalog: api.getMcpCatalog,
  getMcpOauthStatus: api.getMcpOauthStatus,
  listMcpProviders: api.listMcpProviders,
  listMcpServers: api.listMcpServers,
  listUserOptions: api.listUserOptions,
  startMcpOauth: api.startMcpOauth,
  testMcpServer: api.testMcpServer,
  updateMcpServer: api.updateMcpServer,
}))

const server: McpServer = {
  aggregate_naming_mode: 'qualified_only',
  allowed_tools: [],
  args: [],
  auth_mode: 'oauth',
  bearer_tokens: [],
  created_at: '2026-11-06T00:00:00.000Z',
  disabled_resources: [],
  disabled_tools: [],
  enabled: true,
  env_json: {},
  has_basic_password: false,
  has_proxy_url: false,
  http_headers_json: {},
  lifecycle_manual_protocol_version: null,
  lifecycle_policy: 'auto',
  name: 'Personal OAuth',
  owner_user_id: 2,
  provider_kind: 'generic',
  scope: 'user',
  server_id: 'oauth-server',
  source_endpoint_id: null,
  timeout_ms: 30000,
  tool_filter_mode: 'blacklist',
  transport: 'http',
  updated_at: '2026-11-06T00:00:00.000Z',
  url: 'https://huggingface.co/mcp?login',
}

function fakePopup() {
  return {
    close: vi.fn(),
    location: { href: '' },
    opener: window,
    closed: false,
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })
  return { promise, resolve, reject }
}

function renderPanel(
  onConnect = vi.fn(),
  onDisconnect = vi.fn(),
  panelServer: McpServer = server,
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <McpOAuthPanel
          onConnect={onConnect}
          onDisconnect={onDisconnect}
          pending={false}
          server={panelServer}
        />
      </LocaleProvider>
    </QueryClientProvider>,
  )
  return { onConnect, onDisconnect, queryClient }
}

beforeEach(() => {
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  useSessionStore.setState({
    me: {
      display_name: 'Operator',
      is_admin: false,
      login_name: 'operator',
      user_id: 2,
    },
    bootstrapped: true,
    bootstrapping: false,
  })
  api.getMcpOauthStatus.mockResolvedValue({
    data: {
      status: 'connected',
      scopes: ['read-mcp'],
      expires_at: '2026-11-06T01:00:00.000Z',
      reason: null,
    },
  })
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
  vi.restoreAllMocks()
})

describe('personal MCP OAuth panel', () => {
  it('shows connected state, read scope and calls reconnect/disconnect', async () => {
    const { onConnect, onDisconnect } = renderPanel()
    await waitFor(() => expect(screen.getByText('Connected')).toBeTruthy())
    expect(api.getMcpOauthStatus).toHaveBeenCalled()
    expect(screen.getByText(/read-mcp/)).toBeTruthy()
    expect(screen.queryByText(/access_token|refresh_token/)).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }))
    fireEvent.click(screen.getByRole('button', { name: 'Disconnect' }))
    expect(onConnect).toHaveBeenCalledWith(server)
    expect(onDisconnect).toHaveBeenCalledWith(server)
  })

  it('shows authorizing and reauthorization states', async () => {
    api.getMcpOauthStatus.mockResolvedValue({
      data: { status: 'authorizing', scopes: [], reason: null },
    })
    renderPanel()
    await waitFor(() =>
      expect(screen.getByText('Waiting for authorization')).toBeTruthy(),
    )
    expect(
      (screen.getByRole('button', { name: 'Connect' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
  })

  it('offers reauthorization and owner-only gating', async () => {
    api.getMcpOauthStatus.mockResolvedValue({
      data: { status: 'reauth_required', scopes: ['read-mcp'], reason: null },
    })
    const { onConnect } = renderPanel()
    await waitFor(() =>
      expect(screen.getByText('Reauthorization required')).toBeTruthy(),
    )
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }))
    expect(onConnect).toHaveBeenCalledWith(server)

    cleanup()
    useSessionStore.setState({
      me: {
        display_name: 'Someone Else',
        is_admin: true,
        login_name: 'someone',
        user_id: 9,
      },
      bootstrapped: true,
      bootstrapping: false,
    })
    renderPanel()
    await waitFor(() =>
      expect(screen.getByText('Reauthorization required')).toBeTruthy(),
    )
    expect(
      screen.getByText('Only the owner can authorize this MCP server.'),
    ).toBeTruthy()
    expect(
      (screen.getByRole('button', { name: 'Reconnect' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
    expect(
      (screen.getByRole('button', { name: 'Disconnect' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
  })

  it('reports a failed OAuth status read instead of presenting a disconnected grant', async () => {
    api.getMcpOauthStatus.mockRejectedValue(new Error('status unavailable'))
    renderPanel()
    await waitFor(() =>
      expect(screen.getByText(/status unavailable/i)).toBeTruthy(),
    )
  })

  it('does not treat an unloaded status as disconnected', async () => {
    api.getMcpOauthStatus.mockReturnValue(new Promise(() => {}))
    renderPanel()
    await waitFor(() =>
      expect(screen.getByText('Loading OAuth status')).toBeTruthy(),
    )
    expect(screen.queryByText('Not connected')).toBeNull()
    expect(
      (screen.getByRole('button', { name: 'Connect' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
    expect(
      (screen.getByRole('button', { name: 'Disconnect' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
  })

  it('marks a failed refresh as unknown instead of keeping a stale connected state', async () => {
    const { queryClient } = renderPanel()
    await waitFor(() => expect(screen.getByText('Connected')).toBeTruthy())
    api.getMcpOauthStatus.mockRejectedValueOnce(new Error('status unavailable'))
    await queryClient.refetchQueries({
      queryKey: mcpOAuthStatusQueryKey('oauth-server'),
    })
    await waitFor(() =>
      expect(screen.getByText(/OAuth status unavailable/)).toBeTruthy(),
    )
    expect(screen.queryByText('Connected')).toBeNull()
    expect(
      (screen.getByRole('button', { name: 'Connect' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
  })

  it('keeps owner cleanup available after a failed read and recovers on retry', async () => {
    api.getMcpOauthStatus.mockRejectedValueOnce(new Error('status unavailable'))
    const { onDisconnect } = renderPanel()
    await waitFor(() =>
      expect(screen.getByText(/OAuth status unavailable/)).toBeTruthy(),
    )
    expect(
      (screen.getByRole('button', { name: 'Connect' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
    const disconnect = screen.getByRole('button', {
      name: 'Disconnect',
    }) as HTMLButtonElement
    expect(disconnect.disabled).toBe(false)
    fireEvent.click(disconnect)
    expect(onDisconnect).toHaveBeenCalledWith(server)

    const retry = deferred<{
      data: { status: string; scopes: string[]; reason: string | null }
    }>()
    api.getMcpOauthStatus.mockReturnValueOnce(retry.promise)
    fireEvent.click(screen.getByRole('button', { name: 'Retry status' }))
    await waitFor(() =>
      expect(screen.getByText('Loading OAuth status')).toBeTruthy(),
    )
    retry.resolve({
      data: { status: 'connected', scopes: ['read-mcp'], reason: null },
    })
    await waitFor(() => expect(screen.getByText('Connected')).toBeTruthy())
    expect(screen.queryByText(/OAuth status unavailable/)).toBeNull()
    expect(
      (screen.getByRole('button', { name: 'Reconnect' }) as HTMLButtonElement)
        .disabled,
    ).toBe(false)
  })

  it('guides unsaved auth-mode changes without perpetual loading', async () => {
    renderPanel(vi.fn(), vi.fn(), { ...server, auth_mode: 'bearer' })
    expect(screen.queryByText('Loading OAuth status')).toBeNull()
    expect(screen.getByText(/save the node before connecting/)).toBeTruthy()
    expect(
      (screen.getByRole('button', { name: 'Connect' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
    expect(api.getMcpOauthStatus).not.toHaveBeenCalled()
  })

  it('disables actions and explains an unavailable configuration', async () => {
    api.getMcpOauthStatus.mockResolvedValue({
      data: {
        status: 'disconnected',
        scopes: [],
        reason: 'mcp_oauth_redirect_base is not configured',
      },
    })
    renderPanel()
    await waitFor(() =>
      expect(screen.getByText(/OAuth is unavailable/)).toBeTruthy(),
    )
    expect(screen.getByText(/mcp_oauth_redirect_base/)).toBeTruthy()
    expect(
      (screen.getByRole('button', { name: 'Connect' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
  })
})

async function renderOAuthPage(servers: McpServer[] = [server]) {
  api.listMcpServers.mockResolvedValue({
    data: { servers, total: servers.length, first: 0, rows: 10 },
  })
  api.listMcpProviders.mockResolvedValue([
    {
      auth: 'none',
      default_url: null,
      display_name: 'Generic',
      id: 'generic',
      provider_balance_supported: false,
      unit: 'requests',
    },
  ])
  api.listUserOptions.mockResolvedValue({ users: [] })
  api.getMcpCatalog.mockResolvedValue({ tools: [], resources: [], prompts: [] })
  useSessionStore.setState({
    me: {
      display_name: 'Operator',
      is_admin: false,
      login_name: 'operator',
      user_id: 2,
    },
    bootstrapped: true,
    bootstrapping: false,
  })
  const queryClient = new QueryClient({
    defaultOptions: { queries: { gcTime: 0, retry: false } },
  })
  const rendered = render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <NotifierProvider>
          <McpPage />
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
  await waitFor(() =>
    expect(screen.getAllByRole('button', { name: 'Manage OAuth' }).length).toBe(
      servers.length,
    ),
  )
  return { ...rendered, queryClient }
}

describe('personal MCP OAuth page action', () => {
  beforeEach(() => {
    api.getMcpOauthStatus.mockResolvedValue({
      data: { status: 'disconnected', scopes: [], reason: null },
    })
  })

  it('opens the authorization window inside the claimed action and navigates it', async () => {
    api.startMcpOauth.mockResolvedValue({
      data: { authorization_url: 'https://as.example.test/authorize' },
    })
    const popup = fakePopup()
    vi.spyOn(window, 'open').mockReturnValue(popup as unknown as Window)
    await renderOAuthPage()
    fireEvent.click(screen.getByRole('button', { name: 'Manage OAuth' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Connect' }))

    await waitFor(() =>
      expect(api.startMcpOauth).toHaveBeenCalledWith(
        expect.objectContaining({ path: { server_id: 'oauth-server' } }),
      ),
    )
    await waitFor(() =>
      expect(popup.location.href).toBe('https://as.example.test/authorize'),
    )
    expect(popup.opener).toBeNull()
    expect(popup.close).not.toHaveBeenCalled()
  })

  it('closes only its blank window and retries after a start failure', async () => {
    api.startMcpOauth
      .mockRejectedValueOnce(new Error('authorization server unavailable'))
      .mockResolvedValueOnce({
        data: { authorization_url: 'https://as.example.test/retry' },
      })
    const failedPopup = fakePopup()
    const retryPopup = fakePopup()
    vi.spyOn(window, 'open')
      .mockReturnValueOnce(failedPopup as unknown as Window)
      .mockReturnValueOnce(retryPopup as unknown as Window)
    await renderOAuthPage()
    fireEvent.click(screen.getByRole('button', { name: 'Manage OAuth' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Connect' }))

    await waitFor(() => expect(failedPopup.close).toHaveBeenCalled())
    expect(failedPopup.location.href).toBe('')

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(api.startMcpOauth).toHaveBeenCalledTimes(2))
    await waitFor(() =>
      expect(retryPopup.location.href).toBe('https://as.example.test/retry'),
    )
  })

  it('reports an actionable error without starting when the popup is blocked', async () => {
    api.startMcpOauth.mockResolvedValue({
      data: { authorization_url: 'https://as.example.test/blocked' },
    })
    const retryPopup = fakePopup()
    vi.spyOn(window, 'open')
      .mockReturnValueOnce(null)
      .mockReturnValueOnce(retryPopup as unknown as Window)
    const initialPath = window.location.pathname
    await renderOAuthPage()
    fireEvent.click(screen.getByRole('button', { name: 'Manage OAuth' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Connect' }))

    await waitFor(() =>
      expect(screen.getByText(/Allow popups in your browser/)).toBeTruthy(),
    )
    expect(api.startMcpOauth).not.toHaveBeenCalled()
    expect(screen.queryByText('Waiting for authorization')).toBeNull()
    expect(window.location.pathname).toBe(initialPath)

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(api.startMcpOauth).toHaveBeenCalledTimes(1))
    await waitFor(() =>
      expect(retryPopup.location.href).toBe('https://as.example.test/blocked'),
    )
    expect(retryPopup.opener).toBeNull()
  })

  it('does not start authorization when the popup is blocked', async () => {
    api.startMcpOauth.mockResolvedValue({
      data: { authorization_url: 'https://as.example.test/blocked' },
    })
    vi.spyOn(window, 'open').mockReturnValue(null)
    await renderOAuthPage()
    fireEvent.click(screen.getByRole('button', { name: 'Manage OAuth' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Connect' }))
    await waitFor(() =>
      expect(screen.getByText(/Allow popups in your browser/)).toBeTruthy(),
    )
    expect(api.startMcpOauth).not.toHaveBeenCalled()
  })

  it('closes the pending popup instead of navigating after the page unmounts', async () => {
    const pending = deferred<{ data: { authorization_url: string } }>()
    api.startMcpOauth.mockReturnValue(pending.promise)
    const popup = fakePopup()
    vi.spyOn(window, 'open').mockReturnValue(popup as unknown as Window)
    await renderOAuthPage()
    fireEvent.click(screen.getByRole('button', { name: 'Manage OAuth' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(api.startMcpOauth).toHaveBeenCalledTimes(1))
    cleanup()
    pending.resolve({
      data: { authorization_url: 'https://as.example.test/late' },
    })
    await waitFor(() => expect(popup.close).toHaveBeenCalled())
    expect(popup.location.href).toBe('')
  })

  it('does not notify the replacement page when a pending start fails after unmount', async () => {
    const pending = deferred<{ data: { authorization_url: string } }>()
    api.startMcpOauth.mockReturnValue(pending.promise)
    const popup = fakePopup()
    vi.spyOn(window, 'open').mockReturnValue(popup as unknown as Window)
    const { rerender, queryClient } = await renderOAuthPage()
    fireEvent.click(screen.getByRole('button', { name: 'Manage OAuth' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(api.startMcpOauth).toHaveBeenCalledTimes(1))
    rerender(
      <QueryClientProvider client={queryClient}>
        <LocaleProvider>
          <NotifierProvider>
            <div>Replacement page</div>
          </NotifierProvider>
        </LocaleProvider>
      </QueryClientProvider>,
    )
    await act(async () => {
      pending.reject(new Error('late authorization error'))
      try {
        await pending.promise
      } catch {}
    })
    expect(popup.close).toHaveBeenCalled()
    expect(screen.queryByText('late authorization error')).toBeNull()
  })

  it('does not navigate a popup the user already closed', async () => {
    const pending = deferred<{ data: { authorization_url: string } }>()
    api.startMcpOauth.mockReturnValue(pending.promise)
    const popup = fakePopup()
    vi.spyOn(window, 'open').mockReturnValue(popup as unknown as Window)
    await renderOAuthPage()
    fireEvent.click(screen.getByRole('button', { name: 'Manage OAuth' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(api.startMcpOauth).toHaveBeenCalledTimes(1))
    popup.closed = true
    pending.resolve({
      data: { authorization_url: 'https://as.example.test/closed' },
    })
    await waitFor(() => expect(popup.close).toHaveBeenCalled())
    expect(popup.location.href).toBe('')
  })

  it('closes the tracked popup on a successful matching disconnect', async () => {
    api.getMcpOauthStatus.mockResolvedValue({
      data: { status: 'connected', scopes: ['read-mcp'], reason: null },
    })
    api.startMcpOauth.mockResolvedValue({
      data: { authorization_url: 'https://as.example.test/disconnect' },
    })
    api.disconnectMcpOauth.mockResolvedValue(undefined)
    const popup = fakePopup()
    vi.spyOn(window, 'open').mockReturnValue(popup as unknown as Window)
    await renderOAuthPage()
    fireEvent.click(screen.getByRole('button', { name: 'Manage OAuth' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Reconnect' }))
    await waitFor(() =>
      expect(popup.location.href).toBe('https://as.example.test/disconnect'),
    )

    fireEvent.click(screen.getByRole('button', { name: 'Disconnect' }))
    await waitFor(() => expect(popup.close).toHaveBeenCalled())
    expect(api.disconnectMcpOauth).toHaveBeenCalledTimes(1)
  })

  it('closes all owned windows for a server when reconnect is started twice and then disconnected', async () => {
    api.getMcpOauthStatus.mockResolvedValue({
      data: { status: 'connected', scopes: ['read-mcp'], reason: null },
    })
    api.startMcpOauth.mockResolvedValue({
      data: { authorization_url: 'https://as.example.test/authorize' },
    })
    api.disconnectMcpOauth.mockResolvedValue(undefined)
    const firstPopup = fakePopup()
    const secondPopup = fakePopup()
    vi.spyOn(window, 'open')
      .mockReturnValueOnce(firstPopup as unknown as Window)
      .mockReturnValueOnce(secondPopup as unknown as Window)
    await renderOAuthPage()
    fireEvent.click(screen.getByRole('button', { name: 'Manage OAuth' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Reconnect' }))
    await waitFor(() =>
      expect(firstPopup.location.href).toBe(
        'https://as.example.test/authorize',
      ),
    )
    fireEvent.click(screen.getByRole('button', { name: 'Reconnect' }))
    await waitFor(() =>
      expect(secondPopup.location.href).toBe(
        'https://as.example.test/authorize',
      ),
    )
    fireEvent.click(screen.getByRole('button', { name: 'Disconnect' }))
    await waitFor(() => expect(secondPopup.close).toHaveBeenCalled())
    expect(firstPopup.close).toHaveBeenCalled()
  })

  it('dispatches one start for duplicate connect clicks under the shared key', async () => {
    const pending = deferred<{
      data: { authorization_url: string }
    }>()
    api.startMcpOauth.mockReturnValue(pending.promise)
    const popup = fakePopup()
    vi.spyOn(window, 'open').mockReturnValue(popup as unknown as Window)
    await renderOAuthPage()
    fireEvent.click(screen.getByRole('button', { name: 'Manage OAuth' }))
    const connect = await screen.findByRole('button', { name: 'Connect' })
    fireEvent.click(connect)
    fireEvent.click(connect)

    await waitFor(() => expect(api.startMcpOauth).toHaveBeenCalledTimes(1))
    expect(window.open).toHaveBeenCalledTimes(1)
    pending.resolve({
      data: { authorization_url: 'https://as.example.test/once' },
    })
    await waitFor(() =>
      expect(popup.location.href).toBe('https://as.example.test/once'),
    )
  })

  it('blocks dismissal while a start is pending and keeps the result on its own server', async () => {
    const pendingA = deferred<{
      data: { authorization_url: string }
    }>()
    api.startMcpOauth.mockReturnValue(pendingA.promise)
    const popupA = fakePopup()
    vi.spyOn(window, 'open').mockReturnValue(popupA as unknown as Window)
    await renderOAuthPage()
    fireEvent.click(screen.getByRole('button', { name: 'Manage OAuth' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Connect' }))

    await waitFor(() => expect(api.startMcpOauth).toHaveBeenCalledTimes(1))
    expect(
      (screen.getByRole('button', { name: 'Cancel' }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' })
    expect(screen.getByRole('textbox', { name: 'Name' })).toBeTruthy()

    pendingA.resolve({
      data: { authorization_url: 'https://as.example.test/a' },
    })
    await waitFor(() =>
      expect(popupA.location.href).toBe('https://as.example.test/a'),
    )
    expect(
      (screen.getByRole('textbox', { name: 'Name' }) as HTMLInputElement).value,
    ).toBe('Personal OAuth')
  })

  it('suppresses a delayed start after the selected server changes', async () => {
    const serverA = { ...server, server_id: 'oauth-a', name: 'Server A' }
    const serverB = { ...server, server_id: 'oauth-b', name: 'Server B' }
    const pending = deferred<{ data: { authorization_url: string } }>()
    api.startMcpOauth.mockReturnValue(pending.promise)
    const popup = fakePopup()
    vi.spyOn(window, 'open').mockReturnValue(popup as unknown as Window)
    await renderOAuthPage([serverA, serverB])
    const rowA = screen.getByText('Server A').closest('tr') as HTMLElement
    const rowB = screen.getByText('Server B').closest('tr') as HTMLElement
    const buttonB = within(rowB).getByRole('button', { name: 'Manage OAuth' })
    fireEvent.click(within(rowA).getByRole('button', { name: 'Manage OAuth' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(api.startMcpOauth).toHaveBeenCalledTimes(1))
    fireEvent.click(buttonB)
    expect(
      (screen.getByRole('textbox', { name: 'Name' }) as HTMLInputElement).value,
    ).toBe('Server B')
    await act(async () => {
      pending.resolve({
        data: { authorization_url: 'https://as.example.test/a' },
      })
      await pending.promise
    })
    await waitFor(() => expect(popup.close).toHaveBeenCalled())
    expect(popup.location.href).toBe('')
    expect(
      (screen.getByRole('textbox', { name: 'Name' }) as HTMLInputElement).value,
    ).toBe('Server B')
  })

  it('ignores a delayed status result after switching to another server', async () => {
    const serverA = { ...server, server_id: 'oauth-a', name: 'Server A' }
    const serverB = { ...server, server_id: 'oauth-b', name: 'Server B' }
    const statusA = deferred<{
      data: { status: string; scopes: string[]; reason: string | null }
    }>()
    api.getMcpOauthStatus.mockImplementation(
      (options: { path: { server_id: string } }) =>
        options.path.server_id === 'oauth-a'
          ? statusA.promise
          : Promise.resolve({
              data: {
                status: 'connected',
                scopes: ['read-mcp'],
                reason: null,
              },
            }),
    )
    await renderOAuthPage([serverA, serverB])

    const rowA = screen.getByText('Server A').closest('tr')
    expect(rowA).not.toBeNull()
    fireEvent.click(
      within(rowA as HTMLElement).getByRole('button', { name: 'Manage OAuth' }),
    )
    await waitFor(() =>
      expect(screen.getByText('Loading OAuth status')).toBeTruthy(),
    )
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())

    const rowB = screen.getByText('Server B').closest('tr')
    expect(rowB).not.toBeNull()
    fireEvent.click(
      within(rowB as HTMLElement).getByRole('button', { name: 'Manage OAuth' }),
    )
    await waitFor(() =>
      expect(
        (screen.getByRole('textbox', { name: 'Name' }) as HTMLInputElement)
          .value,
      ).toBe('Server B'),
    )
    await waitFor(() => expect(screen.getByText('Connected')).toBeTruthy())
    statusA.resolve({
      data: { status: 'reauth_required', scopes: [], reason: null },
    })
    await waitFor(() =>
      expect(
        (screen.getByRole('textbox', { name: 'Name' }) as HTMLInputElement)
          .value,
      ).toBe('Server B'),
    )
    expect(screen.getByText('Connected')).toBeTruthy()
    expect(screen.queryByText('Reauthorization required')).toBeNull()
  })
})
