import { expect, test } from 'bun:test'
import {
  deferred,
  freshRedactionStore,
  previewRedaction,
  setRedactionSetting,
} from './redaction-actions.fixtures'

const wait = (promise: Promise<unknown>) => promise

test('a held redaction save snapshots its draft and refuses a duplicate', async () => {
  const store = freshRedactionStore()
  store.setTarget('user', 17)
  store.config.enabled = true

  const gate = deferred<void>()
  let submitted: unknown
  setRedactionSetting.mockImplementation(async (options: unknown) => {
    submitted = options
    await gate.promise
    const request = options as { body: typeof store.config; query: object }
    return { config: request.body, scope: 'user', user_id: 17 }
  })

  const saving = store.save()
  expect(store.saving).toBe(true)
  expect(store.isPending('redaction:rules')).toBe(true)

  store.config.enabled = false
  expect(store.config.enabled).toBe(false)
  await expect(store.save()).resolves.toEqual({ executed: false })
  expect(setRedactionSetting).toHaveBeenCalledTimes(1)

  gate.resolve()
  await expect(saving).resolves.toMatchObject({ executed: true })
  expect(submitted).toMatchObject({
    body: { enabled: true },
    query: { scope: 'user', user_id: 17 },
  })
  expect(store.config.enabled).toBe(false)
  expect(store.saving).toBe(false)
  expect(store.isPending('redaction:rules')).toBe(false)
})

test('a rejected redaction save releases its key for a retry', async () => {
  const store = freshRedactionStore()
  setRedactionSetting.mockImplementationOnce(async () => {
    throw new Error('redaction write rejected')
  })

  await expect(store.save()).rejects.toThrow('redaction write rejected')
  expect(store.saving).toBe(false)
  expect(store.isPending('redaction:rules')).toBe(false)

  await expect(store.save()).resolves.toMatchObject({ executed: true })
  expect(setRedactionSetting).toHaveBeenCalledTimes(2)
})

test('preview and save use independent keys and stale previews are discarded', async () => {
  const store = freshRedactionStore()
  store.previewText = 'before'
  const gate = deferred<void>()
  let previewBody: unknown
  previewRedaction.mockImplementation(async (options: unknown) => {
    previewBody = options
    await gate.promise
    return {
      preview: {
        applied_replacements: [],
        findings: [],
        redacted_text: 'old result',
        stats: {
          applied_replacements: 0,
          dropped_findings: 0,
          llm_candidates_accepted: 0,
          llm_candidates_rejected: 0,
          llm_configured: false,
          llm_request_failed: false,
          total_findings: 0,
        },
      },
    }
  })

  const preview = store.runPreview()
  expect(store.previewing).toBe(true)
  expect(store.isPending('redaction:preview')).toBe(true)
  store.previewText = 'after'
  await expect(store.runPreview()).resolves.toEqual({ executed: false })

  await expect(store.save()).resolves.toMatchObject({ executed: true })
  expect(store.saving).toBe(false)
  expect(store.previewing).toBe(true)

  gate.resolve()
  await expect(preview).resolves.toMatchObject({ executed: true })
  expect(previewBody).toMatchObject({ body: { text: 'before' } })
  expect(store.previewResult).toBeNull()
  expect(store.previewing).toBe(false)
})

test('a save response for a previous target cannot replace the current draft', async () => {
  const store = freshRedactionStore()
  store.setTarget('user', 19)
  store.config.enabled = true
  const gate = deferred<void>()
  setRedactionSetting.mockImplementation(async (options: unknown) => {
    await gate.promise
    const request = options as { body: typeof store.config }
    return { config: request.body, scope: 'user', user_id: 19 }
  })

  const saving = store.save()
  store.setTarget('global')
  store.config.enabled = false
  gate.resolve()
  await wait(saving)

  expect(setRedactionSetting.mock.calls[0]?.[0]).toMatchObject({
    query: { scope: 'user', user_id: 19 },
  })
  expect(store.scope).toBe('global')
  expect(store.config.enabled).toBe(false)
})
