import { expect, test } from 'bun:test'
import {
  createRelaySettingsUpdate,
  hasRelaySettingsChange,
  relayHostToForm,
} from '../src/admin-mappers/forms/relay-host'
import {
  isHostRestartRequired,
  workerLinkState,
} from '../src/models/endpoints/capability'

test('host page form shows the running bind while a restart is pending', () => {
  const form = relayHostToForm(
    { role: 'relay', restart_required: true },
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
      restart_required: true,
    },
  )
  // The form shows what the host is running (not the pending value); the
  // banner carries the pending indication.
  expect(form.admin_bind).toBe('127.0.0.1:8790')
  expect(
    isHostRestartRequired(
      { role: 'relay', restart_required: true },
      null,
      null,
    ),
  ).toBe(true)
})

test('blank bind sends no update so an empty save asks for no restart', () => {
  expect(
    createRelaySettingsUpdate({ role: 'relay', admin_bind: '   ' }),
  ).toEqual({})
  expect(hasRelaySettingsChange({ role: 'relay', admin_bind: '' }, null)).toBe(
    false,
  )
})

test('worker link state stays a display label, never a page failure', () => {
  const disconnected = {
    relay_ready: true,
    restart_required: false,
    role: 'relay',
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
    worker: { connected: false, connected_workers: 0 },
  } as unknown as Parameters<typeof workerLinkState>[0]
  expect(workerLinkState(disconnected)).toBe('disconnected')
  expect(workerLinkState(null)).toBe('disconnected')
})

test('host page trims the bind before saving', () => {
  expect(
    createRelaySettingsUpdate({
      role: 'integrated',
      admin_bind: ' 127.0.0.1:8790 ',
    }),
  ).toEqual({ admin_bind: '127.0.0.1:8790' })
})

// Tester additions: the locale surface the host and relays pages render in —
// every host/role/restart/worker-state key must exist in both locales, and the
// two locale tables must carry identical key sets so no card renders blank.

const { messages } = await import('../src/i18n')

const HOST_PAGE_KEYS = [
  'relayHost',
  'relayHostLogin',
  'relayHostLoginDesc',
  'relayHostTokenPlaceholder',
  'relayHostRoleTitle',
  'relayHostSettingsTitle',
  'relayHostAdminBind',
  'relayHostAdminBindHelp',
  'relayHostStatusTitle',
  'relayHostRelayReady',
  'relayHostWorker',
  'relayRoleIntegrated',
  'relayRoleIntegratedDesc',
  'relayRoleWorker',
  'relayRoleWorkerDesc',
  'relayRoleRelay',
  'relayRoleRelayDesc',
  'relayRoleCurrent',
  'relayRolePending',
  'relayRoleSelected',
  'relayUseRole',
  'relayRoleSaved',
  'relayRestartRequired',
  'relayRestartRequiredDesc',
  'relayRestartNow',
  'relayRestartRequested',
  'relayWorkerDisconnected',
  'relayWorkerDisconnectedDesc',
  'relayWorkerUnavailable',
  'relayStatusConnected',
  'relayStatusDisconnected',
] as const

test('every host/role/restart key exists in both locales with text', () => {
  const missing = HOST_PAGE_KEYS.filter((key) => {
    const zh = messages['zh-CN'][key]
    const en = messages['en-US'][key]
    return (
      typeof zh !== 'string' ||
      zh.length === 0 ||
      typeof en !== 'string' ||
      en.length === 0
    )
  })
  expect(missing).toEqual([])
})

test('the zh-CN and en-US locale tables keep identical key sets', () => {
  const zh = Object.keys(messages['zh-CN']).sort()
  const en = Object.keys(messages['en-US']).sort()
  expect(zh).toEqual(en)
})

test('worker-state wording names host controls as unaffected', () => {
  // The scoped-unavailable contract is user-visible: the desc must say the
  // host role and relay settings stay operable, in both locales.
  expect(messages['zh-CN'].relayWorkerDisconnectedDesc).toContain('角色')
  expect(messages['en-US'].relayWorkerDisconnectedDesc).toContain(
    'roles still work',
  )
  expect(messages['zh-CN'].relayWorkerUnavailable).toContain('本机角色不受影响')
  expect(messages['en-US'].relayWorkerUnavailable).toContain(
    'host role is untouched',
  )
})
