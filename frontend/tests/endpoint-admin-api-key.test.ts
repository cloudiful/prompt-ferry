import { expect, test } from 'bun:test'
import {
  clearAdminApiKeyOutsideOpenAi,
  createEmptyEndpointForm,
  duplicateEndpointApiKeyLabelIndexes,
  endpointFormToRequest,
  endpointToForm,
  resolveAdminApiKeyRequest,
} from '../src/admin-mappers/forms/endpoint'
import type {
  EndpointApiKey,
  ProviderEndpoint,
} from '../src/generated/admin-api'
import { messages } from '../src/i18n'
import { endpointOpenAiMessages } from '../src/i18n/modules/endpoints.openai'
import type { EndpointApiKeyForm, EndpointForm } from '../src/models'
import { isOrganizationUsageEligible } from '../src/models/endpoints/quota'

function endpointFixture(
  overrides: Partial<ProviderEndpoint> = {},
): ProviderEndpoint {
  return {
    api_keys: [],
    base_url: 'https://api.openai.com',
    created_at: '2026-09-27T00:00:00Z',
    enabled: true,
    endpoint_id: 'endpoint-openai',
    key_lb_enabled: false,
    mcp_enabled: false,
    name: 'openai',
    native_api: 'responses',
    native_api_source: 'manual',
    provider: 'openai',
    scope: 'admin',
    updated_at: '2026-09-27T00:00:00Z',
    ...overrides,
  }
}

function apiKeyFixture(
  overrides: Partial<EndpointApiKey> = {},
): EndpointApiKey {
  return {
    created_at: '2026-09-27T00:00:00Z',
    enabled: true,
    endpoint_id: 'endpoint-openai',
    key_id: 'key-a',
    key_label: 'primary',
    position: 0,
    updated_at: '2026-09-27T00:00:00Z',
    ...overrides,
  }
}

function formWith(overrides: Partial<EndpointForm>): EndpointForm {
  return { ...createEmptyEndpointForm(), provider: 'openai', ...overrides }
}

function savedKey(
  keyId: string,
  overrides: Partial<EndpointApiKeyForm> = {},
): EndpointApiKeyForm {
  return {
    key_label: '',
    api_key: '',
    has_saved_key: true,
    enabled: true,
    key_id: keyId,
    ...overrides,
  }
}

test('new endpoint forms never carry an Admin API Key by default', () => {
  const form = createEmptyEndpointForm()
  expect(form.admin_api_key).toBe('')
  expect(form.has_admin_api_key).toBe(false)
  expect(form.admin_api_key_clear).toBe(false)
})

test('endpointToForm reports presence without echoing the secret', () => {
  const saved = endpointToForm(
    endpointFixture({ has_admin_api_key: true }) as ProviderEndpoint,
  )
  expect(saved.admin_api_key).toBe('')
  expect(saved.has_admin_api_key).toBe(true)
  expect(saved.admin_api_key_clear).toBe(false)

  const none = endpointToForm(endpointFixture())
  expect(none.has_admin_api_key).toBe(false)
})

test('endpointFormToRequest applies the OpenAI save/keep/clear contract', () => {
  // Replace: a typed value is sent verbatim.
  expect(
    endpointFormToRequest(formWith({ admin_api_key: ' sk-admin-1 ' }))
      .admin_api_key,
  ).toBe('sk-admin-1')
  // Keep: empty + a saved key omits the field so PATCH keeps the stored value.
  expect(
    endpointFormToRequest(
      formWith({ has_admin_api_key: true, admin_api_key: '' }),
    ).admin_api_key,
  ).toBeUndefined()
  // Unset: empty + no saved key also omits it.
  expect(
    endpointFormToRequest(formWith({ admin_api_key: '   ' })).admin_api_key,
  ).toBeUndefined()
  // Clear: an explicit clear sends the empty-string sentinel.
  expect(
    endpointFormToRequest(
      formWith({ has_admin_api_key: true, admin_api_key_clear: true }),
    ).admin_api_key,
  ).toBe('')
  // Clear wins over a leftover typed value.
  expect(
    endpointFormToRequest(
      formWith({ admin_api_key: 'sk-typed', admin_api_key_clear: true }),
    ).admin_api_key,
  ).toBe('')
})

test('non-OpenAI endpoints never submit an Admin API Key', () => {
  for (const provider of ['generic', 'minimax', 'deepseek'] as const) {
    const form = formWith({ provider, admin_api_key: 'sk-should-not-send' })
    expect(resolveAdminApiKeyRequest(form)).toBeUndefined()
    expect(endpointFormToRequest(form).admin_api_key).toBeUndefined()
  }
})

test('switching provider away from OpenAI drops the typed Admin API Key', () => {
  const form = formWith({
    admin_api_key: 'sk-typed',
    admin_api_key_clear: true,
    has_admin_api_key: true,
  })
  form.provider = 'generic'
  clearAdminApiKeyOutsideOpenAi(form)
  expect(form.admin_api_key).toBe('')
  expect(form.admin_api_key_clear).toBe(false)
  // The response-side indicator stays server truth until the next save.
  expect(form.has_admin_api_key).toBe(true)

  form.provider = 'openai'
  clearAdminApiKeyOutsideOpenAi(form)
  expect(form.admin_api_key).toBe('')
})

test('organization usage eligibility is OpenAI Admin-Key only', () => {
  expect(
    isOrganizationUsageEligible({
      provider: 'openai',
      has_admin_api_key: true,
    }),
  ).toBe(true)
  expect(isOrganizationUsageEligible({ provider: 'openai' })).toBe(false)
  expect(
    isOrganizationUsageEligible({
      provider: 'openai',
      has_admin_api_key: false,
    }),
  ).toBe(false)
  expect(
    isOrganizationUsageEligible({
      provider: 'generic',
      has_admin_api_key: true,
    }),
  ).toBe(false)
})

test('extracted OpenAI module keeps flat keys and zh/en parity', () => {
  const zhKeys = Object.keys(endpointOpenAiMessages['zh-CN']).sort()
  const enKeys = Object.keys(endpointOpenAiMessages['en-US']).sort()
  expect(enKeys).toEqual(zhKeys)
  // The merged locale objects expose identical key sets after the extraction.
  expect(Object.keys(messages['en-US']).sort()).toEqual(
    Object.keys(messages['zh-CN']).sort(),
  )
  // Every moved key is reachable from the flat merged object.
  const merged = messages['zh-CN'] as Record<string, string>
  for (const key of zhKeys) {
    expect(typeof merged[key]).toBe('string')
  }
})

test('Admin API Key and organization usage copy exists in both locales', () => {
  for (const locale of ['zh-CN', 'en-US'] as const) {
    const endpointMessages = endpointOpenAiMessages[locale]
    const merged = messages[locale]
    for (const key of [
      'endpointAdminApiKey',
      'endpointAdminApiKeyHint',
      'endpointAdminApiKeyOptionalOnEdit',
      'endpointAdminApiKeyClear',
      'endpointAdminApiKeyPendingClear',
      'endpointAdminApiKeyClearedOnSave',
      'endpointAdminApiKeyKeepHint',
      'endpointOrganizationUsage',
      'endpointOrganizationUsageHint',
      'endpointOrganizationUsageSaveFirst',
      'endpointOrganizationUsageRequiredKey',
      'endpointOrganizationUsageLoad',
      'endpointOrganizationUsageReload',
      'endpointOrganizationUsageInputTokens',
      'endpointOrganizationUsageOutputTokens',
      'endpointOrganizationUsageTotalTokens',
      'endpointOrganizationUsageCost',
      'endpointOrganizationUsageFetchedAt',
      'endpointOrganizationUsageCached',
      'endpointOrganizationUsageTruncated',
    ] as const) {
      expect(endpointMessages[key].length).toBeGreaterThan(0)
      // The extracted block stays flat: the same key resolves through the
      // merged locale object (MessageKey union unchanged).
      expect(merged[key]).toBe(endpointMessages[key])
    }
    // The org-level / UTC scope and the separation from subscription quota
    // must be stated where the operator reads it.
    expect(endpointMessages.endpointOrganizationUsageHint).toContain('UTC')
  }
})

test('endpointToForm echoes every stored key nickname for editing', () => {
  const primary = apiKeyFixture({ key_label: 'primary' })
  const form = endpointToForm(
    endpointFixture({
      api_keys: [
        primary,
        { ...primary, key_id: 'key-b', key_label: 'backup', enabled: false },
      ],
    }),
  )
  expect(
    form.api_keys.map((key) => [key.key_id, key.key_label, key.enabled]),
  ).toEqual([
    ['key-a', 'primary', true],
    ['key-b', 'backup', false],
  ])
  // The nickname is editable while the secret stays masked.
  expect(
    form.api_keys.every((key) => key.api_key === '' && key.has_saved_key),
  ).toBe(true)
})

test('endpointFormToRequest submits edited nicknames and keeps cleared ones', () => {
  // Regression: the request filtered on label-or-secret, so clearing a nickname
  // deleted the stored key on save. The server matches a blank row by key_id
  // and keeps its secret and label.
  const request = endpointFormToRequest(
    formWith({
      api_keys: [
        savedKey('key-a', { key_label: '  primary-eu  ' }),
        savedKey('key-b', { key_label: '   ' }),
        savedKey('key-c'),
      ],
    }),
  )
  const submitted = request.api_keys?.map(
    (key) => `${key.key_id}:${key.key_label}`,
  )
  expect(submitted).toEqual(['key-a:primary-eu', 'key-b:', 'key-c:'])
})

test('an untouched new key row is discarded, a named one is submitted', () => {
  const newKey = { ...createEmptyEndpointForm().api_keys[0] }
  const request = endpointFormToRequest(
    formWith({
      api_keys: [
        savedKey('key-a', { key_label: 'primary' }),
        newKey,
        { ...newKey, key_label: 'new key' },
      ],
    }),
  )
  // A new key still needs its secret; only its nickname may stay blank.
  expect(request.api_keys?.map((key) => key.key_label)).toEqual([
    'primary',
    'new key',
  ])
})

test('duplicate nicknames are reported per row before submit', () => {
  const rows = (labels: string[]) => labels.map((key_label) => ({ key_label }))
  // Exact match after trimming, mirroring the server duplicate check.
  const found = duplicateEndpointApiKeyLabelIndexes(
    rows(['primary', ' Primary ', '', 'primary', 'primary']),
  )
  expect(found).toEqual([0, 3, 4])
  expect(
    duplicateEndpointApiKeyLabelIndexes(rows(['primary', 'backup'])),
  ).toEqual([])
})

test('conversation_seq is labelled as a request sequence in both locales', () => {
  const zh = messages['zh-CN'].conversationSeq
  const en = messages['en-US'].conversationSeq
  // The badge renders the prompt-ferry request counter, so the label must not
  // read as a conversation-scoped sequence or as an OpenCode turn count.
  expect(en).toMatch(/request sequence/i)
  expect(en).not.toMatch(/turn/i)
  expect(en).not.toMatch(/conversation/i)
  expect(zh).toBe('请求序号')
  expect(zh).not.toBe('会话序号')
  // Same key in both locales, so the UI label survives the locale switch.
  const merged = messages['en-US'] as Record<string, string>
  expect(merged.conversationSeq).toBe(en)
})

test('endpoint key nickname copy exists in both locales', () => {
  const keys = [
    'apiKeyName',
    'apiKeyNameDefault',
    'apiKeyNameDuplicate',
    'endpointApiKeysHint',
  ]
  for (const locale of ['zh-CN', 'en-US'] as const) {
    const localeMessages = messages[locale] as Record<string, string>
    for (const key of keys) {
      expect(localeMessages[key].length).toBeGreaterThan(0)
    }
    // Usage lists show the nickname, so the placeholder mirrors the
    // server-side positional default in both locales.
    expect(localeMessages.apiKeyNameDefault).toBe('key {index}')
  }
})
