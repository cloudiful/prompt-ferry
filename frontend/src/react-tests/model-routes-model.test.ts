import { describe, expect, it } from 'vitest'
import type { ModelEndpointRule } from '@/generated/admin-api'
import {
  createEmptyModelRouteForm,
  modelRouteFormToRequest,
  modelRouteToForm,
} from '@/admin-mappers/forms/model-route'
import {
  modelRoutePageQueryKey,
  patchModelRoutePages,
} from '@/lib/model-routes'
import { modelRoutePagesQueryKey } from '@/lib/endpoints'
import { suggestProtocolForTarget } from '@/models/endpoints/model-route-target'
import {
  createModelRouteListItemView,
  modelRouteTestCounts,
} from '@/models/endpoints/model-route-item'
import { reorderTargetByIdentity } from '@/lib/use-model-route-target-drag'
import { capturePayload } from '@/stores/record-action-types'
import { QueryClient } from '@tanstack/react-query'

const route: ModelEndpointRule = {
  created_at: '2026-10-23T12:00:00.000Z',
  enabled: true,
  model_pattern: 'long-model-pattern-*',
  owner_user_id: 7,
  routing_strategy: 'responses_session_affinity',
  rule_id: 'route-7',
  scope: 'user',
  targets: [
    {
      active_windows: [{ start: '22:00', end: '06:00', days: [1, 3] }],
      compact_mode: 'self_summarize',
      created_at: '2026-10-23T12:00:00.000Z',
      dev_system_normalize: true,
      embedding_dimensions_override: 1536,
      enabled: true,
      endpoint_enabled: true,
      endpoint_id: 'endpoint-off-page',
      endpoint_name: 'Endpoint beyond first page',
      has_proxy_url_override: true,
      native_api: 'auto',
      position: 0,
      rule_id: 'route-7',
      service_tier: 'priority',
      target_id: 'target-a',
      thinking_downgrade_enabled: true,
      thinking_effort_override: 'high',
      updated_at: '2026-10-23T12:00:00.000Z',
      upstream_model: 'upstream-model-a',
    },
  ],
  updated_at: '2026-10-23T12:00:00.000Z',
}

describe('model-route integration model', () => {
  it('keeps route pages in the upstream invalidation namespace with independent offsets', () => {
    expect(modelRoutePagesQueryKey()).toEqual(['model-routes-page'])
    expect(modelRoutePageQueryKey(20, 10)).toEqual([
      'model-routes-page',
      20,
      10,
    ])
  })

  it('preserves the mapper payload and target-associated overrides in configured order', () => {
    const form = modelRouteToForm(route)
    form.targets[0]!.proxy_url_override = ''
    form.targets[0]!.has_saved_proxy_url_override = true
    form.targets[0]!.active_windows_touched = false
    form.targets.push({
      ...createEmptyModelRouteForm().targets[0]!,
      endpoint_id: 'endpoint-b',
      upstream_model: 'model-b',
    })
    form.targets.reverse()

    const request = modelRouteFormToRequest(form)

    expect(request).toMatchObject({
      model_pattern: 'long-model-pattern-*',
      owner_user_id: 7,
      scope: 'user',
      targets: [
        {
          endpoint_id: 'endpoint-b',
          upstream_model: 'model-b',
          native_api: 'auto',
          embedding_dimensions_override: null,
          dev_system_normalize: false,
        },
        {
          endpoint_id: 'endpoint-off-page',
          upstream_model: 'upstream-model-a',
          native_api: 'auto',
          embedding_dimensions_override: 1536,
          service_tier: 'priority',
          compact_mode: 'self_summarize',
          thinking_effort_override: 'high',
          thinking_downgrade_enabled: true,
          dev_system_normalize: true,
        },
      ],
    })
    expect(capturePayload(request).targets?.[1]).not.toHaveProperty(
      'proxy_url_override',
    )
    expect(capturePayload(request).targets?.[1]).not.toHaveProperty(
      'active_windows',
    )
  })

  it('resolves automatic target protocols from the resolved all-endpoint option', () => {
    const form = modelRouteToForm(route)
    expect(suggestProtocolForTarget(form.targets[0], () => 'embeddings')).toBe(
      'embeddings',
    )
    form.targets[0]!.native_api = 'chat'
    expect(suggestProtocolForTarget(form.targets[0], () => 'embeddings')).toBe(
      'chat',
    )
    form.targets[0]!.native_api = 'auto'
    expect(
      suggestProtocolForTarget(form.targets[0], () => 'auto'),
    ).toBeUndefined()
  })

  it('reorders the complete target object and cancels when a dragged target was removed', () => {
    const first = { endpoint: 'a', protocol: 'chat', model: 'm-a' }
    const second = { endpoint: 'b', protocol: 'embeddings', model: 'm-b' }
    const third = { endpoint: 'c', protocol: 'responses', model: 'm-c' }

    expect(
      reorderTargetByIdentity([first, second, third], first, third),
    ).toEqual([second, third, first])
    expect(reorderTargetByIdentity([second, third], first, third)).toBeNull()
  })

  it('projects result counts and patches only the matching cached route row', () => {
    const queryClient = new QueryClient()
    const key = modelRoutePageQueryKey(0, 10)
    queryClient.setQueryData(key, {
      first: 0,
      rows: 10,
      total: 1,
      routes: [route],
    })
    patchModelRoutePages(queryClient, { ...route, enabled: false })

    const page = queryClient.getQueryData<{ routes: ModelEndpointRule[] }>(key)
    expect(page?.routes[0]?.enabled).toBe(false)
    expect(
      modelRouteTestCounts([
        {
          duration_ms: 1,
          endpoint_id: 'a',
          endpoint_name: 'A',
          message: 'ok',
          ok: true,
          preferred: true,
          target_id: 't1',
        },
        {
          duration_ms: 1,
          endpoint_id: 'b',
          endpoint_name: 'B',
          message: 'no',
          ok: false,
          preferred: false,
          target_id: 't2',
        },
      ]),
    ).toEqual({ passed: 1, total: 2 })
  })

  it('keeps full accessible route values and result identity in row projection', () => {
    const result = createModelRouteListItemView(route, {
      endpointTestIdle: 'Not tested',
      modelRouteTestNoneEligible: 'No target',
      modelRouteTestPassed: (passed, total) => `${passed}/${total}`,
      ownerLabel: 'Owner',
      scopeAdmin: 'Public',
      scopeUser: 'Private',
      testResult: null,
      testingModelRouteId: '',
      togglingModelRouteId: '',
    })

    expect(result.model_pattern).toBe(route.model_pattern)
    expect(result.scope_label).toBe('Private')
    expect(result.targets[0]).toMatchObject({
      endpoint_label: 'Endpoint beyond first page',
      upstream_model: 'upstream-model-a',
    })
  })
})
