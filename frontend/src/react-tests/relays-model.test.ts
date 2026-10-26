import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { ManagedRelayListResponse } from '@/generated/admin-api'
import {
  createEmptyRelayForm,
  createRelayPatchRequest,
  createRelayRequest,
  relayToForm,
} from '@/admin-mappers/forms/relay'
import {
  loadRelayPageWithCorrection,
  relayActionKey,
  relayPageQueryKey,
} from '@/lib/relays'

const api = vi.hoisted(() => ({
  createRelay: vi.fn(),
  deleteRelay: vi.fn(),
  listRelays: vi.fn(),
  reconnectRelay: vi.fn(),
  updateRelay: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

function response(
  first: number,
  rows: number,
  total: number,
): ManagedRelayListResponse {
  return {
    connected_count: 1,
    enabled_count: 2,
    first,
    relays: [],
    rows,
    total,
  }
}

beforeEach(() => {
  vi.resetAllMocks()
})

describe('relay form mapping', () => {
  it('omits blank create secrets and trims nonempty replacements', () => {
    const form = {
      ...createEmptyRelayForm(),
      bridge_encryption_key: '  bridge-key  ',
      client_cert_pem: '  ',
      name: '  Edge relay  ',
      relay_ca_pem: '  CA material  ',
      relay_url: '  ws://relay.example.test/bridge/  ',
    }

    const request = createRelayRequest(form)

    expect(request).toMatchObject({
      bridge_encryption_key: { mode: 'replace', value: 'bridge-key' },
      name: 'Edge relay',
      relay_ca_pem: { mode: 'replace', value: 'CA material' },
      relay_url: 'ws://relay.example.test/bridge/',
    })
    expect(request.client_cert_pem).toBeUndefined()
    expect(request.client_key_pem).toBeUndefined()
  })

  it('maps each edit secret independently using clear, replace, and keep rules', () => {
    const relay = {
      bridge_encryption_mode: 'required',
      connected: false,
      created_at: '2026-10-24T00:00:00Z',
      enabled: true,
      has_bridge_key: true,
      has_client_cert: true,
      has_client_key: true,
      has_relay_ca: true,
      last_connected_at: null,
      last_disconnected_at: null,
      last_error: null,
      last_snapshot_version: null,
      name: 'Edge relay',
      relay_id: 'relay-edge',
      relay_url: 'wss://relay.example.test/bridge',
      tls_mode: 'mtls',
      updated_at: '2026-10-24T00:00:00Z',
    } as const
    const form = {
      ...relayToForm(relay),
      bridge_encryption_key: '',
      bridge_key_action: 'keep' as const,
      client_cert_action: 'keep' as const,
      client_cert_pem: '  updated cert  ',
      client_key_action: 'replace' as const,
      client_key_pem: '',
      relay_ca_action: 'clear' as const,
      relay_ca_pem: 'ignored because clear wins',
    }

    expect(createRelayPatchRequest(form)).toMatchObject({
      bridge_encryption_key: { mode: 'keep' },
      client_cert_pem: { mode: 'replace', value: 'updated cert' },
      client_key_pem: { mode: 'keep' },
      relay_ca_pem: { mode: 'clear' },
    })
  })

  it('omits blank fields without stored secrets and keeps cleared form defaults', () => {
    const relay = {
      bridge_encryption_mode: 'off',
      connected: false,
      created_at: '2026-10-24T00:00:00Z',
      enabled: true,
      has_bridge_key: false,
      has_client_cert: false,
      has_client_key: false,
      has_relay_ca: false,
      last_connected_at: null,
      last_disconnected_at: null,
      last_error: null,
      last_snapshot_version: null,
      name: 'Edge relay',
      relay_id: 'relay-edge',
      relay_url: 'ws://relay.example.test/bridge',
      tls_mode: 'off',
      updated_at: '2026-10-24T00:00:00Z',
    } as const
    const form = relayToForm(relay)

    expect(form.relay_ca_action).toBe('replace')
    expect(form.client_cert_action).toBe('replace')
    expect(form.client_key_action).toBe('replace')
    expect(form.bridge_key_action).toBe('replace')
    expect(createRelayPatchRequest(form).relay_ca_pem).toBeUndefined()
    expect(createRelayPatchRequest(form).client_cert_pem).toBeUndefined()
    expect(createRelayPatchRequest(form).client_key_pem).toBeUndefined()
    expect(createRelayPatchRequest(form).bridge_encryption_key).toBeUndefined()
  })
})

describe('relay query model', () => {
  it('uses the shared relay:create and relay:<id> guard keys', () => {
    expect(relayActionKey(null)).toBe('relay:create')
    expect(relayActionKey('relay-edge')).toBe('relay:relay-edge')
    expect(relayPageQueryKey(20, 10)).toEqual(['relays', 20, 10])
  })

  it('corrects an out-of-range last page after deleting the former final row', async () => {
    api.listRelays.mockImplementation(
      async ({ query }: { query: { first: number; rows: number } }) =>
        response(query.first, query.rows, 12),
    )

    const page = await loadRelayPageWithCorrection(20, 10)

    expect(page.first).toBe(10)
    expect(api.listRelays).toHaveBeenCalledTimes(2)
    expect(
      api.listRelays.mock.calls.map(([input]) => input.query.first),
    ).toEqual([20, 10])
  })
})
