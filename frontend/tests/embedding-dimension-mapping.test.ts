import { expect, test } from 'bun:test'
import {
  createEmptyEndpointForm,
  endpointFormToRequest,
  endpointToForm,
  createEmptyModelRouteForm,
  modelRouteFormToRequest,
  modelRouteToForm,
  parseEmbeddingDimensionsInput,
} from '../src/admin-mappers'
import {
  canSaveTargets,
  hasTargetEmbeddingDimensions,
  hasTargetSettings,
  isTargetEmbeddingDimensionsValid,
} from '../src/components/endpoints/modelRouteTargetHelpers'
import type {
  ModelEndpointRule,
  ProviderEndpoint,
} from '../src/generated/admin-api'
import {
  effectiveTargetProtocol,
  isEffectiveEmbeddingsTarget,
  normalizeNativeApiAxis,
  supportsChatAdaptationFor,
  supportsEmbeddingDimensionOverrideFor,
  supportsServiceTierFor,
} from '../src/models/endpoints/service-tier'

// Issue #1032: the UI capability axis. Only the embeddings protocol shows the
// dimension override; the chat-only settings are hidden there, and an `auto`
// endpoint never guesses a protocol from the model name.

test('only the embeddings protocol carries the dimension override', () => {
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

test('the chat-only settings are hidden for an embeddings protocol', () => {
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

test('an unknown protocol axis normalizes to auto', () => {
  expect(normalizeNativeApiAxis('embeddings')).toBe('embeddings')
  expect(normalizeNativeApiAxis('nonsense')).toBe('auto')
  expect(normalizeNativeApiAxis(undefined)).toBe('auto')
  expect(normalizeNativeApiAxis('AUTO')).toBe('auto')
})

test('an auto target inherits the endpoint protocol and an explicit target wins', () => {
  expect(effectiveTargetProtocol('auto', 'embeddings')).toBe('embeddings')
  expect(effectiveTargetProtocol('chat', 'embeddings')).toBe('chat')
  expect(effectiveTargetProtocol('embeddings', 'chat')).toBe('embeddings')
  // An auto endpoint leaves the operation to the runtime, so the UI stays on
  // auto instead of inferring a protocol.
  expect(effectiveTargetProtocol('auto', 'auto')).toBe('auto')
  expect(
    isEffectiveEmbeddingsTarget({ native_api: 'auto' } as never, 'embeddings'),
  ).toBe(true)
  expect(
    isEffectiveEmbeddingsTarget({ native_api: 'chat' } as never, 'embeddings'),
  ).toBe(false)
})

test('the dimension input rejects every malformed shape and never clamps', () => {
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

test('a stored dimension loads, and a legacy or invalid one means inherit', () => {
  expect(
    modelRouteToForm(routeWith(512)).targets[0]?.embedding_dimensions_override,
  ).toBe(512)
  expect(
    modelRouteToForm(routeWith(null)).targets[0]?.embedding_dimensions_override,
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

test('a new target form inherits the client dimensions by default', () => {
  const target = createEmptyModelRouteForm().targets[0]
  expect(target?.embedding_dimensions_override).toBeNull()
  expect(target?.embedding_dimensions_invalid).toBeUndefined()
})

test('the mapper always sends the dimension explicitly, never dropping it', () => {
  const form = createEmptyModelRouteForm()
  const target = form.targets[0]
  if (!target) throw new Error('expected a default target')
  target.endpoint_id = 'endpoint-1'

  // Inherit is an explicit null so a save round-trips the stored value.
  expect(
    modelRouteFormToRequest(form).targets?.[0]?.embedding_dimensions_override,
  ).toBeNull()

  target.embedding_dimensions_override = 512
  expect(
    modelRouteFormToRequest(form).targets?.[0]?.embedding_dimensions_override,
  ).toBe(512)

  // A malformed control entry never rewrites the committed value into inherit.
  target.embedding_dimensions_invalid = true
  expect(
    modelRouteFormToRequest(form).targets?.[0]?.embedding_dimensions_override,
  ).toBe(512)
})

test('a malformed dimension blocks save and lights the settings gear', () => {
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

test('an embeddings endpoint protocol round-trips through the form', () => {
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

  // An `auto` endpoint is represented by `protocol_mode: auto` with a null
  // override, never by an explicit protocol.
  const autoForm = createEmptyEndpointForm()
  expect(autoForm.native_api_override).toBeNull()
  expect(endpointFormToRequest(autoForm).native_api_override).toBeNull()
})
