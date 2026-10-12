import { mock } from 'bun:test'
import { ref } from 'vue'

import * as adminApi from '../src/generated/admin-api'

import type {
  AsyncActionGuard,
  McpCatalogResponse,
  McpServer,
  McpTestResponse,
  ManagedRelay,
  RelayHostResponse,
  RelaySettingsResponse,
  RelayStatusResponse,
  User,
} from '../src/generated/admin-api'
import { useAsyncAction } from '../src/composables/useAsyncAction'

/**
 * Shared setup for the record action tests.
 *
 * Each action factory takes its slices, its guard, and its reload as injected
 * dependencies, so a whole domain can be driven on plain refs — no store, no
 * Pinia, no component. The API client is mocked here rather than in the test
 * files: `mock.module` is process-global and this repo's other suites replace
 * the same module, so each file pairs the registration with the import that
 * consumes it (see `loadRecordActions`).
 */

const probe = () => ({
  duration_ms: 1,
  message: 'ok',
  ok: true,
  status: 200,
})

export const deleteClientKey = mock(async (_options: unknown) => undefined)
export const deleteMcpServer = mock(async (_options: unknown) => undefined)
export const deleteRelay = mock(async (_options: unknown) => undefined)
export const deleteUser = mock(async (_options: unknown) => undefined)
export const createClientKey = mock(async (_options: unknown) => ({
  key: { key_id: 'key-new', label: 'new', enabled: true },
  secret: 'secret-new',
}))
export const createMcpServer = mock(async (_options: unknown) => ({
  server_id: 'server-new',
}))
export const createRelay = mock(async (_options: unknown) => ({
  relay_id: 'relay-new',
}))
export const createUser = mock(async (_options: unknown) => ({
  login_name: 'newbie',
  user_id: 9,
}))
export const listClientKeys = mock(async (_options: unknown) => ({
  first: 0,
  keys: [],
  rows: 10,
  total: 0,
}))
export const meCreateClientKey = mock(async (_options: unknown) => ({
  key: { key_id: 'key-new', label: 'new', enabled: true },
  secret: 'secret-new',
}))
export const meDeleteClientKey = mock(async (_options: unknown) => undefined)
export const meUpdateClientKey = mock(async (_options: unknown) => ({
  key_id: 'key-a',
}))
export const relayGetSettings = mock(async (_options: unknown) => ({
  admin_bind: '',
}))
export const relayHost = mock(async (_options: unknown) => ({
  pending_role: null,
  restart_required: false,
  role: 'standalone',
}))
export const relayRequestRestart = mock(async (_options: unknown) => undefined)
export const relaySetHostRole = mock(async (_options: unknown) => ({
  pending_role: null,
  restart_required: true,
  role: 'relay',
}))
export const relaySetSettings = mock(async (_options: unknown) => ({
  admin_bind: '0.0.0.0:8080',
}))
export const relayStatus = mock(async (_options: unknown) => ({
  relay: { connected_workers: 0, role: 'standalone' },
  relay_ready: false,
  restart_required: false,
  worker: { connected: false },
}))
export const reconnectRelay = mock(async (_options: unknown) => ({
  relay_id: 'relay-a',
}))
export const resetPassword = mock(async (_options: unknown) => undefined)
export const testMcpServer = mock(async (_options: unknown) => ({
  prompts: [],
  resources: [],
  tools: [],
}))
export const updateClientKey = mock(async (_options: unknown) => ({
  key_id: 'key-a',
}))
export const updateMcpServer = mock(async (_options: unknown) => ({
  server_id: 'server-a',
}))
export const updateRelay = mock(async (_options: unknown) => ({
  relay_id: 'relay-a',
}))
export const updateUser = mock(async (_options: unknown) => ({
  login_name: 'alice',
  user_id: 1,
}))

const apiModule = () => ({ ...adminApi, ...overridden })

const overridden = {
  createClientKey,
  createMcpServer,
  createRelay,
  createUser,
  deleteClientKey,
  deleteMcpServer,
  deleteRelay,
  deleteUser,
  listClientKeys,
  meCreateClientKey,
  meDeleteClientKey,
  meUpdateClientKey,
  reconnectRelay,
  relayGetSettings,
  relayHost,
  relayRequestRestart,
  relaySetHostRole,
  relaySetSettings,
  relayStatus,
  resetPassword,
  testMcpServer,
  updateClientKey,
  updateMcpServer,
  updateRelay,
  updateUser,
}

export const apiMocks = Object.values(overridden)

/** A gate the tests open by hand, so "in flight" is really in flight. */
export function deferred<T = void>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

export const mcpServer = (id: string) =>
  ({ name: id, server_id: id }) as McpServer
export const relay = (id: string) =>
  ({ name: id, relay_id: id }) as ManagedRelay
export const user = (id: number) =>
  ({ login_name: `user-${id}`, user_id: id }) as User

export type Harness = {
  actions: Record<string, never>
  api: typeof overridden
  guard: AsyncActionGuard
  reloads: { keys: number; mcp: number; page: number; relays: number }
}

function slices() {
  return {
    mcp: {
      catalogCache: ref<Record<string, McpCatalogResponse>>({}),
      servers: ref<McpServer[]>([mcpServer('server-a'), mcpServer('server-b')]),
      testResults: ref<Record<string, McpTestResponse>>({}),
      testingServerId: ref(''),
    },
    users: {
      pageUsers: ref<User[]>([user(1), user(2)]),
      users: ref<User[]>([user(1), user(2)]),
    },
    keys: {
      keys: ref([]),
      selectedUserId: ref(0),
    },
    relays: {
      reconnectingRelayId: ref<string | null>(null),
      relays: ref<ManagedRelay[]>([relay('relay-a'), relay('relay-b')]),
    },
    host: {
      error: ref(''),
      host: ref<RelayHostResponse | null>(null),
      restarting: ref(false),
      savingRole: ref(false),
      savingSettings: ref(false),
      settings: ref<RelaySettingsResponse | null>(null),
      status: ref<RelayStatusResponse | null>(null),
    },
  }
}

/**
 * Every action factory in one harness, on plain refs. `hold` replaces the next
 * call of one API function with one the test controls, which is how a write is
 * held open across its own follow-up reload.
 */
export async function freshHarness(): Promise<Harness> {
  for (const item of apiMocks) item.mockClear()
  mock.module('../src/generated/admin-api', apiModule)

  const slicesNow = slices()
  const guard = useAsyncAction()
  const reloads = { keys: 0, mcp: 0, page: 0, relays: 0 }

  const [mcpActions, userActions, apiKeyActions, relayActions, hostActions] =
    await Promise.all([
      import('../src/stores/mcp-actions'),
      import('../src/stores/users-actions'),
      import('../src/stores/api-keys-actions'),
      import('../src/stores/relays-actions'),
      import('../src/stores/relay-host-actions'),
    ])

  return {
    actions: {
      ...mcpActions.createMcpActions({
        guard,
        mcp: slicesNow.mcp,
        refresh: async () => void reloads.mcp++,
      }),
      ...userActions.createUserActions({
        guard,
        loadPage: async () => void reloads.page++,
        users: slicesNow.users,
      }),
      ...apiKeyActions.createApiKeyActions({
        guard,
        loadKeys: async () => void reloads.keys++,
        selectedUserId: slicesNow.keys.selectedUserId,
      }),
      ...relayActions.createRelayActions({
        guard,
        refresh: async () => void reloads.relays++,
        relays: slicesNow.relays,
      }),
      ...hostActions.createRelayHostActions({ guard, host: slicesNow.host }),
    } as Record<string, never>,
    api: overridden,
    guard,
    reloads,
  }
}
