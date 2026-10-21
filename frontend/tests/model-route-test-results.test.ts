import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import type {
  ModelEndpointRule,
  ModelRouteTestResponse,
  ModelRouteTestTargetResult,
} from '../src/generated/admin-api'
import {
  createModelRouteListItemView,
  modelRouteTestCounts,
  modelRouteTestSeverity,
} from '../src/models/endpoints/model-route-item'

// Issue #1004 P2: a model-route test reports one outcome per eligible target.
// The row shows the aggregate, and the dialog lists every outcome.

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

const labels = {
  endpointTestIdle: 'Not tested',
  modelRouteTestNoneEligible: 'No target is currently active',
  modelRouteTestPassed: (passed: number, total: number) =>
    `${passed}/${total} targets passed`,
  ownerLabel: 'Owner',
  scopeAdmin: 'Public',
  scopeUser: 'Private',
}

function view(result: ModelRouteTestResponse | null) {
  return createModelRouteListItemView(routeFixture(), {
    ...labels,
    testResult: result,
    testingModelRouteId: '',
    togglingModelRouteId: '',
  })
}

test('every tested target passing is a green result', () => {
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

test('some targets passing is an amber result', () => {
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

test('no target passing is a red result', () => {
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

test('the summary of a partial failure names the failing target', () => {
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
  // The failing outcome carries the reason, so it leads the summary.
  expect(message).toContain('upstream failure detail')
})

test('a route without an eligible target is never green', () => {
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

test('an untested route stays neutral', () => {
  expect(modelRouteTestSeverity(null)).toBeNull()
  expect(view(null).test_severity).toBeNull()
  expect(view(null).test_message).toBe(labels.endpointTestIdle)
})

test('the row keeps every target outcome for the details dialog', () => {
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

type Component = Parameters<typeof h>[0]

function rewriteSfcImports(code: string): string {
  return code.replace(
    /import\s+([^;'"]+?)\s+from\s+(['"])([^'"]+)\2;?/g,
    (_match, clause: string, _quote: string, spec: string) => {
      if (clause.trim().startsWith('type ')) return ''
      const statements: string[] = []
      const named = clause.match(/\{([^}]*)\}/)
      const defaults = clause
        .replace(/\{[^}]*\}/, '')
        .split(',')
        .map((part) => part.trim())
        .filter((part) => part && !part.startsWith('type '))
      for (const fallback of defaults) {
        statements.push(
          `const ${fallback} = __resolve(${JSON.stringify(spec)});`,
        )
      }
      for (const part of named?.[1].split(',') ?? []) {
        const entry = part.trim()
        if (!entry || entry.startsWith('type ')) continue
        const [source, local = source] = entry.split(/\s+as\s+/)
        statements.push(
          `const ${local.trim()} = __resolve(${JSON.stringify(spec)})[${JSON.stringify(source.trim())}];`,
        )
      }
      return statements.join(' ')
    },
  )
}

function slotStub(name: string, slot = 'default') {
  return {
    name,
    render(this: { $slots: Record<string, (() => unknown) | undefined> }) {
      return h('div', { 'data-stub': name }, this.$slots[slot]?.())
    },
  }
}

const dialogSource = await Bun.file(
  new URL(
    '../src/components/endpoints/ModelRouteTestResultsDialog.vue',
    import.meta.url,
  ),
).text()

const dialogComponent: Component = (() => {
  const { descriptor } = parse(dialogSource, {
    filename: 'ModelRouteTestResultsDialog.vue',
  })
  const script = compileScript(descriptor, {
    id: 'model-route-test-results',
    inlineTemplate: true,
  })
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    rewriteSfcImports(script.content).replace(/export default/, 'return'),
  )
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return vue
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve) as Component
})()

async function renderDialog(
  results: ModelRouteTestTargetResult[],
): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(dialogComponent, {
        results,
        t: (key: string) => key,
        visible: true,
        'onUpdate:visible': () => {},
      }),
  })
  app.component('UModal', slotStub('UModal', 'body'))
  app.component('UButton', slotStub('UButton'))
  app.component('UIcon', slotStub('UIcon'))
  return renderToString(app)
}

test('the dialog lists every target outcome with its node, model, status and error', async () => {
  const html = await renderDialog([
    targetResult(),
    targetResult({
      target_id: 'target-2',
      endpoint_id: 'endpoint-2',
      endpoint_name: 'azure-openai',
      model: 'gpt-5-mini',
      ok: false,
      status: 502,
      duration_ms: 1200,
      message: 'upstream failure detail',
    }),
  ])
  expect(html).toContain('openai-platform')
  expect(html).toContain('azure-openai')
  expect(html).toContain('gpt-5-mini')
  expect(html).toContain('HTTP 502')
  expect(html).toContain('1200ms')
  expect(html).toContain('upstream failure detail')
})
