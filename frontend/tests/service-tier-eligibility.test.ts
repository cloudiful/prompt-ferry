import { expect, test } from 'bun:test'
import type { EndpointProvider, NativeApi } from '../src/generated/admin-api'
import {
  supportsServiceTierFor,
  supportsServiceTierProvider,
} from '../src/models/endpoints/service-tier'

// Mirrors `EndpointProvider::supports_service_tier_for` in
// `src/db/types/endpoints.rs`. If the Rust matrix changes, this must too.
const ALL_PROTOCOLS: readonly NativeApi[] = [
  'auto',
  'chat',
  'responses',
  'anthropic_messages',
  'realtime',
]

const UNSUPPORTED_PROVIDERS: readonly EndpointProvider[] = [
  'generic',
  'command_code',
  'opencode_go',
  'openrouter',
  'glm',
  'deepseek',
]

test('mirrors the documented provider/protocol matrix', () => {
  for (const protocol of [
    'chat',
    'responses',
    'anthropic_messages',
  ] as NativeApi[]) {
    expect(supportsServiceTierFor('minimax', protocol)).toBe(true)
  }
  expect(supportsServiceTierFor('minimax', 'realtime')).toBe(false)

  for (const protocol of ['chat', 'responses'] as NativeApi[]) {
    expect(supportsServiceTierFor('openai', protocol)).toBe(true)
  }
  for (const protocol of ['anthropic_messages', 'realtime'] as NativeApi[]) {
    expect(supportsServiceTierFor('openai', protocol)).toBe(false)
  }
})

test('auto stays eligible for supported providers and resolves at runtime', () => {
  expect(supportsServiceTierFor('minimax', 'auto')).toBe(true)
  expect(supportsServiceTierFor('openai', 'auto')).toBe(true)
})

test('only MiniMax and OpenAI are supported providers', () => {
  expect(supportsServiceTierProvider('minimax')).toBe(true)
  expect(supportsServiceTierProvider('openai')).toBe(true)
  for (const provider of UNSUPPORTED_PROVIDERS) {
    expect(supportsServiceTierProvider(provider)).toBe(false)
  }
})

test('unsupported providers, realtime and a missing provider are hidden', () => {
  for (const provider of UNSUPPORTED_PROVIDERS) {
    for (const protocol of ALL_PROTOCOLS) {
      expect(supportsServiceTierFor(provider, protocol)).toBe(false)
    }
  }
  for (const protocol of ALL_PROTOCOLS) {
    expect(supportsServiceTierFor(null, protocol)).toBe(false)
    expect(supportsServiceTierFor(undefined, protocol)).toBe(false)
  }
})
