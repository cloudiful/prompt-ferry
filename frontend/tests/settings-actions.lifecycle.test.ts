import { expect, mock, test } from 'bun:test'
import { computed } from 'vue'

import { deferred, freshHarness } from './settings-actions.fixtures'

import type { SettingsTab } from '../src/models/settings'

/**
 * What a settings surface reports when its own save was declined.
 *
 * The guard refusing a call is not a failure, but it must not look like a
 * success either. A declined autosave leaves its form dirty and retries once
 * the key is released; a declined button save reports nothing at all. These are
 * the properties an operator cannot check by reading the code, because getting
 * them wrong looks exactly like working software.
 */

const toastAdds = mock((_toast: unknown) => {})

mock.module('@nuxt/ui/composables', () => ({
  useToast: () => ({ add: toastAdds }),
}))

// Imported after the toast is mocked, so the notifier picks it up.
const { useSettingsPageActions } =
  await import('../src/composables/useSettingsPageActions')

const sleep = (ms: number): Promise<void> =>
  new Promise((resolve) => setTimeout(resolve, ms))

/** Past the longest autosave debounce on the settings page. */
const DEBOUNCE = 340

function pageFor(h: Awaited<ReturnType<typeof freshHarness>>) {
  return useSettingsPageActions({
    activeSection: computed(() => 'requests' as SettingsTab),
    store: h.pageStore as never,
    t: (key: string) => key,
  })
}

test('held shared retention autosave retries after release', async () => {
  const h = await freshHarness()
  const gate = deferred()
  let submitted: unknown
  toastAdds.mockClear()
  h.api.setUsageRetention.mockImplementation(async () => {
    await gate.promise
    return { ...h.refs.usageRetention.value }
  })
  h.api.setRequestContentLogging.mockImplementation(
    async (options: unknown) => {
      submitted = options
      const body = (options as { body: Record<string, unknown> }).body
      return { ...body } as never
    },
  )

  const page = pageFor(h)
  page.adoptLoadedSettings()

  h.refs.usageRetention.value.raw_retention_days = 5
  await sleep(DEBOUNCE)
  expect(h.api.setUsageRetention).toHaveBeenCalledTimes(1)

  // The two forms target the same raw_retention_days field. The second form's
  // in-flight edit must neither race the request nor be overwritten by the
  // first response's shared-field projection.
  h.refs.requestContentLogging.value.mode = 'on'
  h.refs.requestContentLogging.value.raw_retention_days = 7
  await sleep(DEBOUNCE)
  expect(h.api.setRequestContentLogging).not.toHaveBeenCalled()
  expect(h.refs.requestContentLogging.value.raw_retention_days).toBe(7)

  gate.resolve()
  await sleep(DEBOUNCE)
  expect(h.api.setRequestContentLogging).toHaveBeenCalledTimes(1)
  expect(submitted).toMatchObject({
    body: { mode: 'on', raw_retention_days: 7 },
  })
  expect(toastAdds).toHaveBeenCalledTimes(0)

  page.dispose()
})

test('an edit queued during whitelist autosave survives and is submitted after release', async () => {
  const h = await freshHarness()
  const form = h.pageStore as unknown as {
    modelRouteWhitelist: { enabled: boolean }
  }
  const gate = deferred()
  const sent: boolean[] = []
  h.api.setModelRouteWhitelist.mockImplementation(async (options: unknown) => {
    const enabled = (options as { body: { enabled: boolean } }).body.enabled
    sent.push(enabled)
    if (sent.length === 1) await gate.promise
    return { enabled }
  })
  const page = pageFor(h)
  page.adoptLoadedSettings()
  try {
    form.modelRouteWhitelist.enabled = true
    await sleep(DEBOUNCE)
    expect(sent).toEqual([true])
    form.modelRouteWhitelist.enabled = false
    await sleep(DEBOUNCE)
    gate.resolve()
    await sleep(DEBOUNCE)
    expect(sent).toEqual([true, false])
    expect(form.modelRouteWhitelist.enabled).toBe(false)
  } finally {
    gate.resolve()
    page.dispose()
  }
})

test('stream-batching autosave preserves an edit and adopts confirmed normalization', async () => {
  const h = await freshHarness()
  const form = h.pageStore as unknown as {
    streamDeltaBatching: {
      enabled: boolean
      flush_window_ms: number
      max_buffer_chars: number
    }
  }
  const gate = deferred()
  const submitted: Array<{
    enabled: boolean
    flush_window_ms: number
    max_buffer_chars: number
  }> = []
  h.api.setStreamDeltaBatching.mockImplementation(async (options: unknown) => {
    const body = (options as { body: (typeof submitted)[number] }).body
    submitted.push({ ...body })
    if (submitted.length === 1) await gate.promise
    return {
      ...body,
      max_buffer_chars: submitted.length === 1 ? 250 : 300,
    }
  })
  const page = pageFor(h)
  page.adoptLoadedSettings()
  try {
    form.streamDeltaBatching.enabled = true
    form.streamDeltaBatching.flush_window_ms = 60
    await sleep(DEBOUNCE)
    expect(submitted.map((body) => body.enabled)).toEqual([true])

    form.streamDeltaBatching.enabled = false
    form.streamDeltaBatching.flush_window_ms = 90
    await sleep(DEBOUNCE)
    expect(submitted.map((body) => body.enabled)).toEqual([true])

    gate.resolve()
    await sleep(DEBOUNCE * 2)
    expect(submitted.map((body) => body.enabled)).toEqual([true, false])
    expect(submitted.map((body) => body.flush_window_ms)).toEqual([60, 90])
    expect(submitted.map((body) => body.max_buffer_chars)).toEqual([160, 250])
    expect(form.streamDeltaBatching.enabled).toBe(false)
    expect(form.streamDeltaBatching.flush_window_ms).toBe(90)
    expect(form.streamDeltaBatching.max_buffer_chars).toBe(300)
  } finally {
    gate.resolve()
    page.dispose()
  }
})

test('a failed stream save preserves and retries the latest draft', async () => {
  const h = await freshHarness()
  const form = h.pageStore as unknown as {
    streamDeltaBatching: { enabled: boolean; flush_window_ms: number }
  }
  const gate = deferred()
  const submitted: Array<{ enabled: boolean; flush_window_ms: number }> = []
  toastAdds.mockClear()
  h.api.setStreamDeltaBatching.mockImplementation(async (options: unknown) => {
    const body = (options as { body: Record<string, unknown> }).body
    submitted.push({
      enabled: body.enabled as boolean,
      flush_window_ms: body.flush_window_ms as number,
    })
    if (submitted.length === 1) {
      await gate.promise
      throw new Error('stream settings rejected')
    }
    return body as never
  })
  const page = pageFor(h)
  page.adoptLoadedSettings()
  try {
    form.streamDeltaBatching.enabled = true
    await sleep(DEBOUNCE)
    expect(submitted).toEqual([{ enabled: true, flush_window_ms: 50 }])

    form.streamDeltaBatching.enabled = false
    form.streamDeltaBatching.flush_window_ms = 90
    await sleep(DEBOUNCE)
    gate.resolve()
    await sleep(20)

    expect(submitted).toEqual([
      { enabled: true, flush_window_ms: 50 },
      { enabled: false, flush_window_ms: 90 },
    ])
    expect(form.streamDeltaBatching).toMatchObject({
      enabled: false,
      flush_window_ms: 90,
    })
    expect(toastAdds).toHaveBeenCalledTimes(1)
    expect(toastAdds.mock.calls[0][0]).toMatchObject({ color: 'error' })
  } finally {
    gate.resolve()
    page.dispose()
  }
})

test('a declined button save reports nothing, and the real one reports once', async () => {
  const h = await freshHarness()
  const gate = deferred()
  h.api.setRelayIpWhitelist.mockImplementation(async () => {
    await gate.promise
    return {}
  })

  const page = pageFor(h)
  toastAdds.mockClear()

  const saving = page.saveRelayIpWhitelist()
  // The duplicate trigger: the guard collapses it, so it earns no toast.
  await page.saveRelayIpWhitelist()
  expect(toastAdds).toHaveBeenCalledTimes(0)

  gate.resolve()
  await saving
  expect(toastAdds).toHaveBeenCalledTimes(1)
  expect(toastAdds.mock.calls[0][0]).toMatchObject({ color: 'success' })
})

test('a failed button save reports the failure once', async () => {
  const h = await freshHarness()
  h.api.setLlmReviewSetting.mockImplementation(async () => {
    throw new Error('review settings rejected')
  })

  const page = pageFor(h)
  toastAdds.mockClear()

  await page.saveLlmReview()
  expect(toastAdds).toHaveBeenCalledTimes(1)
  expect(toastAdds.mock.calls[0][0]).toMatchObject({ color: 'error' })
})

test('an edit made while a shared-field write is in flight is not submitted with it', async () => {
  const h = await freshHarness()
  const gate = deferred()
  let submitted: unknown = null
  h.api.setUsageRetention.mockImplementation(async (options: unknown) => {
    submitted = options
    await gate.promise
    return { ...h.refs.usageRetention.value }
  })

  h.refs.usageRetention.value.raw_retention_days = 5
  const saving = h.actions.saveUsageRetention()

  // The shared field changes again before the request comes back. The payload
  // was captured at the claim, so this value is not sent and is not lost: it is
  // still dirty, and the next autosave carries it.
  h.refs.usageRetention.value.raw_retention_days = 99
  gate.resolve()
  await saving

  expect(submitted).toMatchObject({ body: { raw_retention_days: 5 } })
  expect(h.refs.usageRetention.value.raw_retention_days).toBe(99)
})
