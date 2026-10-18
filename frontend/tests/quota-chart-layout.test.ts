import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, defineComponent, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import type {
  QuotaKeyObservationTrendPoint,
  QuotaKeyObservationTrendSeries,
} from '../src/generated/admin-api'

const storage = new Map<string, string>()
Object.defineProperty(globalThis, 'localStorage', {
  value: {
    getItem: (key: string) => storage.get(key) ?? null,
    setItem: (key: string, value: string) => {
      storage.set(key, value)
    },
    removeItem: (key: string) => {
      storage.delete(key)
    },
    clear: () => {
      storage.clear()
    },
  },
  configurable: true,
})

const charts = await import('../src/quota-observation-charts')
const datetime = await import('../src/format/datetime')
const appTheme = await import('../src/theme/appTheme')

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyOption = Record<string, any>

const KINDS = {
  five_hour: '5-hour window',
  rolling: 'Rolling window',
  weekly: 'Weekly window',
  monthly: 'Monthly window',
  balance: 'Balance',
  unlimited: 'Unlimited',
  custom: 'Custom window',
} as const

function point(
  at: string,
  value: number | null,
): QuotaKeyObservationTrendPoint {
  return { at, observed_at: value == null ? null : at, value }
}

function series(
  overrides: Partial<QuotaKeyObservationTrendSeries> = {},
): QuotaKeyObservationTrendSeries {
  return {
    key_id: 'key-a',
    key_label: 'Key A',
    source_id: 'glm:five_hour',
    kind: 'five_hour',
    unit: 'percent',
    metric: 'remaining_percent',
    points: [point('2026-10-16T00:00:00Z', 80)],
    ...overrides,
  }
}

function optionInput() {
  return {
    range: '24h' as const,
    formatAxisDate: (value: string) => value,
    formatAxisTime: (value: string) => value,
    formatTooltipTime: (value: string) => value,
    formatAmount: (value: number, unit: string) => `${value} ${unit}`,
    labels: { percentSuffix: '%', unavailable: 'n/a', kinds: { ...KINDS } },
  }
}

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
      const namespace = clause.trim().match(/^\*\s+as\s+(\w+)$/)
      if (namespace) {
        statements.push(
          `const ${namespace[1]} = __resolve(${JSON.stringify(spec)});`,
        )
        return statements.join(' ')
      }
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

function compile(
  source: string,
  id: string,
  resolve: (spec: string) => unknown,
): unknown {
  const { descriptor } = parse(source, { filename: `${id}.vue` })
  const script = compileScript(descriptor, { id, inlineTemplate: true })
  const rewritten = rewriteSfcImports(script.content).replace(
    /export default/,
    'return',
  )
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(rewritten)
  return new Function('__resolve', `"use strict";\n${js}`)(resolve)
}

function slotStub(name: string) {
  return defineComponent({
    name,
    setup(_props, { slots }) {
      return () => h('div', { 'data-stub': name }, slots.default?.())
    },
  })
}

const chartStub = defineComponent({
  name: 'UsageChart',
  props: { option: { type: Object, default: null } },
  setup() {
    return () => h('div', { 'data-stub': 'UsageChart' })
  },
})

const instrumentedVue = {
  ...vue,
  defineAsyncComponent: () => chartStub,
}

const chartComponentSource = await Bun.file(
  new URL(
    '../src/components/endpoints/KeyQuotaObservationChart.vue',
    import.meta.url,
  ),
).text()

const chartComponent = compile(
  chartComponentSource,
  'quota-chart-layout',
  (spec) => {
    if (spec === 'vue') return instrumentedVue
    if (spec === '@/quota-observation-charts') return charts
    if (spec === '@/format/datetime') return datetime
    if (spec === '@/theme/appTheme') return appTheme
    return slotStub(spec.split('/').pop() ?? spec)
  },
)

async function renderChart(): Promise<string> {
  const labels: Record<string, string> = {
    tokenPlanInterval: '5-hour window',
    tokenPlanMonthly: 'Monthly window',
    tokenPlanWeekly: 'Weekly window',
  }
  const app = createSSRApp({
    render: () =>
      h(chartComponent as never, {
        trend: {
          bucket_count: 96,
          bucket_seconds: 900,
          failed_observations: 0,
          from: '2026-10-15T00:00:00Z',
          keys_truncated: false,
          range: '24h',
          sampled_observations: 3,
          series: [
            series({
              key_id: 'key-a',
              key_label:
                'A long key label that must stay readable at the narrow width',
              kind: 'weekly',
            }),
            series({
              key_id: 'key-b',
              key_label:
                'Another long key label that must stay readable at the width',
              kind: 'monthly',
            }),
          ],
          series_truncated: false,
          to: '2026-10-16T00:00:00Z',
        },
        loading: false,
        failed: false,
        range: '24h',
        t: (key: string) => labels[key] ?? key,
      }),
  })
  app.component('UProgress', slotStub('UProgress') as never)
  app.component('UButton', slotStub('UButton') as never)
  return renderToString(app)
}

test('the built-in canvas legend is disabled instead of drawing a clipped pager', () => {
  const option = charts.createQuotaTrendPanelOption(
    charts.groupQuotaTrendSeries([
      series({ key_id: 'a', key_label: 'Long key label one', kind: 'weekly' }),
      series({ key_id: 'b', key_label: 'Long key label two', kind: 'weekly' }),
      series({
        key_id: 'c',
        key_label: 'Long key label three',
        kind: 'weekly',
      }),
    ])[0],
    optionInput(),
  ) as AnyOption

  expect(option.legend.show).toBe(false)
  expect(option.legend.type).toBeUndefined()
  expect(option.legend.width).toBeUndefined()
  expect(option.series).toHaveLength(3)
})

test('the native legend list is bounded and keeps every full series name', async () => {
  const html = await renderChart()
  // A bounded vertical scroller instead of a clipped canvas pager.
  expect(html).toContain('max-h-24')
  expect(html).toContain('overflow-y-auto')
  expect(html).toContain('whitespace-normal')
  // Full names from the shared naming helper, not truncated canvas text.
  expect(html).toContain(
    'A long key label that must stay readable at the narrow width · Weekly window',
  )
  expect(html).toContain(
    'Another long key label that must stay readable at the width · Monthly window',
  )
})

test('the chart grid wrapper and panels can shrink below the canvas width', async () => {
  const html = await renderChart()
  const section = html.match(/<section class="([^"]*)"/)
  expect(section?.[1]).toContain('min-w-0')
  // Both the outer grid and the panel grid carry the shrink guard.
  expect(html).toContain('grid min-w-0 gap-3')
  expect(html.match(/min-w-0/g)?.length ?? 0).toBeGreaterThanOrEqual(2)
  expect(html).toContain('data-stub="UsageChart"')
})

const usageChartSource = await Bun.file(
  new URL('../src/components/usage/UsageChart.vue', import.meta.url),
).text()

const usageChartComponent = compile(
  usageChartSource,
  'usage-chart-layout',
  (spec) => {
    if (spec === 'vue') return vue
    if (spec === 'echarts/core') return { use: () => {}, init: () => null }
    if (spec === 'echarts/charts') return { BarChart: {}, LineChart: {} }
    if (spec === 'echarts/components') {
      return { GridComponent: {}, LegendComponent: {}, TooltipComponent: {} }
    }
    if (spec === 'echarts/renderers') return { CanvasRenderer: {} }
    if (spec === '@/theme/appTheme') return appTheme
    return slotStub(spec.split('/').pop() ?? spec)
  },
)

test('the shared UsageChart root keeps its height and can shrink', async () => {
  const app = createSSRApp({
    render: () =>
      h(usageChartComponent as never, {
        option: { series: [] },
      }),
  })
  const html = await renderToString(app)
  const root = html.match(/<div class="([^"]*)"/)
  expect(root?.[1]).toContain('h-[20rem]')
  expect(root?.[1]).toContain('w-full')
  expect(root?.[1]).toContain('min-w-0')
  // No chart-width prop: layout is owned by the panel, not the component.
  expect(html).not.toContain('data-width')
})

// --- Interaction harness: real native legend buttons -------------------------
// The repo has no DOM test environment, so the SFC is mounted through Vue's
// custom renderer. Buttons and their aria-pressed state are read from the host
// tree, and their compiled click handler is invoked to prove the toggle.

const toggleOptions: AnyOption[] = []
const toggleChartStub = defineComponent({
  name: 'UsageChart',
  props: { option: { type: Object, default: null } },
  setup(props) {
    return () => {
      if (props.option) toggleOptions.push(props.option as AnyOption)
      return h('div', { 'data-stub': 'UsageChart' })
    }
  },
})
const toggleVue = {
  ...vue,
  defineAsyncComponent: () => toggleChartStub,
}

const toggleComponent = compile(
  chartComponentSource,
  'quota-chart-layout-toggle',
  (spec) => {
    if (spec === 'vue') return toggleVue
    if (spec === '@/quota-observation-charts') return charts
    if (spec === '@/format/datetime') return datetime
    if (spec === '@/theme/appTheme') return appTheme
    return slotStub(spec.split('/').pop() ?? spec)
  },
)

type HostNode = {
  type?: string
  text?: string
  props: Record<string, unknown>
  children: HostNode[]
  parent: HostNode | null
}

function toggleHostOps() {
  const make = (type?: string, text?: string): HostNode => ({
    type,
    text,
    props: {},
    children: [],
    parent: null,
  })
  const detach = (child: HostNode) => {
    if (child.parent) {
      const at = child.parent.children.indexOf(child)
      if (at >= 0) child.parent.children.splice(at, 1)
      child.parent = null
    }
  }
  return {
    patchProp: (el: HostNode, key: string, _prev: unknown, next: unknown) => {
      el.props[key] = next
    },
    insert: (child: HostNode, parent: HostNode, anchor?: HostNode | null) => {
      detach(child)
      child.parent = parent
      const at = anchor ? parent.children.indexOf(anchor) : -1
      if (at >= 0) parent.children.splice(at, 0, child)
      else parent.children.push(child)
    },
    remove: (child: HostNode) => detach(child),
    createElement: (type: string) => make(type),
    createText: (text: string) => make(undefined, text),
    createComment: (text: string) => make(undefined, text),
    setText: (node: HostNode, text: string) => {
      node.text = text
    },
    setElementText: (node: HostNode, text: string) => {
      node.children = [make(undefined, text)]
    },
    parentNode: (child: HostNode) => child.parent,
    nextSibling: (child: HostNode) => {
      if (!child.parent) return null
      const at = child.parent.children.indexOf(child)
      return at >= 0 ? (child.parent.children[at + 1] ?? null) : null
    },
    querySelector: () => null,
    setScopeId: () => {},
    cloneNode: (child: HostNode) => child,
    insertStaticContent: () => null,
  }
}

function textOf(node: HostNode): string {
  if (node.text) return node.text
  return node.children.map(textOf).join('')
}

function collectButtons(node: HostNode, found: HostNode[] = []): HostNode[] {
  if (node.type === 'button') found.push(node)
  for (const child of node.children) collectButtons(child, found)
  return found
}

async function settle(): Promise<void> {
  const { nextTick } = vue
  for (let index = 0; index < 4; index += 1) await Promise.resolve()
  await nextTick()
  await Promise.resolve()
}

let mountedTrend: ReturnType<typeof vue.ref> | null = null

function mountToggleChart() {
  toggleOptions.length = 0
  const trend = {
    bucket_count: 96,
    bucket_seconds: 900,
    failed_observations: 0,
    from: '2026-10-15T00:00:00Z',
    keys_truncated: false,
    range: '24h',
    sampled_observations: 3,
    series: [
      series({ key_id: 'k1', key_label: 'Only key', kind: 'five_hour' }),
      series({ key_id: 'k1', key_label: 'Only key', kind: 'weekly' }),
      series({ key_id: 'k1', key_label: 'Only key', kind: 'monthly' }),
    ],
    series_truncated: false,
    to: '2026-10-16T00:00:00Z',
  }
  const trendRef = vue.ref(trend)
  mountedTrend = trendRef
  const Root = defineComponent({
    setup() {
      return () =>
        h(toggleComponent as never, {
          trend: trendRef.value,
          loading: false,
          failed: false,
          range: '24h',
          t: (key: string) => key,
        })
    },
  })
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const renderer = (vue as any).createRenderer(toggleHostOps())
  const app = renderer.createApp(Root)
  app.component('UProgress', slotStub('UProgress') as never)
  app.component('UButton', slotStub('UButton') as never)
  const root: HostNode = {
    props: {},
    children: [],
    parent: null,
  }
  app.mount(root as never)
  return root
}

function lastSeriesNames(): string[] {
  const option = toggleOptions.at(-1)
  return (option?.series ?? []).map((entry: { name: string }) => entry.name)
}

test('native legend buttons wrap, start pressed and toggle the drawn series', async () => {
  const root = mountToggleChart()
  await settle()

  let buttons = collectButtons(root)
  expect(buttons.map(textOf)).toEqual([
    'tokenPlanInterval',
    'tokenPlanMonthly',
    'tokenPlanWeekly',
  ])
  expect(buttons.every((button) => button.props['aria-pressed'] === true)).toBe(
    true,
  )
  expect(lastSeriesNames()).toEqual([
    'tokenPlanInterval',
    'tokenPlanMonthly',
    'tokenPlanWeekly',
  ])

  // Hide the weekly series with a pointer click.
  const weekly = buttons[2]
  ;(weekly.props.onClick as () => void)()
  await settle()

  buttons = collectButtons(root)
  expect(buttons[2].props['aria-pressed']).toBe(false)
  expect(lastSeriesNames()).toEqual(['tokenPlanInterval', 'tokenPlanMonthly'])
  // The remaining series keep their identity and order.
  expect(toggleOptions.at(-1)?.series[1].itemStyle.color).toBeDefined()

  // Restoring brings the same series back, pressed again.
  ;(buttons[2].props.onClick as () => void)()
  await settle()
  buttons = collectButtons(root)
  expect(buttons[2].props['aria-pressed']).toBe(true)
  expect(lastSeriesNames()).toEqual([
    'tokenPlanInterval',
    'tokenPlanMonthly',
    'tokenPlanWeekly',
  ])
})

test('hiding every legend button keeps the controls and leaves axes only', async () => {
  const root = mountToggleChart()
  await settle()

  for (let index = 0; index < 3; index += 1) {
    const buttons = collectButtons(root)
    ;(buttons[index].props.onClick as () => void)()
    await settle()
  }

  const buttons = collectButtons(root)
  expect(buttons).toHaveLength(3)
  expect(
    buttons.every((button) => button.props['aria-pressed'] === false),
  ).toBe(true)
  expect(lastSeriesNames()).toEqual([])
  // The chart still has its category axis, so the panel is not a blank card.
  expect(toggleOptions.at(-1)?.xAxis.data.length).toBeGreaterThan(0)
})

test('legend state survives a theme redraw and resets on a new selection', async () => {
  const root = mountToggleChart()
  await settle()

  let buttons = collectButtons(root)
  ;(buttons[2].props.onClick as () => void)()
  await settle()

  // A theme switch rebuilds the options but keeps the user's series choice.
  appTheme.setThemeMode('light')
  await settle()
  buttons = collectButtons(root)
  expect(buttons[2].props['aria-pressed']).toBe(false)
  expect(lastSeriesNames()).toEqual(['tokenPlanInterval', 'tokenPlanMonthly'])

  // A new trend payload is a new endpoint/key/range selection: stale hidden
  // names are dropped instead of hiding an unrelated series later.
  if (mountedTrend) {
    mountedTrend.value = { ...mountedTrend.value, sampled_observations: 9 }
  }
  await settle()
  buttons = collectButtons(root)
  expect(buttons.every((button) => button.props['aria-pressed'] === true)).toBe(
    true,
  )
  expect(lastSeriesNames()).toHaveLength(3)
  appTheme.setThemeMode('dark')
})
