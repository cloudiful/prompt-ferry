import { expect, test } from 'bun:test'

import { deferred, freshHarness } from './settings-actions.fixtures'

import { settingFormKey, settingKey } from '../src/stores/settings-actions'

/**
 * Which settings writes collide, and which stay independent.
 *
 * The page is a stack of unrelated forms, so the property under test is narrow:
 * a section that is already saving must not be written twice, and no other
 * section may be held up by it. What a surface *reports* when its own save was
 * declined is the completion contract, and it lives with the call sites.
 */

test('a section saving twice sends one write', async () => {
  const h = await freshHarness()

  const gate = deferred()
  h.api.setRequestContentLogging.mockImplementation(async () => {
    await gate.promise
    return { mode: 'off', raw_retention_days: 3 }
  })

  const first = h.actions.saveRequestContentLogging()
  expect(h.guard.isPending(settingFormKey('requestContentLogging'))).toBe(true)

  expect(await h.actions.saveRequestContentLogging()).toEqual({
    executed: false,
  })
  expect(h.api.setRequestContentLogging).toHaveBeenCalledTimes(1)

  gate.resolve()
  expect(await first).toMatchObject({ executed: true })
  expect(h.guard.isPending(settingKey('requestContentLogging'))).toBe(false)
})

test('saving one section leaves the others writable', async () => {
  const h = await freshHarness()

  const gate = deferred()
  h.api.setUsageRetention.mockImplementation(async () => {
    await gate.promise
    return {
      approval_retention_days: 90,
      content_retention_days: 3,
      metadata_retention_days: 90,
      raw_backend: 'object_store',
      raw_retention_days: 3,
      replay_enabled: true,
    }
  })

  const saving = h.actions.saveUsageRetention()
  expect(await h.actions.saveStreamDeltaBatching()).toMatchObject({
    executed: true,
  })
  expect(await h.actions.saveRelayIpWhitelist()).toMatchObject({
    executed: true,
  })
  expect(h.api.setStreamDeltaBatching).toHaveBeenCalledTimes(1)
  expect(h.api.setRelayIpWhitelist).toHaveBeenCalledTimes(1)

  gate.resolve()
  await saving
})

test('a declined write reaches neither the server nor the form', async () => {
  const h = await freshHarness()

  const gate = deferred()
  h.api.setRequestContentLogging.mockImplementation(async () => {
    await gate.promise
    return { mode: 'on', raw_retention_days: 9 }
  })

  const saving = h.actions.saveRequestContentLogging()
  await h.actions.saveRequestContentLogging()

  gate.resolve()
  await saving

  // Exactly one response was applied, and it is the one the server sent.
  expect(h.requestContentLogging.value).toEqual({
    mode: 'on',
    raw_retention_days: 9,
  })
})

test('a held key is released when the write fails', async () => {
  const h = await freshHarness()

  h.api.setRelayIpWhitelist.mockImplementationOnce(async () => {
    throw new Error('relay policy rejected')
  })

  await expect(h.actions.saveRelayIpWhitelist()).rejects.toThrow(
    'relay policy rejected',
  )
  expect(h.guard.isPending(settingFormKey('relayIpWhitelist'))).toBe(false)
  expect(await h.actions.saveRelayIpWhitelist()).toMatchObject({
    executed: true,
  })
})

test('the two retention forms share one key, because they are one value', async () => {
  const h = await freshHarness()

  // One stored `raw_retention_days`: the server rewrites the whole retention
  // record from a logging save, so a concurrent retention save is reverted
  // field for field. Keying them apart would only make that race reachable.
  expect(settingFormKey('requestContentLogging')).toBe(
    settingFormKey('usageRetention'),
  )

  const gate = deferred()
  h.api.setUsageRetention.mockImplementation(async () => {
    await gate.promise
    return {
      approval_retention_days: 90,
      content_retention_days: 3,
      metadata_retention_days: 90,
      raw_backend: 'object_store',
      raw_retention_days: 30,
      replay_enabled: true,
    }
  })

  const saving = h.actions.saveUsageRetention()
  expect(h.guard.isPending(settingFormKey('usageRetention'))).toBe(true)

  // The other form onto the same value is held off, not written concurrently.
  expect(await h.actions.saveRequestContentLogging()).toEqual({
    executed: false,
  })
  expect(h.api.setRequestContentLogging).not.toHaveBeenCalled()

  gate.resolve()
  await saving

  // Released, so the held-off form can be saved once its operator retries.
  expect(h.guard.isPending(settingFormKey('usageRetention'))).toBe(false)
  expect(await h.actions.saveRequestContentLogging()).toMatchObject({
    executed: true,
  })
  expect(h.api.setRequestContentLogging).toHaveBeenCalledTimes(1)
})

test('the cache alert and the storage settings are gated like any section', async () => {
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

test('a held whitelist autosave cannot overwrite an edit made while it runs', async () => {
  const h = await freshHarness()
  const form = h.pageStore as unknown as {
    modelRouteWhitelist: { enabled: boolean }
  }
  const gate = deferred()
  h.api.setModelRouteWhitelist.mockImplementationOnce(async () => {
    await gate.promise
    return { enabled: true }
  })
  form.modelRouteWhitelist.enabled = true
  const saving = h.actions.saveModelRouteWhitelist()
  form.modelRouteWhitelist.enabled = false
  gate.resolve()
  await saving
  expect(form.modelRouteWhitelist.enabled).toBe(false)
})

test('a held stream-batching save merges only fields left unchanged by the operator', async () => {
  const h = await freshHarness()
  const form = h.pageStore as unknown as {
    streamDeltaBatching: {
      enabled: boolean
      flush_window_ms: number
      max_buffer_chars: number
      max_buffer_bytes: number
      flush_on_line_break: boolean
      flush_on_sentence_end: boolean
    }
  }
  const gate = deferred()
  let submitted: unknown
  h.api.setStreamDeltaBatching.mockImplementationOnce(
    async (options: unknown) => {
      submitted = options
      await gate.promise
      return {
        enabled: true,
        flush_window_ms: 40,
        max_buffer_chars: 200,
        max_buffer_bytes: 2048,
        flush_on_line_break: false,
        flush_on_sentence_end: true,
      }
    },
  )

  form.streamDeltaBatching.enabled = true
  form.streamDeltaBatching.flush_window_ms = 60
  const saving = h.actions.saveStreamDeltaBatching()
  form.streamDeltaBatching.enabled = false
  form.streamDeltaBatching.flush_window_ms = 90
  gate.resolve()
  await saving

  expect(submitted).toMatchObject({
    body: { enabled: true, flush_window_ms: 60 },
  })
  expect(form.streamDeltaBatching.enabled).toBe(false)
  expect(form.streamDeltaBatching.flush_window_ms).toBe(90)
  expect(form.streamDeltaBatching.max_buffer_chars).toBe(200)
  expect(form.streamDeltaBatching.max_buffer_bytes).toBe(2048)
  expect(form.streamDeltaBatching.flush_on_line_break).toBe(false)
  expect(form.streamDeltaBatching.flush_on_sentence_end).toBe(true)
})

test('a storage save submits the payload it was given', async () => {
  const h = await freshHarness()

  let seen: unknown = null
  h.api.setRawObjectStore.mockImplementation(async (options: unknown) => {
    seen = options
    return { ...h.rawObjectStore.value }
  })

  const outcome = await h.actions.saveRawObjectStore({
    backend: 'local',
    local_dir: '/srv/raw',
  })

  expect(outcome).toMatchObject({ executed: true })
  expect(seen).toMatchObject({
    body: { backend: 'local', local_dir: '/srv/raw' },
  })
})
