import { expect, test } from 'bun:test'
import type { NativeApi } from '../src/generated/admin-api'
import {
  endpointFormProtocol,
  supportsServiceTierFor,
} from '../src/models/endpoints/service-tier'

// Issue #644: mirrors `supports_service_tier_for` in
// `src/db/types/endpoints.rs` — the configured override is provider-agnostic
// and covers every HTTP JSON protocol; Realtime carries WebSocket frames and
// is the only exclusion. If the Rust gate changes, this must too.

test('every HTTP JSON protocol is eligible', () => {
  for (const protocol of [
    'auto',
    'chat',
    'responses',
    'anthropic_messages',
  ] as NativeApi[]) {
    expect(supportsServiceTierFor(protocol)).toBe(true)
  }
})

test('realtime is the only excluded protocol', () => {
  expect(supportsServiceTierFor('realtime')).toBe(false)
})

test('endpointFormProtocol resolves the endpoint protocol axis', () => {
  expect(
    endpointFormProtocol({ protocol_mode: 'auto', native_api_override: null }),
  ).toBe('auto')
  expect(
    endpointFormProtocol({
      protocol_mode: 'manual',
      native_api_override: 'chat',
    }),
  ).toBe('chat')
  // A manual mode without an explicit override falls back to Responses,
  // mirroring the request-side default.
  expect(
    endpointFormProtocol({
      protocol_mode: 'manual',
      native_api_override: null,
    }),
  ).toBe('responses')
})
