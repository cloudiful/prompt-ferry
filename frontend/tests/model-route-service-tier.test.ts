import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import {
  createEmptyModelRouteForm,
  modelRouteFormToRequest,
  modelRouteToForm,
} from '../src/admin-mappers/forms/model-route'
import {
  hasTargetServiceTier,
  hasTargetSettings,
} from '../src/components/endpoints/modelRouteTargetHelpers'
import type {
  EndpointProvider,
  ModelEndpointRule,
} from '../src/generated/admin-api'
import type { ModelRouteTargetForm } from '../src/models'
import * as serviceTierHelpers from '../src/models/endpoints/service-tier'

function routeFixture(
  service_tier: string | null | undefined,
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
        service_tier,
        target_id: 'target-1',
        updated_at: '2026-09-04T00:00:00Z',
      },
    ],
  } as ModelEndpointRule
}

test('new model-route target forms default to inherit', () => {
  expect(createEmptyModelRouteForm().targets[0]?.service_tier).toBeNull()
})

test('modelRouteToForm preserves a configured target tier', () => {
  expect(
    modelRouteToForm(routeFixture('priority')).targets[0]?.service_tier,
  ).toBe('priority')
  // Free-form values survive so provider-specific vocabularies (e.g.
  // OpenAI `fast`) are never coerced.
  expect(modelRouteToForm(routeFixture('fast')).targets[0]?.service_tier).toBe(
    'fast',
  )
  expect(
    modelRouteToForm(routeFixture('standard')).targets[0]?.service_tier,
  ).toBe('standard')
})

test('modelRouteToForm inherits missing, null and blank target tiers', () => {
  expect(
    modelRouteToForm(routeFixture(null)).targets[0]?.service_tier,
  ).toBeNull()
  expect(modelRouteToForm(routeFixture('')).targets[0]?.service_tier).toBeNull()
  expect(
    modelRouteToForm(routeFixture('   ')).targets[0]?.service_tier,
  ).toBeNull()
  const rule = routeFixture('priority')
  const { service_tier, ...bareTarget } = rule.targets[0] ?? {}
  void service_tier
  const withoutTier = {
    ...rule,
    targets: [bareTarget],
  } as unknown as ModelEndpointRule
  expect(modelRouteToForm(withoutTier).targets[0]?.service_tier).toBeNull()
})

test('modelRouteFormToRequest round-trips the target tier', () => {
  // Issue #637: always sent (null means inherit) so a save or enable
  // toggle preserves a configured override instead of clearing it.
  const request = modelRouteFormToRequest(
    modelRouteToForm(routeFixture('priority')),
  )
  expect(request.targets?.[0]?.service_tier).toBe('priority')
  const blank = modelRouteToForm(routeFixture('priority'))
  blank.targets[0].service_tier = '   '
  expect(modelRouteFormToRequest(blank).targets?.[0]?.service_tier).toBeNull()
  const inherit = modelRouteFormToRequest(modelRouteToForm(routeFixture(null)))
  expect(inherit.targets?.[0]?.service_tier).toBeNull()
})

test('a configured target tier lights up the target settings gear', () => {
  const target = createEmptyModelRouteForm().targets[0]
  if (!target) throw new Error('expected a default target')
  expect(hasTargetServiceTier(target)).toBe(false)
  expect(hasTargetSettings(target)).toBe(false)

  target.service_tier = 'priority'
  expect(hasTargetServiceTier(target)).toBe(true)
  expect(hasTargetSettings(target)).toBe(true)

  // Whitespace-only is inherit, so it must not highlight the gear.
  target.service_tier = '   '
  expect(hasTargetServiceTier(target)).toBe(false)
  expect(hasTargetSettings(target)).toBe(false)

  target.service_tier = null
  expect(hasTargetSettings(target)).toBe(false)
})

// Issue #644: SSR harness for the route-target settings subpage. The control is
// exposed for every provider on the HTTP JSON protocols; the endpoint provider
// no longer gates it.
type Component = Parameters<typeof h>[0]

function rewriteSfcImports(code: string): string {
  return code.replace(
    /import\s+([^;'"]+?)\s+from\s+(['"])([^'"]+)\2;?/g,
    (_match, clause: string, _quote: string, spec: string) => {
      if (clause.trim().startsWith('type ')) return ''
      const named = clause.match(/\{([^}]*)\}/)
      const fallback = clause
        .replace(/\{[^}]*\}/, '')
        .replace(/^,|,$/g, '')
        .trim()
      const statements: string[] = []
      if (fallback) {
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

function slotStub(name: string) {
  return {
    name,
    render(this: { $slots: Record<string, (() => unknown) | undefined> }) {
      return h('div', { 'data-stub': name }, this.$slots.default?.())
    },
  }
}

const targetSettingsSource = await Bun.file(
  new URL(
    '../src/components/endpoints/ModelRouteTargetSettingsPage.vue',
    import.meta.url,
  ),
).text()

const settingsPageComponent: Component = (() => {
  const { descriptor } = parse(targetSettingsSource, {
    filename: 'ModelRouteTargetSettingsPage.vue',
  })
  const script = compileScript(descriptor, {
    id: 'model-route-target-settings',
    inlineTemplate: true,
  })
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    rewriteSfcImports(script.content).replace(/export default/, 'return'),
  )
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return vue
    if (spec === '@/models/endpoints/service-tier') return serviceTierHelpers
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve) as Component
})()

const TIER_MARKER = 'data-stub="ServiceTierOverrideField.vue"'

async function renderTargetSettings(
  target: ModelRouteTargetForm,
  endpointProvider: EndpointProvider,
): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(settingsPageComponent, {
        target,
        endpointProvider,
        t: (key: string) => key,
      }),
  })
  app.component('USelect', slotStub('USelect'))
  app.component('USwitch', slotStub('USwitch'))
  return renderToString(app)
}

function targetForm(native_api: ModelRouteTargetForm['native_api']) {
  const form = createEmptyModelRouteForm()
  const target = form.targets[0]
  if (!target) throw new Error('expected a default target')
  target.native_api = native_api
  return target
}

test('the target settings subpage exposes the tier for every provider on JSON protocols', async () => {
  for (const provider of [
    'generic',
    'minimax',
    'command_code',
    'opencode_go',
    'openrouter',
    'glm',
    'deepseek',
    'openai',
  ] as const) {
    for (const native_api of [
      'chat',
      'responses',
      'anthropic_messages',
    ] as const) {
      const html = await renderTargetSettings(targetForm(native_api), provider)
      expect(html).toContain(TIER_MARKER)
    }
    // Auto resolves at request time, so it stays eligible.
    expect(await renderTargetSettings(targetForm('auto'), provider)).toContain(
      TIER_MARKER,
    )
  }
})

test('the target settings subpage hides the tier on Realtime', async () => {
  const html = await renderTargetSettings(targetForm('realtime'), 'minimax')
  expect(html).not.toContain(TIER_MARKER)
})
