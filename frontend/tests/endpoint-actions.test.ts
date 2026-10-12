import { beforeEach, expect, test } from 'bun:test'

import type { EndpointRequest } from '../src/generated/admin-api'
import {
  deferred,
  freshHarness,
  holdPersist,
  loadEndpointActions,
  rows,
  type Harness,
} from './endpoint-actions.fixtures'

const { ENDPOINT_KEY_PREFIX, pendingRowsById } = await loadEndpointActions()

/**
 * What an action does once it owns its key: the payload it sends, the row state
 * it holds while it runs, what a failure leaves behind, and the per-row view the
 * surfaces read. Which operations exclude which is covered in
 * `endpoint-actions.gates.test.ts`.
 */

let h: Harness

beforeEach(() => {
  h = freshHarness()
})

test('the payload is captured before the first await', async () => {
  const gate = deferred()
  holdPersist(async () => {
    await gate.promise
  })
  const body = {
    name: 'first',
    active_windows: [{ days: [1, 2], start: '09:00' }],
  } as unknown as EndpointRequest

  const saving = h.actions.saveEndpoint('endpoint-a', body)

  // Both the top level and a nested list the dialog owns stay reachable.
  body.name = 'edited while saving'
  ;(
    body as unknown as { active_windows: { start: string }[] }
  ).active_windows[0]!.start = '23:00'

  gate.resolve()
  await saving

  expect(h.api.persistEndpoint.mock.calls[0]?.[1]).toEqual({
    active_windows: [{ days: [1, 2], start: '09:00' }],
    name: 'first',
  })
})

test('a toggle holds its row flag for the whole write, and a failure clears it', async () => {
  const gate = deferred()
  h.api.updateEndpointEnabled.mockImplementationOnce(async (value) => {
    await gate.promise
    return { ...value, enabled: false }
  })
  const toggling = h.actions.toggleEndpointEnabled('endpoint-a', false)
  await Promise.resolve()
  expect(h.endpoints.togglingId.value).toBe('endpoint-a')

  gate.resolve()
  await toggling

  expect(h.endpoints.togglingId.value).toBe('')
  expect(h.endpoints.items.value[0]?.enabled).toBe(false)
  // Only the toggled record changed.
  expect(h.endpoints.items.value).toHaveLength(2)

  h.api.updateEndpointEnabled.mockImplementationOnce(async () => {
    throw new Error('rejected')
  })
  await expect(
    h.actions.toggleEndpointEnabled('endpoint-a', true),
  ).rejects.toThrow('rejected')

  expect(h.endpoints.togglingId.value).toBe('')
  expect(h.endpoints.items.value[0]?.enabled).toBe(false)
  expect(h.guard.isPending('endpoint:endpoint-a')).toBe(false)
})

test('a toggle of a record that is not loaded fails without a request', async () => {
  h.endpoints.items.value = []

  await expect(
    h.actions.toggleEndpointEnabled('endpoint-missing', false),
  ).rejects.toThrow('Endpoint not found: endpoint-missing')
  expect(h.api.updateEndpointEnabled).not.toHaveBeenCalled()
})

test('a probe records its result and a failed probe leaves none', async () => {
  const gate = deferred()
  h.api.runEndpointTest.mockImplementationOnce(async () => {
    await gate.promise
    return { duration_ms: 7, message: 'slow', ok: true, status: 200 }
  })
  const probing = h.actions.runEndpointTest('endpoint-a')
  await Promise.resolve()
  expect(h.endpoints.testingId.value).toBe('endpoint-a')

  gate.resolve()
  await probing

  expect(h.endpoints.testingId.value).toBe('')
  expect(h.endpoints.testResults.value['endpoint-a']?.duration_ms).toBe(7)
  expect(h.guard.isPending('networktest:endpoint:endpoint-a')).toBe(false)

  h.api.runEndpointTest.mockImplementationOnce(async () => {
    throw new Error('probe failed')
  })
  await expect(h.actions.runEndpointTest('endpoint-a')).rejects.toThrow(
    'probe failed',
  )

  expect(h.endpoints.testingId.value).toBe('')
  // A failure records nothing, so the earlier result is left exactly as it was.
  expect(h.endpoints.testResults.value['endpoint-a']).toEqual({
    duration_ms: 7,
    message: 'slow',
    ok: true,
    status: 200,
  })
  expect(h.guard.isPending('networktest:endpoint:endpoint-a')).toBe(false)
})

test('a model route probe records its result without touching the row', async () => {
  const gate = deferred()
  h.api.runModelRouteProbe.mockImplementationOnce(async () => {
    await gate.promise
    return { duration_ms: 3, message: 'ok', ok: true, status: 200 }
  })
  const probing = h.actions.runModelRouteTest('rule-a')
  await Promise.resolve()
  expect(h.modelRoutes.testingId.value).toBe('rule-a')

  gate.resolve()
  await probing

  expect(h.modelRoutes.testingId.value).toBe('')
  expect(h.modelRoutes.testResults.value['rule-a']?.ok).toBe(true)
  expect(h.guard.isPending('networktest:modelroute:rule-a')).toBe(false)
  // A probe is not a write, so it never appears as a gated row.
  expect(rows(h.guard, 'modelroute')).toEqual({})
})

test('the row projection reports the holding action and skips foreign keys', async () => {
  const foreign = new Map<string, string | undefined>([
    ['endpoint:a', 'save'],
    ['networktest:endpoint:a', 'test'],
    ['endpointish:b', 'save'],
  ])
  expect(pendingRowsById(foreign, ENDPOINT_KEY_PREFIX)).toEqual({ a: 'save' })
  // An unlabelled key still gates its row.
  expect(
    pendingRowsById(new Map([['endpoint:a', undefined]]), ENDPOINT_KEY_PREFIX),
  ).toEqual({ a: '' })

  const gate = deferred()
  h.api.deleteEndpointById.mockImplementationOnce(async () => {
    await gate.promise
  })
  const deleting = h.actions.removeEndpoint('endpoint-a')

  // The row's other write controls are gated but not spinning, which is what
  // the label is for.
  expect(await h.actions.toggleEndpointEnabled('endpoint-a', false)).toEqual({
    executed: false,
  })
  expect(rows(h.guard, ENDPOINT_KEY_PREFIX)).toEqual({ 'endpoint-a': 'delete' })

  gate.resolve()
  await deleting
})
