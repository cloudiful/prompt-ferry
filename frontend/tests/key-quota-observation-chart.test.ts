import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, defineComponent, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import type {
  QuotaKeyObservationTrendPoint,
  QuotaKeyObservationTrendResponse,
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
const subscriptionQuota =
  await import('../src/composables/subscriptionQuotaWindows')
const appTheme = await import('../src/theme/appTheme')

const componentSource = await Bun.file(
  new URL(
    '../src/components/endpoints/KeyQuotaObservationChart.vue',
    import.meta.url,
  ),
).text()

function slotStub(name: string) {
  return defineComponent({
    name,
    setup(_props, { slots }) {
      return () => h('div', { 'data-stub': name }, slots.default?.())
    },
  })
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyOption = Record<string, any>

const renderedOptions: AnyOption[] = []
const usageChartStub = defineComponent({
  name: 'UsageChart',
  props: { option: { type: Object, default: null } },
  setup(props) {
    return () => {
      if (props.option) renderedOptions.push(props.option as AnyOption)
      return h('div', { 'data-stub': 'UsageChart' })
    }
  },
})

// The real component lazily imports the ECharts wrapper; the harness replaces
// only that dynamic import with a stub that still receives the built option.
const instrumentedVue = {
  ...vue,
  defineAsyncComponent: () => usageChartStub,
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

const component = (() => {
  const { descriptor } = parse(componentSource, {
    filename: 'KeyQuotaObservationChart.vue',
  })
  const script = compileScript(descriptor, {
    id: 'key-quota-observation-chart',
    inlineTemplate: true,
  })
  const source = rewriteSfcImports(script.content).replace(
    /export default/,
    'return',
  )
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(source)
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return instrumentedVue
    if (spec === '@/quota-observation-charts') return charts
    if (spec === '@/composables/subscriptionQuotaWindows') {
      return subscriptionQuota
    }
    if (spec === '@/theme/appTheme') return appTheme
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve)
})()

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
    source_id: 'deepseek:CNY',
    kind: 'balance',
    unit: 'CNY',
    metric: 'amount',
    points: [
      point('2026-10-16T00:00:00Z', 5),
      point('2026-10-16T00:15:00Z', null),
    ],
    ...overrides,
  }
}

function trend(
  overrides: Partial<QuotaKeyObservationTrendResponse> = {},
): QuotaKeyObservationTrendResponse {
  return {
    bucket_count: 96,
    bucket_seconds: 900,
    failed_observations: 0,
    from: '2026-10-15T00:00:00Z',
    keys_truncated: false,
    range: '24h',
    sampled_observations: 3,
    series: [series()],
    series_truncated: false,
    to: '2026-10-16T00:00:00Z',
    ...overrides,
  }
}

async function render(props: {
  trend?: QuotaKeyObservationTrendResponse | null
  loading?: boolean
  failed?: boolean
}): Promise<string> {
  renderedOptions.length = 0
  const app = createSSRApp({
    render: () =>
      h(component as never, {
        trend: props.trend ?? null,
        loading: props.loading ?? false,
        failed: props.failed ?? false,
        range: '24h',
        t: (key: string) => key,
      }),
  })
  app.component('UProgress', slotStub('UProgress') as never)
  app.component('UButton', slotStub('UButton') as never)
  return renderToString(app)
}

test('loading shows the trend placeholder, never a chart', async () => {
  const html = await render({ loading: true, trend: null })
  expect(html).toContain('quotaTrendLoading')
  expect(html).toContain('quotaTrendTitle')
  expect(html).not.toContain('data-stub="UsageChart"')
})

test('a failure offers retry and never renders a chart', async () => {
  const html = await render({ failed: true, trend: null })
  expect(html).toContain('quotaTrendError')
  expect(html).toContain('quotaTrendRetry')
  expect(html).not.toContain('data-stub="UsageChart"')
})

test('an all-null or empty trend shows the empty state instead of a flat line', async () => {
  const empty = await render({ trend: trend({ series: [] }) })
  expect(empty).toContain('quotaTrendEmpty')
  expect(empty).not.toContain('data-stub="UsageChart"')

  const allNull = await render({
    trend: trend({
      series: [series({ points: [point('2026-10-16T00:00:00Z', null)] })],
    }),
  })
  expect(allNull).toContain('quotaTrendEmpty')
  expect(allNull).not.toContain('data-stub="UsageChart"')
})

test('a plottable trend renders the chart with the themed option', async () => {
  const html = await render({ trend: trend() })
  expect(html).toContain('data-stub="UsageChart"')
  expect(html).toContain('quotaTrendTitle')
  expect(html).toContain('quotaTrendBucketHint')
  expect(html).toContain('quotaTrendUnitHint')
  expect(renderedOptions).toHaveLength(1)
})

test('each (metric, unit) group renders its own single-axis scroll-legend panel', async () => {
  const html = await render({
    trend: trend({
      series: [
        series({
          key_id: 'k1',
          source_id: 'deepseek:CNY',
          unit: 'CNY',
          metric: 'amount',
        }),
        series({
          key_id: 'k1',
          source_id: 'deepseek:USD',
          unit: 'USD',
          metric: 'amount',
        }),
        series({
          key_id: 'k1',
          source_id: 'command_code:balances',
          unit: 'credits',
          metric: 'amount',
        }),
        series({
          key_id: 'k2',
          source_id: 'glm:five_hour',
          kind: 'five_hour',
          unit: 'percent',
          metric: 'remaining_percent',
        }),
      ],
    }),
  })

  // Four panels, four charts, headings rendered for each unit.
  expect((html.match(/data-stub="UsageChart"/g) ?? []).length).toBe(4)
  expect(renderedOptions).toHaveLength(4)
  for (const option of renderedOptions) {
    expect(option.yAxis.type).toBe('value')
    expect(option.legend.type).toBe('scroll')
  }
  // Units and the percent metric each get a distinct axis; none is merged.
  expect(renderedOptions.map((option) => option.yAxis.name)).toEqual([
    'CNY',
    'USD',
    'credits',
    '%',
  ])
  expect(html).toContain('>CNY<')
  expect(html).toContain('>USD<')
  expect(html).toContain('>credits<')
  expect(html).toContain('>%<')
})

test('a panel heading escapes an untrusted unit through Vue interpolation', async () => {
  const html = await render({
    trend: trend({
      series: [
        series({
          unit: '<img src=x onerror=alert(1)>',
          metric: 'amount',
        }),
      ],
    }),
  })
  expect(html).not.toContain('<img')
  expect(html).toContain('&lt;img src=x onerror=alert(1)&gt;')
})

test('key and series truncation, failures and sampling are surfaced honestly', async () => {
  const html = await render({
    trend: trend({
      failed_observations: 2,
      keys_truncated: true,
      series_truncated: true,
      sampled_observations: 7,
    }),
  })
  expect(html).toContain('quotaTrendFailed')
  expect(html).toContain('quotaTrendKeysTruncated')
  expect(html).toContain('quotaTrendSeriesTruncated')
  expect(html).toContain('quotaTrendSampled')
})

test('no failure or truncation indicator appears when everything is complete', async () => {
  const html = await render({ trend: trend() })
  expect(html).not.toContain('quotaTrendFailed')
  expect(html).not.toContain('quotaTrendKeysTruncated')
  expect(html).not.toContain('quotaTrendSeriesTruncated')
})
