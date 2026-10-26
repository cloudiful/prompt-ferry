import { describe, expect, it } from 'vitest'
import type {
  RelayHostResponse,
  RelaySettingsResponse,
  RelayStatusResponse,
} from '@/generated/admin-api'
import {
  HOST_ROLES,
  createEmptyRelayHostForm,
  createRelayHostRoleRequest,
  createRelaySettingsUpdate,
  hasHostRoleChange,
  hasRelaySettingsChange,
  isValidHostRole,
  relayHostToForm,
} from '@/admin-mappers/forms/relay-host'
import {
  canManageWorker,
  isHostRestartRequired,
  isRelayReady,
  isWorkerConnected,
  workerLinkState,
} from '@/models/endpoints/capability'

function relayConfig(): RelaySettingsResponse['relay'] {
  return {
    admin_bind: '127.0.0.1:8790',
    bind: '127.0.0.1:19701',
    bridge_encryption_required: false,
    client_token_configured: false,
    request_timeout_seconds: 30,
    tls_enabled: false,
    worker_bind: '127.0.0.1:19702',
    worker_heartbeat_timeout_seconds: 30,
    worker_tls_enabled: false,
    worker_token_configured: false,
  }
}

function host(overrides: Partial<RelayHostResponse> = {}): RelayHostResponse {
  return { role: 'relay', restart_required: false, ...overrides }
}

function settings(
  overrides: Partial<RelaySettingsResponse> = {},
): RelaySettingsResponse {
  return {
    relay: relayConfig(),
    restart_required: false,
    ...overrides,
  }
}

function status(
  overrides: Partial<RelayStatusResponse> = {},
): RelayStatusResponse {
  return {
    relay: relayConfig(),
    relay_ready: true,
    restart_required: false,
    role: 'relay',
    worker: { connected: false, connected_workers: 0 },
    ...overrides,
  }
}

describe('relay host role and capability model', () => {
  it('starts a new host form on the integrated role without a bind', () => {
    expect(createEmptyRelayHostForm()).toEqual({
      role: 'integrated',
      admin_bind: '',
    })
    expect([...HOST_ROLES]).toEqual(['integrated', 'worker', 'relay'])
    for (const role of HOST_ROLES) {
      expect(isValidHostRole(role)).toBe(true)
    }
    expect(isValidHostRole('unknown-role')).toBe(false)
    expect(isValidHostRole(undefined)).toBe(false)
  })

  it('projects the running role and bind, falling back for an unknown role', () => {
    expect(relayHostToForm(host({ role: 'worker' }), settings())).toEqual({
      role: 'worker',
      admin_bind: '127.0.0.1:8790',
    })
    expect(
      relayHostToForm(
        host({ role: 'unknown-role' as never }),
        settings({ relay: { ...relayConfig(), admin_bind: '' } }),
      ),
    ).toEqual({ role: 'integrated', admin_bind: '' })
    expect(relayHostToForm(null, null)).toEqual({
      role: 'integrated',
      admin_bind: '',
    })
  })

  it('carries only the role in a host role request', () => {
    const request = createRelayHostRoleRequest({
      ...createEmptyRelayHostForm(),
      role: 'worker',
    })
    expect(request).toEqual({ role: 'worker' })
    expect('enabled' in request).toBe(false)
    expect('admin_bind' in request).toBe(false)
  })

  it('detects a role change against the running role only', () => {
    expect(
      hasHostRoleChange(
        { role: 'relay', admin_bind: '' },
        host({ role: 'integrated' }),
      ),
    ).toBe(true)
    expect(
      hasHostRoleChange(
        { role: 'integrated', admin_bind: '' },
        host({ role: 'integrated', restart_required: true }),
      ),
    ).toBe(false)
    expect(hasHostRoleChange({ role: 'relay', admin_bind: '' }, null)).toBe(
      false,
    )
  })

  it('keeps the worker display scoped to the worker link', () => {
    const disconnected = status({
      worker: { connected: false, connected_workers: 0 },
    })
    expect(isWorkerConnected(disconnected)).toBe(false)
    expect(canManageWorker(disconnected)).toBe(false)
    expect(isRelayReady(disconnected)).toBe(true)

    const connected = status({
      worker: { connected: true, connected_workers: 1, config_version: 1 },
    })
    expect(isWorkerConnected(connected)).toBe(true)
    expect(canManageWorker(connected)).toBe(true)
  })

  it('requires a restart when any host surface reports it', () => {
    const hostOff = host({ restart_required: false })
    const statusOn = status({ restart_required: true })
    const settingsOff = settings({ restart_required: false })

    expect(isHostRestartRequired(hostOff, statusOn, settingsOff)).toBe(true)
    expect(
      isHostRestartRequired(
        hostOff,
        status({ restart_required: false }),
        settingsOff,
      ),
    ).toBe(false)
    expect(
      isHostRestartRequired(
        host({ restart_required: true }),
        status({ restart_required: false }),
        settingsOff,
      ),
    ).toBe(true)
    expect(
      isHostRestartRequired(
        hostOff,
        status({ restart_required: false }),
        settings({ restart_required: true }),
      ),
    ).toBe(true)
  })

  it('shows the running bind while a restart is pending', () => {
    const form = relayHostToForm(
      host({ role: 'relay', restart_required: true }),
      settings({ restart_required: true }),
    )
    expect(form.admin_bind).toBe('127.0.0.1:8790')
    expect(
      isHostRestartRequired(host({ restart_required: true }), null, null),
    ).toBe(true)
  })

  it('sends no update for a blank bind so an empty save asks for no restart', () => {
    expect(
      createRelaySettingsUpdate({ role: 'relay', admin_bind: '   ' }),
    ).toEqual({})
    expect(
      hasRelaySettingsChange({ role: 'relay', admin_bind: '' }, null),
    ).toBe(false)
    expect(
      hasRelaySettingsChange(
        { role: 'relay', admin_bind: '127.0.0.1:8791' },
        settings(),
      ),
    ).toBe(true)
  })

  it('trims the bind before saving', () => {
    expect(
      createRelaySettingsUpdate({
        role: 'integrated',
        admin_bind: ' 127.0.0.1:8790 ',
      }),
    ).toEqual({ admin_bind: '127.0.0.1:8790' })
  })

  it('keeps the worker link state a display label, never a page failure', () => {
    expect(
      workerLinkState(
        status({ worker: { connected: false, connected_workers: 0 } }),
      ),
    ).toBe('disconnected')
    expect(workerLinkState(null)).toBe('disconnected')
    expect(
      workerLinkState(
        status({ worker: { connected: true, connected_workers: 1 } }),
      ),
    ).toBe('connected')
  })
})
