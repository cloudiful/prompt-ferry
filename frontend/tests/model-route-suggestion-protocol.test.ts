import { expect, test } from 'bun:test'
import { suggestProtocolForTarget } from '../src/components/endpoints/modelRouteTargetHelpers'
import type { NativeApi } from '../src/generated/admin-api'

// Issue #1032: the model-suggestion catalog selector follows the effective
// target protocol. An explicit target protocol wins; `auto` inherits the
// endpoint protocol; when both are `auto` the listing stays unscoped so the
// endpoint's own union is preserved instead of being used as a scoped list.

const endpoints: Record<string, NativeApi> = {
  'endpoint-auto': 'auto',
  'endpoint-chat': 'chat',
  'endpoint-embeddings': 'embeddings',
  'endpoint-responses': 'responses',
  'endpoint-anthropic': 'anthropic_messages',
}

function endpointProtocolFor(endpointId: string): NativeApi {
  return endpoints[endpointId] ?? 'auto'
}

function target(
  endpoint_id: string,
  native_api: NativeApi,
): { endpoint_id: string; native_api: NativeApi } {
  return { endpoint_id, native_api }
}

test('an explicit target protocol scopes the catalog regardless of the endpoint', () => {
  expect(
    suggestProtocolForTarget(
      target('endpoint-auto', 'embeddings'),
      endpointProtocolFor,
    ),
  ).toBe('embeddings')
  expect(
    suggestProtocolForTarget(
      target('endpoint-auto', 'chat'),
      endpointProtocolFor,
    ),
  ).toBe('chat')
  expect(
    suggestProtocolForTarget(
      target('endpoint-embeddings', 'responses'),
      endpointProtocolFor,
    ),
  ).toBe('responses')
})

test('an auto target inherits the endpoint protocol', () => {
  expect(
    suggestProtocolForTarget(
      target('endpoint-embeddings', 'auto'),
      endpointProtocolFor,
    ),
  ).toBe('embeddings')
  expect(
    suggestProtocolForTarget(
      target('endpoint-chat', 'auto'),
      endpointProtocolFor,
    ),
  ).toBe('chat')
  expect(
    suggestProtocolForTarget(
      target('endpoint-anthropic', 'auto'),
      endpointProtocolFor,
    ),
  ).toBe('anthropic_messages')
})

test('an auto target on an auto endpoint stays unscoped', () => {
  expect(
    suggestProtocolForTarget(
      target('endpoint-auto', 'auto'),
      endpointProtocolFor,
    ),
  ).toBeUndefined()
})

test('an unknown or missing target never scopes the listing', () => {
  expect(
    suggestProtocolForTarget(
      target('endpoint-missing', 'auto'),
      endpointProtocolFor,
    ),
  ).toBeUndefined()
  expect(suggestProtocolForTarget(null, endpointProtocolFor)).toBeUndefined()
})
