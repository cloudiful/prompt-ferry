import { describe, expect, it } from 'vitest'
import type {
  ModelEndpointRule,
  NativeApi,
  ProviderEndpoint,
} from '@/generated/admin-api'
import {
  createEmptyEndpointForm,
  endpointFormToRequest,
  endpointToForm,
  normalizeServiceTier,
} from '@/admin-mappers/forms/endpoint'
import {
  createEmptyModelRouteForm,
  modelRouteFormToRequest,
  modelRouteToForm,
  parseEmbeddingDimensionsInput,
} from '@/admin-mappers/forms/model-route'
import {
  canSaveTargets,
  hasTargetEmbeddingDimensions,
  hasTargetServiceTier,
  hasTargetSettings,
  isTargetEmbeddingDimensionsValid,
} from '@/models/endpoints/model-route-target'
import {
  effectiveTargetProtocol,
  endpointFormProtocol,
  isEffectiveEmbeddingsTarget,
  normalizeNativeApiAxis,
  supportsChatAdaptationFor,
  supportsEmbeddingDimensionOverrideFor,
  supportsServiceTierFor,
} from '@/models/endpoints/service-tier'
import { endpointMessages } from '@/i18n/modules/endpoints'

function endpointFixture(
  overrides: Partial<ProviderEndpoint> = {},
): ProviderEndpoint {
  return {
    api_keys: [],
    base_url: 'https://api.minimaxi.com',
    created_at: '2026-09-04T00:00:00Z',
    enabled: true,
    endpoint_id: 'endpoint-1',
    key_lb_enabled: false,
    mcp_enabled: false,
    name: 'minimax',
    native_api: 'chat',
    native_api_source: 'manual',
    provider: 'minimax',
    scope: 'admin',
    updated_at: '2026-09-04T00:00:00Z',
    ...overrides,
  }
}

function routeWith(dimension: unknown): ModelEndpointRule {
  return {
    created_at: '2026-10-22T00:00:00Z',
    enabled: true,
    model_pattern: 'qwen/*',
    rule_id: 'rule-1',
    scope: 'admin',
    updated_at: '2026-10-22T00:00:00Z',
    targets: [
      {
        created_at: '2026-10-22T00:00:00Z',
        enabled: true,
        endpoint_enabled: true,
        endpoint_id: 'endpoint-1',
        position: 0,
        rule_id: 'rule-1',
        target_id: 'target-1',
        updated_at: '2026-10-22T00:00:00Z',
        native_api: 'embeddings',
        embedding_dimensions_override: dimension as number | null,
      },
    ],
  } as ModelEndpointRule
}

function routeWithTier(
  serviceTier: string | null | undefined,
): ModelEndpointRule {
  return {
    created_at: '2026-09-04T00:00:00Z',
    enabled: true,
    model_pattern: 'test-*',
    rule_id: 'rule-1',
    scope: 'admin',
    updated_at: '2026-09-04T00:00:00Z',
    targets: [
      {
        created_at: '2026-09-04T00:00:00Z',
        enabled: true,
        endpoint_enabled: true,
        endpoint_id: 'endpoint-1',
        position: 0,
        rule_id: 'rule-1',
        service_tier: serviceTier,
        target_id: 'target-1',
        updated_at: '2026-09-04T00:00:00Z',
      },
    ],
  } as ModelEndpointRule
}

describe('protocol axis and service tier', () => {
  it('carries the dimension override only on the embeddings protocol', () => {
    expect(supportsEmbeddingDimensionOverrideFor('embeddings')).toBe(true)
    for (const protocol of [
      'auto',
      'chat',
      'responses',
      'anthropic_messages',
      'realtime',
    ] as const) {
      expect(supportsEmbeddingDimensionOverrideFor(protocol)).toBe(false)
    }
  })

  it('hides the chat-only settings for an embeddings protocol', () => {
    expect(supportsChatAdaptationFor('embeddings')).toBe(false)
    expect(supportsServiceTierFor('embeddings')).toBe(false)
    for (const protocol of [
      'auto',
      'chat',
      'responses',
      'anthropic_messages',
    ] as const) {
      expect(supportsChatAdaptationFor(protocol)).toBe(true)
      expect(supportsServiceTierFor(protocol)).toBe(true)
    }
  })

  it('normalizes an unknown protocol axis to auto', () => {
    expect(normalizeNativeApiAxis('embeddings')).toBe('embeddings')
    expect(normalizeNativeApiAxis('nonsense')).toBe('auto')
    expect(normalizeNativeApiAxis(undefined)).toBe('auto')
    expect(normalizeNativeApiAxis('AUTO')).toBe('auto')
  })

  it('inherits the endpoint protocol for an auto target and lets an explicit target win', () => {
    expect(effectiveTargetProtocol('auto', 'embeddings')).toBe('embeddings')
    expect(effectiveTargetProtocol('chat', 'embeddings')).toBe('chat')
    expect(effectiveTargetProtocol('embeddings', 'chat')).toBe('embeddings')
    expect(effectiveTargetProtocol('auto', 'auto')).toBe('auto')
    expect(
      isEffectiveEmbeddingsTarget(
        { native_api: 'auto' } as never,
        'embeddings',
      ),
    ).toBe(true)
    expect(
      isEffectiveEmbeddingsTarget(
        { native_api: 'chat' } as never,
        'embeddings',
      ),
    ).toBe(false)
  })

  it('rejects every malformed dimension shape and never clamps it', () => {
    expect(parseEmbeddingDimensionsInput('')).toEqual({
      value: null,
      invalid: false,
    })
    expect(parseEmbeddingDimensionsInput('   ')).toEqual({
      value: null,
      invalid: false,
    })
    expect(parseEmbeddingDimensionsInput('512')).toEqual({
      value: 512,
      invalid: false,
    })
    expect(parseEmbeddingDimensionsInput(' 1024 ')).toEqual({
      value: 1024,
      invalid: false,
    })
    expect(parseEmbeddingDimensionsInput('2147483647')).toEqual({
      value: 2147483647,
      invalid: false,
    })
    for (const raw of ['0', '-1', '1.5', 'abc', '1e3', '+8', '2147483648']) {
      const parsed = parseEmbeddingDimensionsInput(raw)
      expect(parsed.invalid).toBe(true)
      expect(parsed.value).toBeNull()
    }
  })

  it('loads a stored dimension and treats a legacy or invalid one as inherit', () => {
    expect(
      modelRouteToForm(routeWith(512)).targets[0]
        ?.embedding_dimensions_override,
    ).toBe(512)
    expect(
      modelRouteToForm(routeWith(null)).targets[0]
        ?.embedding_dimensions_override,
    ).toBeNull()
    expect(
      modelRouteToForm(routeWith(undefined)).targets[0]
        ?.embedding_dimensions_override,
    ).toBeNull()
    for (const invalid of [0, -3, 1.5]) {
      expect(
        modelRouteToForm(routeWith(invalid)).targets[0]
          ?.embedding_dimensions_override,
      ).toBeNull()
    }
  })

  it('inherits the client dimensions in a new target form', () => {
    const target = createEmptyModelRouteForm().targets[0]
    expect(target?.embedding_dimensions_override).toBeNull()
    expect(target?.embedding_dimensions_invalid).toBeUndefined()
  })

  it('always sends the dimension explicitly instead of dropping it', () => {
    const form = createEmptyModelRouteForm()
    const target = form.targets[0]
    if (!target) throw new Error('expected a default target')
    target.endpoint_id = 'endpoint-1'

    expect(
      modelRouteFormToRequest(form).targets?.[0]?.embedding_dimensions_override,
    ).toBeNull()

    target.embedding_dimensions_override = 512
    expect(
      modelRouteFormToRequest(form).targets?.[0]?.embedding_dimensions_override,
    ).toBe(512)

    target.embedding_dimensions_invalid = true
    expect(
      modelRouteFormToRequest(form).targets?.[0]?.embedding_dimensions_override,
    ).toBe(512)
  })

  it('blocks the save and lights the settings gear on a malformed dimension', () => {
    const target = createEmptyModelRouteForm().targets[0]
    if (!target) throw new Error('expected a default target')
    expect(canSaveTargets([target])).toBe(true)
    expect(hasTargetEmbeddingDimensions(target)).toBe(false)
    expect(hasTargetSettings(target)).toBe(false)

    target.embedding_dimensions_override = 768
    expect(hasTargetEmbeddingDimensions(target)).toBe(true)
    expect(hasTargetSettings(target)).toBe(true)

    target.embedding_dimensions_invalid = true
    expect(isTargetEmbeddingDimensionsValid(target)).toBe(false)
    expect(canSaveTargets([target])).toBe(false)
  })

  it('round-trips an embeddings endpoint protocol through the form', () => {
    const endpoint = {
      api_keys: [],
      base_url: 'https://openrouter.ai/api',
      created_at: '2026-10-22T00:00:00Z',
      enabled: true,
      endpoint_id: 'endpoint-1',
      has_proxy_url: false,
      key_lb_enabled: false,
      mcp_enabled: false,
      name: 'embeddings',
      native_api: 'embeddings',
      native_api_source: 'manual',
      owner_user_id: null,
      plan: 'platform_api_key',
      provider: 'openrouter',
      scope: 'admin',
      updated_at: '2026-10-22T00:00:00Z',
    } as ProviderEndpoint
    const form = endpointToForm(endpoint)
    expect(form.protocol_mode).toBe('manual')
    expect(form.native_api_override).toBe('embeddings')
    expect(endpointFormToRequest(form).native_api_override).toBe('embeddings')

    const autoForm = createEmptyEndpointForm()
    expect(autoForm.native_api_override).toBeNull()
    expect(endpointFormToRequest(autoForm).native_api_override).toBeNull()
  })

  it('keeps an auto endpoint protocol unresolved and honours an explicit one', () => {
    expect(
      endpointFormProtocol({
        protocol_mode: 'auto',
        native_api_override: null,
      }),
    ).toBe('auto')
    expect(
      endpointFormProtocol({
        protocol_mode: 'manual',
        native_api_override: 'embeddings',
      }),
    ).toBe('embeddings')
    expect(
      endpointFormProtocol({
        protocol_mode: 'manual',
        native_api_override: null,
      }),
    ).toBe('responses')
  })

  it('defaults a new endpoint form to inherit the service tier', () => {
    expect(createEmptyEndpointForm().service_tier).toBeNull()
  })

  it('trims the service tier and treats a blank value as inherit', () => {
    expect(normalizeServiceTier('priority')).toBe('priority')
    expect(normalizeServiceTier('standard')).toBe('standard')
    expect(normalizeServiceTier('fast')).toBe('fast')
    expect(normalizeServiceTier(' legacy-unknown ')).toBe('legacy-unknown')
    expect(normalizeServiceTier(undefined)).toBeNull()
    expect(normalizeServiceTier(null)).toBeNull()
    expect(normalizeServiceTier('')).toBeNull()
    expect(normalizeServiceTier('   ')).toBeNull()
  })

  it('preserves a configured endpoint tier and inherits legacy values', () => {
    expect(
      endpointToForm(endpointFixture({ service_tier: 'priority' }))
        .service_tier,
    ).toBe('priority')
    expect(
      endpointToForm(endpointFixture({ service_tier: 'standard' }))
        .service_tier,
    ).toBe('standard')
    expect(
      endpointToForm(endpointFixture({ service_tier: 'fast' })).service_tier,
    ).toBe('fast')
    expect(
      endpointToForm(endpointFixture({ service_tier: null })).service_tier,
    ).toBeNull()
    const { service_tier, ...legacy } = endpointFixture()
    void service_tier
    expect(endpointToForm(legacy as ProviderEndpoint).service_tier).toBeNull()
  })

  it('round-trips the endpoint service tier and inherits a blank one', () => {
    const priority = {
      ...createEmptyEndpointForm(),
      service_tier: 'priority',
    }
    expect(endpointFormToRequest(priority).service_tier).toBe('priority')
    const freeForm = {
      ...createEmptyEndpointForm(),
      service_tier: ' fast ',
    }
    expect(endpointFormToRequest(freeForm).service_tier).toBe('fast')
    expect(
      endpointFormToRequest(createEmptyEndpointForm()).service_tier,
    ).toBeNull()
    const blank = {
      ...createEmptyEndpointForm(),
      service_tier: '   ',
    }
    expect(endpointFormToRequest(blank).service_tier).toBeNull()
  })

  it('submits a configured tier for every provider', () => {
    for (const provider of [
      'generic',
      'minimax',
      'openai',
      'deepseek',
      'glm',
      'openrouter',
      'command_code',
      'opencode_go',
    ] as const) {
      const configured = {
        ...createEmptyEndpointForm(),
        provider,
        service_tier: 'priority',
      }
      expect(endpointFormToRequest(configured).service_tier).toBe('priority')
    }
  })

  it('keeps the service tier eligible on the JSON protocols only', () => {
    for (const protocol of [
      'auto',
      'chat',
      'responses',
      'anthropic_messages',
    ] as const) {
      expect(supportsServiceTierFor(protocol)).toBe(true)
    }
    expect(supportsServiceTierFor('realtime')).toBe(false)
    expect(supportsServiceTierFor('embeddings')).toBe(false)
    // A target left on auto inherits the endpoint protocol, so an embeddings
    // endpoint hides the tier for that target while an auto endpoint keeps it.
    expect(
      supportsServiceTierFor(effectiveTargetProtocol('auto', 'embeddings')),
    ).toBe(false)
    expect(
      supportsServiceTierFor(effectiveTargetProtocol('auto', 'auto')),
    ).toBe(true)
  })

  it('defaults a new model-route target form to inherit the tier', () => {
    expect(createEmptyModelRouteForm().targets[0]?.service_tier).toBeNull()
  })

  it('preserves a configured target tier', () => {
    expect(
      modelRouteToForm(routeWithTier('priority')).targets[0]?.service_tier,
    ).toBe('priority')
    expect(
      modelRouteToForm(routeWithTier('fast')).targets[0]?.service_tier,
    ).toBe('fast')
    expect(
      modelRouteToForm(routeWithTier('standard')).targets[0]?.service_tier,
    ).toBe('standard')
  })

  it('inherits a missing, null or blank target tier', () => {
    expect(
      modelRouteToForm(routeWithTier(null)).targets[0]?.service_tier,
    ).toBeNull()
    expect(
      modelRouteToForm(routeWithTier('')).targets[0]?.service_tier,
    ).toBeNull()
    expect(
      modelRouteToForm(routeWithTier('   ')).targets[0]?.service_tier,
    ).toBeNull()
    const rule = routeWithTier('priority')
    const { service_tier, ...bareTarget } = rule.targets[0] ?? {}
    void service_tier
    const withoutTier = {
      ...rule,
      targets: [bareTarget],
    } as unknown as ModelEndpointRule
    expect(modelRouteToForm(withoutTier).targets[0]?.service_tier).toBeNull()
  })

  it('round-trips the target tier through the request', () => {
    const request = modelRouteFormToRequest(
      modelRouteToForm(routeWithTier('priority')),
    )
    expect(request.targets?.[0]?.service_tier).toBe('priority')
    const blank = modelRouteToForm(routeWithTier('priority'))
    const target = blank.targets[0]
    if (!target) throw new Error('expected a default target')
    target.service_tier = '   '
    expect(modelRouteFormToRequest(blank).targets?.[0]?.service_tier).toBeNull()
    const inherit = modelRouteFormToRequest(
      modelRouteToForm(routeWithTier(null)),
    )
    expect(inherit.targets?.[0]?.service_tier).toBeNull()
  })

  it('lights the target settings gear for a configured tier only', () => {
    const target = createEmptyModelRouteForm().targets[0]
    if (!target) throw new Error('expected a default target')
    expect(hasTargetServiceTier(target)).toBe(false)
    expect(hasTargetSettings(target)).toBe(false)

    target.service_tier = 'priority'
    expect(hasTargetServiceTier(target)).toBe(true)
    expect(hasTargetSettings(target)).toBe(true)

    target.service_tier = '   '
    expect(hasTargetServiceTier(target)).toBe(false)
    expect(hasTargetSettings(target)).toBe(false)

    target.service_tier = null
    expect(hasTargetSettings(target)).toBe(false)
  })

  it('explains the free-form inherit contract in both locales', () => {
    for (const locale of ['zh-CN', 'en-US'] as const) {
      const messages = endpointMessages[locale]
      expect(messages.serviceTier.length).toBeGreaterThan(0)
      expect(messages.proxyInherit.length).toBeGreaterThan(0)
      expect(messages.serviceTierHint.length).toBeGreaterThan(0)
    }
    expect(endpointMessages['zh-CN'].proxyInherit).toBe('继承')
  })

  it('names the service tier control in the endpoint settings copy', () => {
    for (const locale of ['zh-CN', 'en-US'] as const) {
      const messages = endpointMessages[locale]
      expect(messages.endpointSettingsHint.toLowerCase()).toContain(
        messages.serviceTier.toLowerCase(),
      )
    }
  })
})

describe('protocol axis types', () => {
  it('keeps the native api axis closed', () => {
    const axis: NativeApi[] = [
      'auto',
      'anthropic_messages',
      'chat',
      'embeddings',
      'responses',
      'realtime',
    ]
    for (const value of axis) {
      expect(normalizeNativeApiAxis(value)).toBe(value)
    }
  })
})
