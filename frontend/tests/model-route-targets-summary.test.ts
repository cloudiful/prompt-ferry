import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import type { ModelEndpointRule } from '../src/generated/admin-api'
import { messages } from '../src/i18n'
import type { ModelRouteTargetItemView } from '../src/models/endpoints'
import * as modelRouteItem from '../src/models/endpoints/model-route-item'
import {
  createModelRouteListItemView,
  enabledRouteTargets,
  hiddenRouteTargetCount,
  routeTargetOverride,
  routeTargetPairLabel,
  routeTargetPreview,
} from '../src/models/endpoints/model-route-item'

// Issue #1051 P1: a route row shows the enabled configured targets and their
// upstream model override in a bounded layout. Disabled targets, targets on a
// disabled endpoint and targets whose endpoint is gone never reach the
// summary, and the configured order is kept.

type Component = Parameters<typeof h>[0]

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

function translator(locale: 'zh-CN' | 'en-US') {
  return (key: string, params?: Record<string, unknown>): string => {
    const template = (messages[locale] as Record<string, string>)[key]
    if (!template) return key
    if (!params) return template
    return template.replace(/\{(\w+)\}/g, (match, name: string) =>
      params[name] === undefined ? match : String(params[name]),
    )
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

function routeView(targets: unknown[]) {
  return createModelRouteListItemView(ruleFixture(targets), {
    endpointTestIdle: 'Not tested',
    modelRouteTestNoneEligible: 'No target is currently active',
    modelRouteTestPassed: (passed: number, total: number) =>
      `${passed}/${total} targets passed`,
    ownerLabel: 'Owner',
    scopeAdmin: 'Public',
    scopeUser: 'Private',
    testResult: null,
    testingModelRouteId: '',
    togglingModelRouteId: '',
  })
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

test('the target view keeps the configured target state', () => {
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

test('only enabled targets on enabled endpoints are listed', () => {
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

test('the configured order is kept and duplicates stay distinct', () => {
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

test('a blank override is no override', () => {
  expect(routeTargetOverride(targetView({ upstream_model: '   ' }))).toBeNull()
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

test('the summary shows two pairs at most and counts the rest', () => {
  const many = Array.from({ length: 5 }, (_unused, index) =>
    targetView({ target_id: `target-${index}` }),
  )
  expect(routeTargetPreview([])).toEqual([])
  expect(routeTargetPreview(many.slice(0, 1))).toHaveLength(1)
  expect(routeTargetPreview(many.slice(0, 2))).toHaveLength(2)
  expect(routeTargetPreview(many)).toHaveLength(2)
  expect(hiddenRouteTargetCount([])).toBe(0)
  expect(hiddenRouteTargetCount(many.slice(0, 2))).toBe(0)
  expect(hiddenRouteTargetCount(many)).toBe(3)
})

test('a disabled target never counts towards the disclosure', () => {
  const targets = [
    targetView({ target_id: 'a' }),
    targetView({ target_id: 'b', enabled: false }),
    targetView({ target_id: 'c', endpoint_enabled: false }),
  ]
  expect(hiddenRouteTargetCount(targets)).toBe(0)
  expect(routeTargetPreview(targets)).toHaveLength(1)
})

test('a toggled target joins and leaves the summary', () => {
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
  expect(hiddenRouteTargetCount(toggled.targets)).toBe(0)
})

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

// The disclosure renders its content next to the trigger, so the rendered
// markup shows the whole list the popover holds.
const popoverStub = {
  name: 'UPopover',
  render(this: { $slots: Record<string, (() => unknown) | undefined> }) {
    return h('div', { 'data-stub': 'UPopover' }, [
      this.$slots.default?.(),
      h('div', { 'data-stub': 'UPopoverContent' }, this.$slots.content?.()),
    ])
  },
}

const buttonStub = {
  name: 'UButton',
  render(this: { $slots: Record<string, (() => unknown) | undefined> }) {
    return h('button', this.$slots.default?.())
  },
}

const vueModule = new Proxy(vue as unknown as Record<string, unknown>, {
  get(target, property: string) {
    if (property in target) return target[property]
    if (property === '_defineComponent') return target.defineComponent
    return undefined
  },
})

const summarySource = await Bun.file(
  new URL(
    '../src/components/endpoints/ModelRouteTargetsSummary.vue',
    import.meta.url,
  ),
).text()

const summaryComponent: Component = (() => {
  const { descriptor } = parse(summarySource, {
    filename: 'ModelRouteTargetsSummary.vue',
  })
  const script = compileScript(descriptor, {
    id: 'model-route-targets-summary',
    inlineTemplate: true,
  })
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    rewriteSfcImports(script.content).replace(/export default/, 'return'),
  )
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return vueModule
    if (spec === '@/models/endpoints/model-route-item') return modelRouteItem
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve) as Component
})()

async function renderSummary(
  targets: ModelRouteTargetItemView[],
  locale: 'zh-CN' | 'en-US' = 'en-US',
): Promise<string> {
  const app = createSSRApp({
    render: () => h(summaryComponent, { targets, t: translator(locale) }),
  })
  app.component('UPopover', popoverStub)
  app.component('UButton', buttonStub)
  return renderToString(app)
}

test('the summary shows the first two enabled pairs with their override', async () => {
  const html = await renderSummary([
    targetView({ target_id: 'a', endpoint_label: 'openai-platform' }),
    targetView({
      target_id: 'b',
      endpoint_label: 'azure-openai',
      upstream_model: 'gpt-5',
    }),
    targetView({ target_id: 'c', endpoint_label: 'third-endpoint' }),
  ])
  expect(html).toContain('openai-platform')
  expect(html).toContain('azure-openai')
  expect(html).toContain('gpt-5')
  // The third pair stays in the disclosure only.
  const preview = html.slice(0, html.indexOf('data-stub="UPopoverContent"'))
  expect(preview).not.toContain('third-endpoint')
  expect(preview).toContain('+1')
})

test('the disclosure names the remaining targets and holds every enabled pair', async () => {
  const html = await renderSummary([
    targetView({ target_id: 'a', endpoint_label: 'openai-platform' }),
    targetView({ target_id: 'b', endpoint_label: 'azure-openai' }),
    targetView({ target_id: 'c', endpoint_label: 'third-endpoint' }),
    targetView({
      target_id: 'd',
      endpoint_label: 'fourth-endpoint',
      upstream_model: 'gpt-5-mini',
    }),
  ])
  expect(html).toContain('+2')
  expect(html).toContain('aria-label="+2 enabled targets"')
  // The disclosure holds the whole ordered list, the visible pairs included.
  const content = html.slice(html.indexOf('data-stub="UPopoverContent"'))
  expect(content).toContain('openai-platform')
  expect(content).toContain('azure-openai')
  expect(content).toContain('third-endpoint')
  expect(content).toContain('fourth-endpoint')
  expect(content).toContain('gpt-5-mini')
  expect(content.indexOf('openai-platform')).toBeLessThan(
    content.indexOf('fourth-endpoint'),
  )
})

test('a route without an enabled target says so', async () => {
  const html = await renderSummary([
    targetView({ target_id: 'a', enabled: false }),
    targetView({ target_id: 'b', endpoint_enabled: false }),
  ])
  expect(html).toContain('No enabled targets')
  expect(html).not.toContain('openai-platform')
})

test('a route without targets keeps the dash', async () => {
  const html = await renderSummary([])
  expect(html).toContain('-')
  expect(html).not.toContain('No enabled targets')
})

test('the empty state is localized', async () => {
  const html = await renderSummary(
    [targetView({ target_id: 'a', enabled: false })],
    'zh-CN',
  )
  expect(html).toContain('无启用目标')
  expect(
    (messages['zh-CN'] as Record<string, string>).modelRouteNoEnabledTargets,
  ).toBe('无启用目标')
  expect(
    (messages['en-US'] as Record<string, string>).modelRouteNoEnabledTargets,
  ).toBe('No enabled targets')
})

test('the table keeps the ownership badge outside the target summary', async () => {
  const table = await Bun.file(
    new URL(
      '../src/components/endpoints/ModelRoutesTable.vue',
      import.meta.url,
    ),
  ).text()
  const cell = table.slice(
    table.indexOf('#targets-cell'),
    table.indexOf('#status-cell'),
  )
  expect(cell).toContain('<ModelRouteTargetsSummary')
  expect(cell).toContain('row.original.owner_label')
  // The summary is a sibling of the badge, not its container.
  expect(cell.indexOf('row.original.owner_label')).toBeGreaterThan(
    cell.indexOf('/>'),
  )
  // The old all-target scroller is gone from the target cell.
  expect(cell).not.toContain('overflow-x-auto')
  expect(cell).not.toContain('row.original.targets.length')
})

test('the mobile card shows the same bounded summary', async () => {
  const card = await Bun.file(
    new URL(
      '../src/components/endpoints/ModelRouteMobileCard.vue',
      import.meta.url,
    ),
  ).text()
  expect(card).toContain('<ModelRouteTargetsSummary')
  // The comma joined target list is gone.
  expect(card).not.toContain("join(', ')")
})

test('the editor keeps every configured target', async () => {
  const dialog = await Bun.file(
    new URL(
      '../src/components/endpoints/ModelRouteDialog.vue',
      import.meta.url,
    ),
  ).text()
  // The editor still lists all configured targets and keeps their own enabled
  // switch, so a disabled target stays editable.
  expect(dialog).toContain(':data="form?.targets ?? []"')
  expect(dialog).toContain('v-model="row.original.enabled"')
})

test('the preview slots and the pair tracks are bounded', async () => {
  const source = await Bun.file(
    new URL(
      '../src/components/endpoints/ModelRouteTargetsSummary.vue',
      import.meta.url,
    ),
  ).text()
  // The desktop row gives both preview slots the same bounded space, so a
  // long second pair cannot starve the first one, and the +N button keeps
  // its own auto track.
  expect(source).toContain('md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]')
  expect(source).toContain('md:grid-cols-[minmax(0,1fr)_auto]')
  // A pair with an override keeps a bounded track for the override, so it
  // never collapses, and the endpoint track stays shrinkable so it ellipsizes
  // instead of being clipped by the chip.
  expect(source).toContain('grid-cols-[minmax(0,auto)_auto_minmax(2rem,1fr)]')
  // A pair without an override uses the whole chip for its endpoint.
  expect(source).toContain('grid-cols-[minmax(0,1fr)]')
  // The endpoint segment stays shrinkable, so it ellipsizes inside its track
  // instead of being clipped by the chip.
  expect(source).toContain('<span class="min-w-0 truncate">{{ pair.endpoint }}')
  expect(source).not.toContain('shrink-0 truncate')
})

test('a pair without an override renders the endpoint alone', async () => {
  const html = await renderSummary([
    targetView({ target_id: 'a', endpoint_label: 'openai-platform' }),
  ])
  const chip = html.slice(html.indexOf('title="openai-platform"'))
  expect(chip).toContain('openai-platform')
  expect(chip).not.toContain('·')
})
