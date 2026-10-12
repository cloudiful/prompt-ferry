import { expect, test } from 'bun:test'

import { deferred, freshHarness } from './settings-actions.fixtures'

import {
  settingFormKey,
  settingKey,
  SETTING_FORM_SECTIONS,
} from '../src/stores/settings-actions'

/**
 * Which settings writes collide, and which stay independent.
 *
 * The gate is keyed by the stored resource, not by the form that writes it. One
 * pair of forms shares a resource — they are the same value on the server — and
 * everything else is independent. These cases are about the gate itself; what a
 * surface reports when its own save was declined is
 * `settings-actions.lifecycle.test.ts`.
 */

test('the retention forms are declared as one resource', async () => {
  const h = await freshHarness()

  // Declared, not merely coincidental: the form-to-resource map is the single
  // place the shared value is expressed.
  expect(SETTING_FORM_SECTIONS.requestContentLogging).toBe('rawRetention')
  expect(SETTING_FORM_SECTIONS.usageRetention).toBe('rawRetention')
  expect(settingFormKey('requestContentLogging')).toBe(
    settingFormKey('usageRetention'),
  )
  expect(h.guard.isPending(settingKey('rawRetention'))).toBe(false)
})

test('a held retention write declines the other form and sends nothing', async () => {
  const h = await freshHarness()

  const gate = deferred()
  h.api.setUsageRetention.mockImplementation(async () => {
    await gate.promise
    return { ...h.refs.usageRetention.value }
  })

  const saving = h.actions.saveUsageRetention()
  expect(h.guard.isPending(settingFormKey('usageRetention'))).toBe(true)

  // The other form onto the same stored value is held off, not raced.
  expect(await h.actions.saveRequestContentLogging()).toEqual({
    executed: false,
  })
  expect(h.api.setRequestContentLogging).not.toHaveBeenCalled()

  gate.resolve()
  await saving
})

test('the key is released for the other form once the write settles', async () => {
  const h = await freshHarness()

  const gate = deferred()
  h.api.setUsageRetention.mockImplementation(async () => {
    await gate.promise
    return { ...h.refs.usageRetention.value }
  })

  const saving = h.actions.saveUsageRetention()
  await h.actions.saveRequestContentLogging()

  gate.resolve()
  await saving

  expect(h.guard.isPending(settingFormKey('requestContentLogging'))).toBe(false)
  expect(await h.actions.saveRequestContentLogging()).toMatchObject({
    executed: true,
  })
  expect(h.api.setRequestContentLogging).toHaveBeenCalledTimes(1)
})

test('a shared key is released when the write fails', async () => {
  const h = await freshHarness()

  h.api.setUsageRetention.mockImplementation(async () => {
    throw new Error('retention rejected')
  })

  await expect(h.actions.saveUsageRetention()).rejects.toThrow(
    'retention rejected',
  )
  expect(h.guard.isPending(settingFormKey('usageRetention'))).toBe(false)
  expect(await h.actions.saveRequestContentLogging()).toMatchObject({
    executed: true,
  })
})

test('the other resources are independent of the retention pair', async () => {
  const h = await freshHarness()

  const gate = deferred()
  h.api.setUsageRetention.mockImplementation(async () => {
    await gate.promise
    return { ...h.refs.usageRetention.value }
  })

  const saving = h.actions.saveUsageRetention()
  for (const save of [
    () => h.actions.saveStreamDeltaBatching(),
    () => h.actions.saveRelayIpWhitelist(),
    () => h.actions.saveModelRouteWhitelist(),
    () => h.actions.saveLlmReview(),
    () => h.actions.saveCacheAlert(),
  ]) {
    expect(await save()).toMatchObject({ executed: true })
  }
  expect(h.api.setStreamDeltaBatching).toHaveBeenCalledTimes(1)
  expect(h.api.setRelayIpWhitelist).toHaveBeenCalledTimes(1)
  expect(h.api.setModelRouteWhitelist).toHaveBeenCalledTimes(1)
  expect(h.api.setLlmReviewSetting).toHaveBeenCalledTimes(1)
  expect(h.cacheAlertSaves.count).toBe(1)

  gate.resolve()
  await saving
})

test('only the form that claimed the shared key is the origin', async () => {
  const h = await freshHarness()

  const gate = deferred()
  h.api.setUsageRetention.mockImplementation(async () => {
    await gate.promise
    return { ...h.refs.usageRetention.value }
  })

  const saving = h.actions.saveUsageRetention()
  // The form that is actually writing is the origin, so it is the one that
  // spins; the other is merely waiting on the same value.
  expect(h.actions.pendingRetentionForm.value).toBe('usageRetention')

  await h.actions.saveRequestContentLogging()
  // A declined call must not steal the spinner from the one holding the key.
  expect(h.actions.pendingRetentionForm.value).toBe('usageRetention')

  gate.resolve()
  await saving
  expect(h.actions.pendingRetentionForm.value).toBe(null)
})

test('the cache alert and the storage settings are gated like any resource', async () => {
  const h = await freshHarness()

  const gate = deferred()
  h.api.setRawObjectStore.mockImplementation(async () => {
    await gate.promise
    return { ...h.rawObjectStore.value }
  })

  const saving = h.actions.saveRawObjectStore({ backend: 'local' })
  expect(h.guard.isPending(settingKey('storage'))).toBe(true)

  expect(await h.actions.saveCacheAlert()).toMatchObject({ executed: true })
  expect(h.cacheAlertSaves.count).toBe(1)

  expect(
    await h.actions.saveRawObjectStore({ backend: 'disabled' }),
  ).toMatchObject({ executed: false })
  expect(h.cacheAlertSaves.count).toBe(1)

  gate.resolve()
  await saving
})
