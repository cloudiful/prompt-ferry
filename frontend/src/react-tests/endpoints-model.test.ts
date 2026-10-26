import { describe, expect, it } from 'vitest'
import {
  clearAdminApiKeyOutsideOpenAi,
  createEmptyEndpointForm,
  duplicateEndpointApiKeyLabelIndexes,
  endpointFormToRequest,
  endpointToForm,
  normalizeEndpointPlan,
  normalizeProviderPlan,
  resolveAdminApiKeyRequest,
} from '@/admin-mappers/forms/endpoint'
import type { ProviderEndpoint } from '@/generated/admin-api'
import type { EndpointApiKeyForm, EndpointForm } from '@/models'
import {
  ENDPOINT_KEY_PREFIX,
  MODEL_ROUTE_CACHE_NAMESPACE,
  MODEL_ROUTE_KEY_PREFIX,
  NETWORK_TEST_KEY_PREFIX,
  endpointActionKey,
  modelRouteActionKey,
  networkTestKey,
} from '@/models/endpoints'
import {
  isBlankScheduleRow,
  isScheduleRowValid,
} from '@/models/endpoints/schedule-windows'
import {
  endpointFormProtocol,
  supportsServiceTierFor,
} from '@/models/endpoints/service-tier'
import { capturePayload } from '@/stores/record-action-types'
import { isProxyUrlValid } from '@/components/shared/ProxySettingsFields'
import {
  applyOAuthStatusToForm,
  sanitizeOAuthErrorMessage,
  type OAuthStatusSnapshot,
} from '@/models/endpoints/oauth'

function endpoint(overrides: Partial<ProviderEndpoint> = {}): ProviderEndpoint {
  return {
    active_windows: [],
    api_keys: [],
    base_url: 'https://api.openai.com',
    created_at: '2026-10-23T12:00:00.000Z',
    enabled: true,
    endpoint_id: 'endpoint-1',
    has_admin_api_key: false,
    has_oauth_token: false,
    has_proxy_url: false,
    key_lb_enabled: false,
    mcp_enabled: false,
    name: 'Fixture upstream',
    native_api: 'auto',
    native_api_source: 'auto',
    owner_user_id: null,
    plan: 'platform_api_key',
    provider: 'openai',
    provider_region: null,
    scope: 'admin',
    service_tier: null,
    updated_at: '2026-10-23T12:00:00.000Z',
    ...overrides,
  }
}

function openAiForm(overrides: Partial<EndpointForm> = {}): EndpointForm {
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

describe('upstream endpoint model', () => {
  it('keeps the shared write and probe key grammar stable', () => {
    expect(ENDPOINT_KEY_PREFIX).toBe('endpoint')
    expect(MODEL_ROUTE_KEY_PREFIX).toBe('modelroute')
    expect(NETWORK_TEST_KEY_PREFIX).toBe('networktest')
    expect(endpointActionKey(null)).toBe('endpoint:create')
    expect(endpointActionKey('endpoint-1')).toBe('endpoint:endpoint-1')
    expect(modelRouteActionKey(null)).toBe('modelroute:create')
    expect(modelRouteActionKey('route-1')).toBe('modelroute:route-1')
    expect(networkTestKey('endpoint', 'endpoint-1')).toBe(
      'networktest:endpoint:endpoint-1',
    )
    expect(MODEL_ROUTE_CACHE_NAMESPACE).toBe('model-routes-page')
  })

  it('keeps stored OpenAI Admin API key state mapper-only and blank on edit', () => {
    const form = endpointToForm(
      endpoint({
        has_admin_api_key: true,
        plan: 'platform_api_key',
      }),
    )

    expect(form.has_admin_api_key).toBe(true)
    expect(form.admin_api_key).toBe('')
    expect(form.admin_api_key_clear).toBe(false)
    expect(capturePayload(endpointFormToRequest(form))).not.toHaveProperty(
      'admin_api_key',
    )
  })

  it('preserves provider, protocol, owner, key, proxy, schedule and tier mapping', () => {
    const form = endpointToForm(
      endpoint({
        active_windows: [{ start: '22:00', end: '06:00', days: [1, 3] }],
        api_keys: [
          {
            created_at: '2026-10-23T12:00:00.000Z',
            enabled: true,
            endpoint_id: 'endpoint-1',
            key_id: 'key-1',
            key_label: 'primary',
            position: 0,
            updated_at: '2026-10-23T12:00:00.000Z',
          },
        ],
        has_proxy_url: true,
        provider: 'minimax',
        provider_region: 'global',
        scope: 'user',
        owner_user_id: 4,
        service_tier: ' priority ',
      }),
    )
    form.protocol_mode = 'manual'
    form.native_api_override = 'embeddings'
    form.proxy_url = 'socks5h://proxy.example:1080'
    form.active_windows_touched = true

    const request = endpointFormToRequest(form)

    expect(request).toMatchObject({
      active_windows: [{ start: '22:00', end: '06:00', days: [1, 3] }],
      api_keys: [
        {
          api_key: '',
          enabled: true,
          key_id: 'key-1',
          key_label: 'primary',
        },
      ],
      owner_user_id: 4,
      plan: 'platform_api_key',
      protocol_mode: 'manual',
      provider: 'minimax',
      provider_region: 'global',
      proxy_url: 'socks5h://proxy.example:1080',
      scope: 'user',
      service_tier: 'priority',
      native_api_override: 'embeddings',
    })
    expect(capturePayload(request)).not.toHaveProperty('admin_api_key')
    expect(request.active_windows?.[0]?.days).toEqual([1, 3])
  })

  it('omits untouched proxy and schedule values and sends explicit all-day edits', () => {
    const form = createEmptyEndpointForm()
    form.has_saved_proxy_url = true
    form.active_windows = [{ start: '', end: '' }]

    const untouched = endpointFormToRequest(form)
    expect(capturePayload(untouched)).not.toHaveProperty('proxy_url')
    expect(capturePayload(untouched)).not.toHaveProperty('active_windows')

    form.proxy_url = ''
    form.has_saved_proxy_url = false
    form.active_windows_touched = true
    const cleared = endpointFormToRequest(form)
    expect(cleared.proxy_url).toBe('')
    expect(cleared.active_windows).toEqual([{ start: '00:00', end: '24:00' }])
  })

  it('warns on duplicate key labels without changing submitted form data', () => {
    const form = createEmptyEndpointForm()
    form.api_keys = [
      { ...form.api_keys[0]!, key_label: 'Primary' },
      { ...form.api_keys[0]!, key_label: ' Primary ' },
      { ...form.api_keys[0]!, key_label: '' },
    ]

    expect(duplicateEndpointApiKeyLabelIndexes(form.api_keys)).toEqual([0, 1])
    expect(endpointFormToRequest(form).api_keys).toHaveLength(2)
  })

  it('applies OAuth status without clobbering user-edited plan state', () => {
    const form = endpointToForm(endpoint())
    form.name = 'Draft name'
    form.proxy_url = 'socks5h://proxy.example:1080'
    const snapshot: OAuthStatusSnapshot = {
      endpointId: 'endpoint-1',
      dialogGeneration: 7,
      provider: 'openai',
      plan: 'platform_api_key',
      has_oauth_token: false,
      userPlanRevision: 3,
    }
    const status = {
      has_oauth_token: true,
      plan: 'chatgpt_subscription' as const,
    }

    const applied = applyOAuthStatusToForm(form, snapshot, status, {
      endpointId: 'endpoint-1',
      dialogGeneration: 7,
      userPlanRevision: 3,
    })

    expect(applied).toMatchObject({
      has_oauth_token: true,
      name: 'Draft name',
      plan: 'chatgpt_subscription',
      proxy_url: 'socks5h://proxy.example:1080',
    })

    const awayAndBack = applyOAuthStatusToForm(
      { ...form, plan: 'platform_api_key' },
      snapshot,
      status,
      {
        endpointId: 'endpoint-1',
        dialogGeneration: 7,
        userPlanRevision: 5,
      },
    )
    expect(awayAndBack?.plan).toBe('platform_api_key')

    const userPlanWins = applyOAuthStatusToForm(
      { ...form, plan: 'chatgpt_subscription' },
      snapshot,
      { has_oauth_token: true, plan: 'platform_api_key' },
      {
        endpointId: 'endpoint-1',
        dialogGeneration: 7,
        userPlanRevision: 4,
      },
    )
    expect(userPlanWins?.plan).toBe('chatgpt_subscription')
  })

  it('forces platform plan after token removal and rejects stale OAuth status', () => {
    const form = endpointToForm(
      endpoint({
        has_oauth_token: true,
        plan: 'chatgpt_subscription',
      }),
    )
    const snapshot: OAuthStatusSnapshot = {
      endpointId: 'endpoint-1',
      dialogGeneration: 2,
      provider: 'openai',
      plan: 'chatgpt_subscription',
      has_oauth_token: true,
      userPlanRevision: 0,
    }

    expect(
      applyOAuthStatusToForm(
        form,
        snapshot,
        { has_oauth_token: false, plan: 'platform_api_key' },
        {
          endpointId: 'endpoint-1',
          dialogGeneration: 2,
          userPlanRevision: 0,
        },
      ),
    ).toMatchObject({ has_oauth_token: false, plan: 'platform_api_key' })
    expect(
      applyOAuthStatusToForm(
        form,
        snapshot,
        { has_oauth_token: true, plan: 'platform_api_key' },
        {
          endpointId: 'endpoint-2',
          dialogGeneration: 2,
          userPlanRevision: 0,
        },
      ),
    ).toBeNull()
  })

  it('redacts OAuth redirect URLs and authorization-code fields in errors', () => {
    const redirectUrl =
      'https://callback.example.test/finish?code=synthetic-code&state=synthetic-state'
    const safe = sanitizeOAuthErrorMessage(`OAuth failed for ${redirectUrl}`, [
      redirectUrl,
    ])

    expect(safe).not.toContain(redirectUrl)
    expect(safe).not.toContain('synthetic-code')
    expect(safe).not.toContain('synthetic-state')
    expect(safe).toContain('[redacted]')
  })

  it('uses existing proxy, schedule and service-tier rules', () => {
    expect(isProxyUrlValid('')).toBe(true)
    expect(isProxyUrlValid('https://proxy.example:8443')).toBe(true)
    expect(isProxyUrlValid('socks5://')).toBe(false)
    expect(isBlankScheduleRow({ start: ' ', end: '' })).toBe(true)
    expect(isScheduleRowValid({ start: '', end: '' })).toBe(true)
    expect(isScheduleRowValid({ start: '22:00', end: '06:00' })).toBe(true)
    expect(isScheduleRowValid({ start: '24:00', end: '06:00' })).toBe(false)
    expect(supportsServiceTierFor('auto')).toBe(true)
    expect(supportsServiceTierFor('embeddings')).toBe(false)
    expect(supportsServiceTierFor('realtime')).toBe(false)
    expect(
      endpointFormProtocol({
        protocol_mode: 'manual',
        native_api_override: 'realtime',
      } as Pick<EndpointForm, 'protocol_mode' | 'native_api_override'>),
    ).toBe('realtime')
  })

  it('applies the OpenAI Admin API Key save, keep and clear contract', () => {
    const empty = createEmptyEndpointForm()
    expect(empty.admin_api_key).toBe('')
    expect(empty.has_admin_api_key).toBe(false)
    expect(empty.admin_api_key_clear).toBe(false)

    const saved = endpointToForm(endpoint({ has_admin_api_key: true }))
    expect(saved.has_admin_api_key).toBe(true)
    expect(saved.admin_api_key).toBe('')
    expect(saved.admin_api_key_clear).toBe(false)
    expect(endpointToForm(endpoint()).has_admin_api_key).toBe(false)

    // Replace: a typed value is sent trimmed.
    expect(
      endpointFormToRequest(openAiForm({ admin_api_key: ' sk-admin-1 ' }))
        .admin_api_key,
    ).toBe('sk-admin-1')
    // Keep: empty plus a stored key omits the field so PATCH keeps it.
    expect(
      endpointFormToRequest(
        openAiForm({ has_admin_api_key: true, admin_api_key: '' }),
      ).admin_api_key,
    ).toBeUndefined()
    // Unset: a blank value without a stored key omits it as well.
    expect(
      endpointFormToRequest(openAiForm({ admin_api_key: '   ' })).admin_api_key,
    ).toBeUndefined()
    // Clear: the explicit clear sends the empty-string sentinel.
    expect(
      endpointFormToRequest(
        openAiForm({ has_admin_api_key: true, admin_api_key_clear: true }),
      ).admin_api_key,
    ).toBe('')
    // An explicit clear wins over a leftover typed value.
    expect(
      endpointFormToRequest(
        openAiForm({ admin_api_key: 'sk-typed', admin_api_key_clear: true }),
      ).admin_api_key,
    ).toBe('')
  })

  it('never submits an Admin API Key away from OpenAI and drops the typed secret', () => {
    for (const provider of ['generic', 'minimax', 'deepseek'] as const) {
      const form = openAiForm({ provider, admin_api_key: 'sk-should-not-send' })
      expect(resolveAdminApiKeyRequest(form)).toBeUndefined()
      expect(endpointFormToRequest(form).admin_api_key).toBeUndefined()
    }

    const form = openAiForm({
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

  it('discards an untouched new key row and submits a named one', () => {
    const newKey = { ...createEmptyEndpointForm().api_keys[0]! }
    const request = endpointFormToRequest(
      openAiForm({
        api_keys: [
          savedKey('key-a', { key_label: 'primary' }),
          newKey,
          { ...newKey, key_label: 'new key' },
        ],
      }),
    )

    expect(request.api_keys?.map((key) => key.key_label)).toEqual([
      'primary',
      'new key',
    ])
  })

  it('submits edited nicknames and keeps a cleared one', () => {
    const request = endpointFormToRequest(
      openAiForm({
        api_keys: [
          savedKey('key-a', { key_label: '  primary-eu  ' }),
          savedKey('key-b', { key_label: '   ' }),
          savedKey('key-c'),
        ],
      }),
    )

    expect(
      request.api_keys?.map((key) => `${key.key_id}:${key.key_label}`),
    ).toEqual(['key-a:primary-eu', 'key-b:', 'key-c:'])
  })

  it('keeps the endpoint plan on the platform fallback away from OpenAI', () => {
    expect(normalizeEndpointPlan('chatgpt_subscription')).toBe(
      'chatgpt_subscription',
    )
    expect(normalizeEndpointPlan('platform_api_key')).toBe('platform_api_key')
    expect(normalizeEndpointPlan(undefined)).toBe('platform_api_key')
    expect(normalizeEndpointPlan(null)).toBe('platform_api_key')
    expect(normalizeEndpointPlan('legacy-unknown')).toBe('platform_api_key')

    expect(normalizeProviderPlan('openai', 'chatgpt_subscription')).toBe(
      'chatgpt_subscription',
    )
    for (const provider of [
      'generic',
      'minimax',
      'command_code',
      'opencode_go',
      'openrouter',
      'glm',
      'deepseek',
    ] as const) {
      expect(normalizeProviderPlan(provider, 'chatgpt_subscription')).toBe(
        'platform_api_key',
      )
    }

    // The request always sends the effective plan, so a stale subscription
    // selection never survives a provider switch.
    expect(
      endpointFormToRequest(
        openAiForm({ provider: 'minimax', plan: 'chatgpt_subscription' }),
      ).plan,
    ).toBe('platform_api_key')
    expect(
      endpointFormToRequest(
        openAiForm({ provider: 'openai', plan: 'chatgpt_subscription' }),
      ).plan,
    ).toBe('chatgpt_subscription')
  })
})
