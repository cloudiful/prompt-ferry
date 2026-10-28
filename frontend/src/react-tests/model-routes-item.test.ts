import { describe, expect, it } from 'vitest'
import type {
  ModelEndpointRule,
  ModelRouteTestResponse,
  ModelRouteTestTargetResult,
} from '@/generated/admin-api'
import type { ModelRouteTargetItemView } from '@/models/endpoints'
import {
  createModelRouteListItemView,
  enabledRouteTargets,
  hiddenRouteTargetCount,
  modelRouteTestCounts,
  modelRouteTestSeverity,
  routeTargetOverride,
  routeTargetPairLabel,
  routeTargetPreview,
} from '@/models/endpoints/model-route-item'

function targetView(
  overrides: Partial<ModelRouteTargetItemView> = {},
): ModelRouteTargetItemView {
  return {
    target_id: 'target-1',
    endpoint_label: 'openai-platform',
    enabled: true,
    endpoint_enabled: true,
    upstream_model: null,
    ...overrides,
  }
}

function backendTarget(overrides: Record<string, unknown> = {}) {
  return {
    created_at: '2026-10-01T00:00:00Z',
    enabled: true,
    endpoint_enabled: true,
    endpoint_id: 'endpoint-1',
    endpoint_name: 'openai-platform',
    position: 0,
    rule_id: 'rule-1',
    target_id: 'target-1',
    updated_at: '2026-10-01T00:00:00Z',
    upstream_model: null,
    ...overrides,
  }
}

function ruleFixture(targets: unknown[]): ModelEndpointRule {
  return {
    created_at: '2026-10-01T00:00:00Z',
    enabled: true,
    model_pattern: 'gpt-*',
    rule_id: 'rule-1',
    scope: 'admin',
    updated_at: '2026-10-01T00:00:00Z',
    targets,
  } as unknown as ModelEndpointRule
}

const labels = {
  endpointTestIdle: 'Not tested',
  modelRouteTestNoneEligible: 'No target is currently active',
  modelRouteTestPassed: (passed: number, total: number) =>
    `${passed}/${total} targets passed`,
  ownerLabel: 'Owner',
  scopeAdmin: 'Public',
  scopeUser: 'Private',
}

function routeView(targets: unknown[]) {
  return createModelRouteListItemView(ruleFixture(targets), {
    ...labels,
    testResult: null,
    testingModelRouteId: '',
    togglingModelRouteId: '',
  })
}

function targetResult(
  overrides: Partial<ModelRouteTestTargetResult> = {},
): ModelRouteTestTargetResult {
  return {
    target_id: 'target-1',
    endpoint_id: 'endpoint-1',
    endpoint_name: 'openai-platform',
    model: 'gpt-5',
    ok: true,
    status: 200,
    duration_ms: 42,
    message: 'OK (responses)',
    preferred: false,
    ...overrides,
  }
}

function response(
  overrides: Partial<ModelRouteTestResponse> = {},
): ModelRouteTestResponse {
  return {
    ok: true,
    status: 200,
    duration_ms: 60,
    endpoint_id: 'endpoint-1',
    endpoint_name: 'openai-platform',
    preferred_endpoint_id: 'endpoint-1',
    preferred_endpoint_name: 'openai-platform',
    rule_id: 'rule-1',
    model_pattern: 'gpt-*',
    model: 'gpt-5',
    message: 'OK (responses)',
    results: [targetResult()],
    passed: 1,
    total: 1,
    ...overrides,
  }
}

function routeFixture(): ModelEndpointRule {
  return {
    created_at: '2026-09-04T00:00:00Z',
    enabled: true,
    model_pattern: 'gpt-*',
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
        service_tier: null,
        target_id: 'target-1',
        updated_at: '2026-09-04T00:00:00Z',
      },
    ],
  } as ModelEndpointRule
}

function view(result: ModelRouteTestResponse | null) {
  return createModelRouteListItemView(routeFixture(), {
    ...labels,
    testResult: result,
    testingModelRouteId: '',
    togglingModelRouteId: '',
  })
}

describe('model route item view', () => {
  it('keeps the configured target state in the row view', () => {
    const view = routeView([
      backendTarget({ target_id: 'target-1', enabled: true }),
      backendTarget({ target_id: 'target-2', enabled: false }),
    ])
    expect(view.targets.map((target) => target.enabled)).toEqual([true, false])
    expect(view.targets.map((target) => target.endpoint_enabled)).toEqual([
      true,
      true,
    ])
  })

  it('lists only enabled targets on enabled endpoints', () => {
    const targets = [
      targetView({ target_id: 'a', endpoint_label: 'enabled-endpoint' }),
      targetView({
        target_id: 'b',
        endpoint_label: 'disabled-target',
        enabled: false,
      }),
      targetView({
        target_id: 'c',
        endpoint_label: 'disabled-endpoint',
        endpoint_enabled: false,
      }),
      targetView({
        target_id: 'd',
        endpoint_label: 'orphan',
        endpoint_enabled: false,
      }),
    ]
    expect(
      enabledRouteTargets(targets).map((target) => target.target_id),
    ).toEqual(['a'])
  })

  it('keeps the configured order and lets duplicates stay distinct', () => {
    const targets = [
      targetView({
        target_id: 'a',
        endpoint_label: 'same',
        upstream_model: 'm-1',
      }),
      targetView({ target_id: 'b', endpoint_label: 'other' }),
      targetView({
        target_id: 'c',
        endpoint_label: 'same',
        upstream_model: 'm-2',
      }),
    ]
    expect(
      enabledRouteTargets(targets).map((target) => target.target_id),
    ).toEqual(['a', 'b', 'c'])
    expect(
      enabledRouteTargets(targets).map((target) => routeTargetOverride(target)),
    ).toEqual(['m-1', null, 'm-2'])
  })

  it('treats a blank override as no override', () => {
    expect(
      routeTargetOverride(targetView({ upstream_model: '   ' })),
    ).toBeNull()
    expect(routeTargetOverride(targetView({ upstream_model: '' }))).toBeNull()
    expect(routeTargetOverride(targetView({ upstream_model: null }))).toBeNull()
    expect(routeTargetOverride(targetView({ upstream_model: ' gpt-5 ' }))).toBe(
      'gpt-5',
    )
    expect(routeTargetPairLabel(targetView({ upstream_model: '   ' }))).toBe(
      'openai-platform',
    )
    expect(
      routeTargetPairLabel(
        targetView({ endpoint_label: 'azure-openai', upstream_model: 'gpt-5' }),
      ),
    ).toBe('azure-openai · gpt-5')
  })

  it('shows one pair at most and counts the rest', () => {
    const many = Array.from({ length: 5 }, (_unused, index) =>
      targetView({ target_id: `target-${index}` }),
    )
    expect(routeTargetPreview([])).toEqual([])
    expect(routeTargetPreview(many.slice(0, 1))).toHaveLength(1)
    expect(routeTargetPreview(many.slice(0, 2))).toHaveLength(1)
    expect(routeTargetPreview(many)).toHaveLength(1)
    expect(hiddenRouteTargetCount([])).toBe(0)
    expect(hiddenRouteTargetCount(many.slice(0, 1))).toBe(0)
    expect(hiddenRouteTargetCount(many)).toBe(4)
  })

  it('never counts a disabled target towards the disclosure', () => {
    const targets = [
      targetView({ target_id: 'a' }),
      targetView({ target_id: 'b', enabled: false }),
      targetView({ target_id: 'c', endpoint_enabled: false }),
    ]
    expect(hiddenRouteTargetCount(targets)).toBe(0)
    expect(routeTargetPreview(targets)).toHaveLength(1)
  })

  it('lets a toggled target join and leave the summary', () => {
    const rule = ruleFixture([
      backendTarget({ target_id: 'target-1', enabled: false }),
      backendTarget({ target_id: 'target-2', enabled: true }),
    ])
    const before = routeView((rule.targets ?? []) as unknown[])
    expect(enabledRouteTargets(before.targets)).toHaveLength(1)

    const toggled = routeView([
      backendTarget({ target_id: 'target-1', enabled: true }),
      backendTarget({ target_id: 'target-2', enabled: true }),
    ])
    expect(enabledRouteTargets(toggled.targets)).toHaveLength(2)
    expect(hiddenRouteTargetCount(toggled.targets)).toBe(1)
  })

  it('marks every tested target passing as a green result', () => {
    const result = response({
      results: [
        targetResult(),
        targetResult({ target_id: 'target-2', endpoint_id: 'endpoint-2' }),
      ],
      passed: 2,
      total: 2,
    })
    expect(modelRouteTestSeverity(result)).toBe('success')
    expect(view(result).test_severity).toBe('success')
    expect(view(result).test_message).toContain('2/2 targets passed')
  })

  it('marks some targets passing as an amber result', () => {
    const result = response({
      ok: false,
      results: [
        targetResult(),
        targetResult({
          target_id: 'target-2',
          endpoint_id: 'endpoint-2',
          ok: false,
          status: 502,
          message: 'upstream failure',
        }),
      ],
      passed: 1,
      total: 2,
    })
    expect(modelRouteTestSeverity(result)).toBe('warning')
    expect(view(result).test_severity).toBe('warning')
    expect(view(result).test_message).toContain('1/2 targets passed')
  })

  it('marks no target passing as a red result', () => {
    const result = response({
      ok: false,
      results: [
        targetResult({
          ok: false,
          status: 502,
          message: 'upstream failure',
        }),
      ],
      passed: 0,
      total: 1,
    })
    expect(modelRouteTestSeverity(result)).toBe('error')
    expect(view(result).test_severity).toBe('error')
  })

  it('names the failing target in the summary of a partial failure', () => {
    const result = response({
      ok: false,
      results: [
        targetResult({ preferred: true }),
        targetResult({
          target_id: 'target-2',
          endpoint_id: 'endpoint-2',
          ok: false,
          status: 502,
          message: 'upstream failure detail',
        }),
      ],
      passed: 1,
      total: 2,
    })
    const message = view(result).test_message
    expect(message).toContain('1/2 targets passed')
    expect(message).toContain('upstream failure detail')
  })

  it('never marks a route without an eligible target as green', () => {
    const result = response({
      ok: false,
      status: null,
      endpoint_id: null,
      endpoint_name: null,
      model: null,
      message: 'no enabled target of this route is currently active',
      results: [],
      passed: 0,
      total: 0,
    })
    expect(modelRouteTestSeverity(result)).toBe('warning')
    expect(view(result).test_severity).toBe('warning')
    expect(view(result).test_message).toBe(labels.modelRouteTestNoneEligible)
  })

  it('keeps an untested route neutral', () => {
    expect(modelRouteTestSeverity(null)).toBeNull()
    expect(view(null).test_severity).toBeNull()
    expect(view(null).test_message).toBe(labels.endpointTestIdle)
  })

  it('keeps every target outcome for the details dialog', () => {
    const result = response({
      results: [
        targetResult(),
        targetResult({
          target_id: 'target-2',
          endpoint_id: 'endpoint-2',
          endpoint_name: 'azure-openai',
        }),
      ],
      passed: 2,
      total: 2,
    })
    const item = view(result)
    expect(item.test_results.map((entry) => entry.target_id)).toEqual([
      'target-1',
      'target-2',
    ])
    expect(modelRouteTestCounts(item.test_results)).toEqual({
      passed: 2,
      total: 2,
    })
  })
})
