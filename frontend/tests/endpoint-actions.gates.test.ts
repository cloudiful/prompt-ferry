import { beforeEach, expect, test } from 'bun:test'

import {
  deferred,
  endpointKey,
  freshHarness,
  holdPersist,
  holdRoutePersist,
  loadEndpointActions,
  routeKey,
  rows,
  type Harness,
} from './endpoint-actions.fixtures'

const { ENDPOINT_KEY_PREFIX, MODEL_ROUTE_KEY_PREFIX } =
  await loadEndpointActions()

/**
 * Which key excludes what.
 *
 * These cases are about the gate itself: which operations collide, which stay
 * independent, and how long a key is held. What an individual action does once
 * it owns the key is covered in `endpoint-actions.test.ts`.
 */

let h: Harness

beforeEach(() => {
  h = freshHarness()
})

test('a save persists once, holds its key through the reload, then releases', async () => {
  const gate = deferred()
  h = freshHarness(async () => {
    h.reloads.workspace += 1
    await gate.promise
  })
  const running = h.actions.saveEndpoint('endpoint-a', { name: 'renamed' })
  await Promise.resolve()
  await Promise.resolve()

  // The write is already done; the guard is still held because the refreshed
  // list has not landed yet.
  expect(h.persistCalls()).toBe(1)
  expect(h.api.persistEndpoint.mock.calls[0]?.[0]).toBe('endpoint-a')
  expect(h.guard.isPending(endpointKey('endpoint-a'))).toBe(true)
  expect(h.guard.pendingAction(endpointKey('endpoint-a'))).toBe('save')
  expect(await h.actions.removeEndpoint('endpoint-a')).toEqual({
    executed: false,
  })

  gate.resolve()
  await expect(running).resolves.toMatchObject({ executed: true })
  expect(h.guard.isPending(endpointKey('endpoint-a'))).toBe(false)
  expect(h.reloads.workspace).toBe(1)
})

test('save, delete, and toggle of one record share a gate', async () => {
  const gate = deferred()
  holdPersist(async () => {
    await gate.promise
  })
  const saving = h.actions.saveEndpoint('endpoint-a', { name: 'renamed' })

  expect(await h.actions.removeEndpoint('endpoint-a')).toEqual({
    executed: false,
  })
  expect(await h.actions.toggleEndpointEnabled('endpoint-a', false)).toEqual({
    executed: false,
  })
  // A refused operation reaches neither the API nor the reload.
  expect(h.api.deleteEndpointById).not.toHaveBeenCalled()
  expect(h.api.updateEndpointEnabled).not.toHaveBeenCalled()
  expect(h.reloads.workspace).toBe(0)

  gate.resolve()
  await saving
})

test('another record stays writable while one is being written', async () => {
  const gate = deferred()
  holdPersist(async () => {
    await gate.promise
  })
  const saving = h.actions.saveEndpoint('endpoint-a', { name: 'renamed' })

  expect(
    await h.actions.saveEndpoint('endpoint-b', { name: 'other' }),
  ).toMatchObject({ executed: true })
  expect(h.persistCalls()).toBe(2)
  expect(rows(h.guard, ENDPOINT_KEY_PREFIX)).toEqual({ 'endpoint-a': 'save' })

  gate.resolve()
  await saving
})

test('a create is gated under its own key', async () => {
  const gate = deferred()
  holdPersist(async () => {
    await gate.promise
  })
  const creating = h.actions.saveEndpoint(null, { name: 'fresh' })

  expect(endpointKey(null)).toBe(`${ENDPOINT_KEY_PREFIX}:create`)
  expect(h.guard.isPending(endpointKey(null))).toBe(true)
  expect(await h.actions.saveEndpoint(null, { name: 'again' })).toEqual({
    executed: false,
  })
  expect(rows(h.guard, ENDPOINT_KEY_PREFIX)).toEqual({ create: 'save' })

  gate.resolve()
  await creating
})

test('OAuth writes share the endpoint key with save while other endpoints stay independent', async () => {
  const gate = deferred()
  let completed = false
  const running = h.actions.runEndpointOAuthAction(
    'endpoint-a',
    async () => {
      await gate.promise
      completed = true
    },
    'oauth-device-start',
  )
  await Promise.resolve()

  expect(h.guard.pendingAction(endpointKey('endpoint-a'))).toBe(
    'oauth-device-start',
  )
  expect(
    await h.actions.saveEndpoint('endpoint-a', { name: 'blocked' }),
  ).toEqual({
    executed: false,
  })
  expect(
    await h.actions.saveEndpoint('endpoint-b', { name: 'independent' }),
  ).toMatchObject({ executed: true })
  expect(completed).toBe(false)

  gate.resolve()
  await expect(running).resolves.toMatchObject({ executed: true })
  expect(completed).toBe(true)
  expect(h.guard.isPending(endpointKey('endpoint-a'))).toBe(false)
})

test('a probe has its own prefix: it neither blocks a write nor repeats', async () => {
  const gate = deferred()
  h.api.runEndpointTest.mockImplementationOnce(async () => {
    await gate.promise
    return { duration_ms: 7, message: 'slow', ok: true, status: 200 }
  })
  const probing = h.actions.runEndpointTest('endpoint-a')
  await Promise.resolve()

  // A write and a probe of another record still run; a repeat probe does not.
  expect(
    await h.actions.saveEndpoint('endpoint-a', { name: 'x' }),
  ).toMatchObject({ executed: true })
  expect(await h.actions.runEndpointTest('endpoint-b')).toMatchObject({
    executed: true,
  })
  expect(await h.actions.runEndpointTest('endpoint-a')).toEqual({
    executed: false,
  })

  gate.resolve()
  await probing
  expect(h.api.runEndpointTest).toHaveBeenCalledTimes(2)
})

test('model routes share one gate and reload only the routing list', async () => {
  const gate = deferred()
  holdRoutePersist(async () => {
    await gate.promise
  })
  const saving = h.actions.saveModelRoute('rule-a', { model_pattern: 'gpt-*' })

  expect(await h.actions.removeModelRoute('rule-a')).toEqual({
    executed: false,
  })
  expect(
    await h.actions.saveModelRoute('rule-b', { model_pattern: 'o3' }),
  ).toMatchObject({ executed: true })
  expect(h.reloads).toEqual({ modelroute: 1, workspace: 0 })
  expect(rows(h.guard, MODEL_ROUTE_KEY_PREFIX)).toEqual({ 'rule-a': 'save' })

  gate.resolve()
  await saving
  expect(h.guard.isPending(routeKey('rule-a'))).toBe(false)
})

test('a model route toggle shares its record gate with delete', async () => {
  const gate = deferred()
  h.api.updateModelRouteEnabled.mockImplementationOnce(async (value) => {
    await gate.promise
    return { ...value, enabled: false }
  })
  const toggling = h.actions.toggleModelRouteEnabled('rule-a', false)

  expect(await h.actions.removeModelRoute('rule-a')).toEqual({
    executed: false,
  })
  expect(h.api.deleteModelRouteById).not.toHaveBeenCalled()

  gate.resolve()
  await toggling
  expect(h.guard.isPending(routeKey('rule-a'))).toBe(false)
})
