import { expect, mock, test } from 'bun:test'
import {
  cacheAlertFormToRequest,
  cacheAlertToForm,
} from '../src/admin-mappers/forms/cache-alert'
import type { CacheAlertSettingsResponse } from '../src/generated/admin-api'

function responseFixture(
  overrides: Partial<CacheAlertSettingsResponse> = {},
): CacheAlertSettingsResponse {
  return {
    cooldown_minutes: 60,
    dingtalk_secret: '',
    dingtalk_webhook_url:
      'https://oapi.dingtalk.com/robot/send?access_token=test',
    enabled: true,
    has_dingtalk_secret: false,
    min_turns: 5,
    threshold: 0.2,
    window_minutes: 30,
    ...overrides,
  }
}

test('cacheAlertToForm copies the server presence flag and never the secret', () => {
  const configured = cacheAlertToForm(
    responseFixture({ has_dingtalk_secret: true }),
  )
  expect(configured.has_dingtalk_secret).toBe(true)
  // The response never carries the plaintext, so the form starts on "keep".
  expect(configured.secret).toEqual({ mode: 'keep' })

  const missing = cacheAlertToForm(
    responseFixture({ has_dingtalk_secret: false }),
  )
  expect(missing.has_dingtalk_secret).toBe(false)
})

test('cacheAlertFormToRequest never sends the derived presence flag', () => {
  const form = {
    ...cacheAlertToForm(responseFixture({ has_dingtalk_secret: true })),
    secret: { mode: 'replace' as const, value: '  SEC-test-secret  ' },
  }

  const replace = cacheAlertFormToRequest(form)
  expect('has_dingtalk_secret' in replace).toBe(false)
  expect(replace.dingtalk_secret).toBe('SEC-test-secret')

  const keep = cacheAlertFormToRequest({
    ...form,
    secret: { mode: 'keep' as const },
  })
  expect('has_dingtalk_secret' in keep).toBe(false)
  expect(keep.dingtalk_secret).toBe('')
})

const actualAdminApi = await import('../src/generated/admin-api')

const getCacheAlertSetting = mock(async () => ({
  data: responseFixture({ has_dingtalk_secret: false }),
}))
const setCacheAlertSetting = mock(async () => ({
  data: responseFixture({ has_dingtalk_secret: true }),
}))

// Keep every other generated export intact: `mock.module` is process-global and
// would otherwise break suites that import sibling SDK functions.
mock.module('../src/generated/admin-api', () => ({
  ...actualAdminApi,
  getCacheAlertSetting,
  setCacheAlertSetting,
}))

const { createCacheAlertStore } =
  await import('../src/stores/settings/cache-alert')

test('saveCacheAlert refreshes presence from the response, not the local action', async () => {
  const store = createCacheAlertStore()
  getCacheAlertSetting.mockClear()
  setCacheAlertSetting.mockClear()

  await store.refreshCacheAlert()
  expect(store.cacheAlert.value?.has_dingtalk_secret).toBe(false)

  const form = store.cacheAlert.value
  if (!form) throw new Error('cache alert form missing')
  form.secret = { mode: 'replace', value: 'SEC-test-secret' }

  await store.saveCacheAlert()

  expect(setCacheAlertSetting).toHaveBeenCalledTimes(1)
  const sent = setCacheAlertSetting.mock.calls[0]?.[0] as {
    body: Record<string, unknown>
  }
  expect('has_dingtalk_secret' in sent.body).toBe(false)
  expect(sent.body.dingtalk_secret).toBe('SEC-test-secret')

  // Presence now comes from the server response, with the input cleared.
  expect(store.cacheAlert.value?.has_dingtalk_secret).toBe(true)
  expect(store.cacheAlert.value?.secret).toEqual({ mode: 'keep' })
})
