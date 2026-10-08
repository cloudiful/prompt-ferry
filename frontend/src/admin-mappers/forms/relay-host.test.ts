import { describe, expect, it } from 'vitest'
import {
  createEmptyRelayHostForm,
  createRelayHostRoleRequest,
  createRelaySettingsUpdate,
  hasHostRoleChange,
  hasRelaySettingsChange,
  isValidHostRole,
  relayHostToForm,
} from './relay-host'

describe('relay-host form', () => {
  it('defaults to integrated with a blank bind', () => {
    expect(createEmptyRelayHostForm()).toEqual({
      role: 'integrated',
      admin_bind: '',
    })
  })

  it('accepts only the three documented roles', () => {
    expect(isValidHostRole('integrated')).toBe(true)
    expect(isValidHostRole('worker')).toBe(true)
    expect(isValidHostRole('relay')).toBe(true)
    expect(isValidHostRole('relay-only')).toBe(false)
    expect(isValidHostRole('')).toBe(false)
    expect(isValidHostRole(null)).toBe(false)
  })

  it('maps host and settings into the form', () => {
    const form = relayHostToForm(
      { role: 'relay', restart_required: false },
      {
        relay: {
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
        },
        restart_required: false,
      },
    )
    expect(form).toEqual({ role: 'relay', admin_bind: '127.0.0.1:8790' })
  })

  it('falls back to integrated when the role is missing', () => {
    const form = relayHostToForm(null, null)
    expect(form.role).toBe('integrated')
    expect(form.admin_bind).toBe('')
  })

  it('builds a role request without touching settings', () => {
    const request = createRelayHostRoleRequest({
      role: 'worker',
      admin_bind: '127.0.0.1:8790',
    })
    expect(request).toEqual({ role: 'worker' })
  })

  it('trims the bind and omits it when blank', () => {
    expect(
      createRelaySettingsUpdate({ role: 'relay', admin_bind: '  ' }),
    ).toEqual({})
    expect(
      createRelaySettingsUpdate({
        role: 'relay',
        admin_bind: ' 127.0.0.1:8790 ',
      }),
    ).toEqual({ admin_bind: '127.0.0.1:8790' })
  })

  it('detects role and bind changes separately', () => {
    const host = { role: 'integrated', restart_required: false } as const
    expect(hasHostRoleChange({ role: 'relay', admin_bind: '' }, host)).toBe(
      true,
    )
    expect(
      hasHostRoleChange({ role: 'integrated', admin_bind: '' }, host),
    ).toBe(false)
    expect(
      hasRelaySettingsChange(
        { role: 'integrated', admin_bind: '127.0.0.1:8790' },
        null,
      ),
    ).toBe(true)
  })

  // Tester additions: independent probing of the role vocabulary boundary and
  // the change-detection edges the implementation suite asserts indirectly.

  it('rejects role spellings outside the three-value vocabulary', () => {
    expect(isValidHostRole('worker-only')).toBe(false)
    expect(isValidHostRole('integrated-only')).toBe(false)
    expect(isValidHostRole('Integated')).toBe(false)
    expect(isValidHostRole('INTEGRATED')).toBe(false)
    expect(isValidHostRole(' integrated')).toBe(false)
    expect(isValidHostRole(undefined)).toBe(false)
    expect(isValidHostRole(1)).toBe(false)
    expect(isValidHostRole({ role: 'integrated' })).toBe(false)
  })

  it('falls back to integrated for a role value outside the vocabulary', () => {
    const form = relayHostToForm(
      { role: 'relay-only' as never, restart_required: false },
      null,
    )
    expect(form.role).toBe('integrated')
  })

  it('treats a whitespace-only difference as no settings change', () => {
    expect(
      hasRelaySettingsChange(
        { role: 'relay', admin_bind: '127.0.0.1:8790' },
        {
          relay: {
            admin_bind: '  127.0.0.1:8790  ',
            bind: 'x',
            bridge_encryption_required: false,
            client_token_configured: false,
            request_timeout_seconds: 30,
            tls_enabled: false,
            worker_bind: 'y',
            worker_heartbeat_timeout_seconds: 30,
            worker_tls_enabled: false,
            worker_token_configured: false,
          },
          restart_required: false,
        },
      ),
    ).toBe(false)
    expect(
      hasRelaySettingsChange(
        { role: 'relay', admin_bind: '127.0.0.1:8791' },
        {
          relay: {
            admin_bind: '127.0.0.1:8790',
            bind: 'x',
            bridge_encryption_required: false,
            client_token_configured: false,
            request_timeout_seconds: 30,
            tls_enabled: false,
            worker_bind: 'y',
            worker_heartbeat_timeout_seconds: 30,
            worker_tls_enabled: false,
            worker_token_configured: false,
          },
          restart_required: false,
        },
      ),
    ).toBe(true)
  })

  it('builds a settings update that never carries the role', () => {
    const update = createRelaySettingsUpdate({
      role: 'worker',
      admin_bind: '127.0.0.1:8790',
    })
    expect(update).toEqual({ admin_bind: '127.0.0.1:8790' })
    expect('role' in update).toBe(false)
    expect('pending_role' in update).toBe(false)
  })
})
