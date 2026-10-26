import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  McpCatalogResponse,
  McpProviderDescriptor,
  McpServer,
  McpServerPageResponse,
  McpServerRequest,
  User,
} from '@/generated/admin-api'
import { McpPage } from '@/pages/McpPage'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'

const api = vi.hoisted(() => ({
  createMcpServer: vi.fn(),
  deleteMcpServer: vi.fn(),
  getMcpCatalog: vi.fn(),
  listMcpProviders: vi.fn(),
  listMcpServers: vi.fn(),
  listUserOptions: vi.fn(),
  testMcpServer: vi.fn(),
  updateMcpServer: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => ({
  createMcpServer: api.createMcpServer,
  deleteMcpServer: api.deleteMcpServer,
  getMcpCatalog: api.getMcpCatalog,
  listMcpProviders: api.listMcpProviders,
  listMcpServers: api.listMcpServers,
  listUserOptions: api.listUserOptions,
  testMcpServer: api.testMcpServer,
  updateMcpServer: api.updateMcpServer,
}))

const admin = {
  display_name: 'Fixture Administrator',
  is_admin: true,
  login_name: 'admin',
  user_id: 1,
}
const operator = {
  display_name: 'Fixture Operator',
  is_admin: false,
  login_name: 'operator',
  user_id: 2,
}

const users: User[] = [
  {
    created_at: '2026-10-20T00:00:00.000Z',
    display_name: 'Fixture Operator',
    is_active: true,
    is_admin: false,
    login_name: 'operator',
    updated_at: '2026-10-24T00:00:00.000Z',
    user_id: 2,
  },
]

const providers: McpProviderDescriptor[] = [
  {
    auth: 'none',
    default_url: null,
    display_name: 'Generic',
    id: 'generic',
    provider_balance_supported: false,
    unit: 'requests',
  },
  {
    auth: 'bearer',
    default_url: 'https://mcp.context7.com/mcp',
    display_name: 'Context7',
    id: 'context7',
    provider_balance_supported: false,
    unit: 'requests',
  },
]

function makeServer(overrides: Partial<McpServer> = {}): McpServer {
  return {
    aggregate_naming_mode: 'passthrough_preferred',
    allowed_tools: [],
    args: [],
    auth_mode: 'none',
    bearer_tokens: [],
    created_at: '2026-10-20T00:00:00.000Z',
    disabled_resources: [],
    disabled_tools: [],
    enabled: true,
    env_json: {},
    has_basic_password: false,
    has_proxy_url: false,
    http_headers_json: {},
    lifecycle_manual_protocol_version: null,
    lifecycle_policy: 'auto',
    name: 'Fixture Context7',
    owner_user_id: null,
    provider_kind: 'context7',
    scope: 'admin',
    server_id: 'mcp-context7',
    source_endpoint_id: null,
    timeout_ms: 30000,
    tool_filter_mode: 'blacklist',
    transport: 'http',
    updated_at: '2026-10-24T00:00:00.000Z',
    url: 'https://mcp.context7.com/mcp',
    ...overrides,
  }
}

let servers: McpServer[]

function currentPage(first: number, rows: number): McpServerPageResponse {
  const visible = servers.filter(
    (server) =>
      useSessionStore.getState().me?.is_admin === true ||
      (server.scope === 'user' &&
        server.owner_user_id === useSessionStore.getState().me?.user_id),
  )
  return {
    first,
    rows,
    servers: visible.slice(first, first + rows),
    total: visible.length,
  }
}

function renderPage(me: typeof admin | typeof operator) {
  useSessionStore.setState({ me, bootstrapped: true, bootstrapping: false })
  const queryClient = new QueryClient({
    defaultOptions: { queries: { gcTime: 0, retry: false } },
  })
  const view = render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <NotifierProvider>
          <McpPage />
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
  return { ...view, queryClient }
}

beforeEach(() => {
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
  })
  servers = [makeServer()]
  api.listMcpServers.mockImplementation(
    async (options: { query?: { first?: number; rows?: number } }) =>
      currentPage(options.query?.first ?? 0, options.query?.rows ?? 10),
  )
  api.listMcpProviders.mockResolvedValue(providers)
  api.listUserOptions.mockResolvedValue({ users })
  api.getMcpCatalog.mockResolvedValue({
    tools: [],
    resources: [],
    prompts: [],
  } satisfies McpCatalogResponse)
  api.createMcpServer.mockImplementation(
    async (options: { body: McpServerRequest }) => {
      const created = makeServer({
        aggregate_naming_mode:
          options.body.aggregate_naming_mode ?? 'passthrough_preferred',
        allowed_tools: options.body.allowed_tools ?? [],
        args: options.body.args ?? [],
        auth_mode: options.body.auth_mode ?? 'none',
        basic_username: options.body.basic_username ?? null,
        bearer_tokens: options.body.bearer_tokens ?? [],
        command: options.body.command ?? null,
        created_at: '2026-10-24T00:00:00.000Z',
        disabled_resources: options.body.disabled_resources ?? [],
        disabled_tools: options.body.disabled_tools ?? [],
        enabled: options.body.enabled ?? true,
        env_json: options.body.env_json ?? {},
        has_basic_password: Boolean(options.body.basic_password),
        has_proxy_url: Boolean(options.body.proxy_url),
        http_headers_json: options.body.http_headers_json ?? {},
        lifecycle_manual_protocol_version:
          options.body.lifecycle_manual_protocol_version ?? null,
        lifecycle_policy: options.body.lifecycle_policy ?? 'auto',
        name: options.body.name,
        owner_user_id: options.body.owner_user_id ?? null,
        provider_kind: options.body.provider_kind ?? null,
        scope: options.body.scope ?? 'admin',
        server_id: 'mcp-created',
        source_endpoint_id: options.body.source_endpoint_id ?? null,
        timeout_ms: options.body.timeout_ms ?? 30000,
        tool_filter_mode: options.body.tool_filter_mode ?? 'blacklist',
        transport: options.body.transport,
        updated_at: '2026-10-24T00:00:00.000Z',
        url: options.body.url ?? null,
      })
      servers.push(created)
      return created
    },
  )
  api.updateMcpServer.mockImplementation(
    async (options: {
      path: { server_id: string }
      body: McpServerRequest
    }) => {
      const server = servers.find(
        (item) => item.server_id === options.path.server_id,
      )!
      Object.assign(server, options.body)
      return server
    },
  )
  api.deleteMcpServer.mockImplementation(
    async (options: { path: { server_id: string } }) => {
      servers = servers.filter(
        (server) => server.server_id !== options.path.server_id,
      )
      return undefined
    },
  )
  api.testMcpServer.mockResolvedValue({
    duration_ms: 1,
    message: 'fixture succeeded',
    ok: true,
    prompt_count: 0,
    prompts: [],
    resource_count: 0,
    resources: [],
    tool_count: 0,
    tools: [],
  })
})

afterEach(() => {
  cleanup()
})

describe('MCP page role and write behavior', () => {
  it('loads an ordinary user’s owned servers without requesting admin user options', async () => {
    servers = [
      makeServer({
        name: 'Operator MCP',
        scope: 'user',
        owner_user_id: 2,
        server_id: 'mcp-operator',
      }),
      makeServer({
        name: 'Foreign MCP',
        scope: 'user',
        owner_user_id: 3,
        server_id: 'mcp-foreign',
      }),
    ]
    renderPage(operator)

    expect(await screen.findByText('Operator MCP')).toBeTruthy()
    expect(screen.queryByText('Foreign MCP')).toBeNull()
    expect(api.listUserOptions).not.toHaveBeenCalled()
    expect(api.listMcpServers).toHaveBeenCalled()

    fireEvent.click(screen.getByRole('button', { name: 'New MCP' }))
    expect(await screen.findByRole('dialog')).toBeTruthy()
    expect(screen.queryByLabelText('Scope')).toBeNull()
    expect(screen.queryByLabelText('Owner')).toBeNull()
  })

  it('keeps admin scope and owner fields available and forces a non-admin create to its caller', async () => {
    renderPage(admin)
    await screen.findByText('Fixture Context7')
    await waitFor(() => expect(api.listUserOptions).toHaveBeenCalledTimes(1))

    fireEvent.click(screen.getByRole('button', { name: 'New MCP' }))
    const dialog = await screen.findByRole('dialog')
    fireEvent.change(screen.getByLabelText('Name'), {
      target: { value: 'Created MCP' },
    })
    fireEvent.click(within(dialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(api.createMcpServer).toHaveBeenCalledTimes(1))
    const adminBody = api.createMcpServer.mock.calls[0]?.[0]
      .body as McpServerRequest
    expect(adminBody.scope).toBe('admin')
    expect(adminBody.owner_user_id).toBeNull()

    cleanup()
    api.createMcpServer.mockClear()
    servers = [
      makeServer({
        name: 'Operator MCP',
        scope: 'user',
        owner_user_id: 2,
        server_id: 'mcp-operator',
      }),
    ]
    renderPage(operator)
    await screen.findByText('Operator MCP')
    fireEvent.click(screen.getByRole('button', { name: 'New MCP' }))
    const userDialog = await screen.findByRole('dialog')
    fireEvent.change(screen.getByLabelText('Name'), {
      target: { value: 'Operator-created MCP' },
    })
    fireEvent.click(within(userDialog).getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(api.createMcpServer).toHaveBeenCalledTimes(1))
    const userBody = api.createMcpServer.mock.calls[0]?.[0]
      .body as McpServerRequest
    expect(userBody.scope).toBe('user')
    expect(userBody.owner_user_id).toBe(2)
  })
})
