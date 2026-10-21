import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import { i18n } from '../src/i18n/plugin'
import * as requestOverview from '../src/request-overview'
import type { RequestRecordOverviewBreakdownRow } from '../src/generated/admin-api'
import type { RequestRecordFormatting } from '../src/models/request-record-formatting'

// Issue #993 P1: the upstream breakdown keeps its identifier columns visible
// while the metric columns scroll behind them. The model perspective pins the
// endpoint and the model, the upstream perspective only knows the model.

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

function compileComponent(
  source: string,
  filename: string,
  resolve: (spec: string) => unknown,
): Component {
  const { descriptor } = parse(source, { filename })
  const script = compileScript(descriptor, {
    id: filename,
    inlineTemplate: true,
  })
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    rewriteSfcImports(script.content).replace(/export default/, 'return'),
  )
  return new Function('__resolve', `"use strict";\n${js}`)(resolve) as Component
}

function slotStub(name: string, slot = 'default') {
  return {
    name,
    render(this: { $slots: Record<string, (() => unknown) | undefined> }) {
      return h('div', { 'data-stub': name }, this.$slots[slot]?.())
    },
  }
}

type TableProps = {
  columns: Array<Record<string, unknown>>
  columnPinning: { left: string[]; right: string[] }
  ui: Record<string, string>
}

const popoverSource = await Bun.file(
  new URL('../src/components/BreakdownUpstreamPopover.vue', import.meta.url),
).text()

const popoverComponent: Component = compileComponent(
  popoverSource,
  'BreakdownUpstreamPopover.vue',
  (spec) => {
    if (spec === 'vue') return vue
    if (spec === '@/composables/useLocale') {
      return {
        useLocale: () => ({
          t: (key: string) => key,
          locale: vue.computed(() => 'en-US'),
        }),
      }
    }
    if (spec === '@/request-overview') return requestOverview
    return slotStub(spec.split('/').pop() ?? spec)
  },
)

function upstreamRow(): RequestRecordOverviewBreakdownRow {
  return {
    avg_output_tokens_per_second: 42,
    endpoint_id: null,
    error_count: 0,
    error_rate: 0,
    label: 'gpt-4o',
    mcp_server_id: null,
    model: 'gpt-4o',
    request_count: 12,
    request_share: 0.5,
    server_provider_kind: null,
    success_count: 12,
    success_rate: 1,
    token_share: 0.5,
    total_tokens: 1200,
    upstream_count: 2,
    model_breakdown: [
      {
        avg_output_tokens_per_second: 40,
        cache_rate: 0.5,
        error_count: 0,
        error_rate: 0,
        model: 'gpt-4o',
        request_count: 7,
        request_share: 0.3,
        token_share: 0.3,
        total_tokens: 700,
      },
      {
        avg_output_tokens_per_second: 44,
        cache_rate: 0.4,
        error_count: 0,
        error_rate: 0,
        model: 'gpt-4o-mini',
        request_count: 5,
        request_share: 0.2,
        token_share: 0.2,
        total_tokens: 500,
      },
    ],
    upstream_breakdown: [
      {
        avg_output_tokens_per_second: 40,
        cache_rate: 0.5,
        endpoint_id: 'endpoint-1',
        endpoint_name: 'openai-platform',
        error_count: 0,
        error_rate: 0,
        request_count: 7,
        request_share: 0.3,
        token_share: 0.3,
        total_tokens: 700,
        upstream_model: 'gpt-4o',
      },
      {
        avg_output_tokens_per_second: 44,
        cache_rate: 0.4,
        endpoint_id: 'endpoint-2',
        endpoint_name: 'azure-openai',
        error_count: 0,
        error_rate: 0,
        request_count: 5,
        request_share: 0.2,
        token_share: 0.2,
        total_tokens: 500,
        upstream_model: null,
      },
    ],
  } as RequestRecordOverviewBreakdownRow
}

const formatting = {
  formatCount: (value?: number | null) => String(value ?? 0),
  formatPercent: (value?: number | null) => `${value ?? 0}`,
  formatTokenQuantity: (value?: number | null) => String(value ?? 0),
  formatTokensPerSecond: (value?: number | null) => String(value ?? 0),
} as unknown as RequestRecordFormatting

async function renderPopover(
  perspective: 'model' | 'upstream',
): Promise<{ html: string; table: TableProps }> {
  let table: TableProps | undefined
  const tableStub = {
    name: 'UTable',
    inheritAttrs: false,
    props: ['data', 'columns', 'columnPinning', 'ui'],
    setup(props: Record<string, unknown>) {
      return () => {
        table = props as unknown as TableProps
        return h('div', { 'data-stub': 'UTable' })
      }
    },
  }
  const app = createSSRApp({
    render: () =>
      h(popoverComponent, {
        row: upstreamRow(),
        perspective,
        formatting,
      }),
  })
  app.use(i18n)
  app.component('UPopover', slotStub('UPopover', 'content'))
  app.component('UButton', slotStub('UButton'))
  app.component('UIcon', slotStub('UIcon'))
  app.component('UTable', tableStub)
  const html = await renderToString(app)
  if (!table) throw new Error(`the breakdown table was not rendered: ${html}`)
  return { html, table }
}

function column(
  columns: Array<Record<string, unknown>>,
  accessorKey: string,
): Record<string, unknown> {
  const found = columns.find((entry) => entry.accessorKey === accessorKey)
  if (!found) throw new Error(`column ${accessorKey} is missing`)
  return found
}

test('the model perspective pins the endpoint and the model', async () => {
  const { table } = await renderPopover('model')

  expect(table.columnPinning).toEqual({
    left: ['endpoint', 'model'],
    right: [],
  })

  const endpoint = column(table.columns, 'endpoint')
  const model = column(table.columns, 'model')
  // Issue #1004 P2: the identifier widths follow the content, so they are
  // bounded instead of fixed. The popover test pins the exact bounds.
  expect(Number(endpoint.size)).toBeGreaterThan(0)
  expect(Number(model.size)).toBeGreaterThan(0)
})

test('the upstream perspective pins only the model', async () => {
  const { table } = await renderPopover('upstream')

  expect(table.columnPinning).toEqual({ left: ['model'], right: [] })
  expect(
    table.columns.find((entry) => entry.accessorKey === 'endpoint'),
  ).toBeUndefined()
  expect(Number(column(table.columns, 'model').size)).toBeGreaterThan(0)
})

test('a pinned cell is opaque and as wide as its sticky offset', async () => {
  for (const perspective of ['model', 'upstream'] as const) {
    const { table } = await renderPopover(perspective)
    const pinned = table.columnPinning.left
    expect(pinned.length).toBeGreaterThan(0)

    let offset = 0
    for (const accessorKey of pinned) {
      const entry = column(table.columns, accessorKey)
      const style = entry.meta as {
        style: { th: Record<string, string>; td: Record<string, string> }
      }
      // The declared size and the rendered width are the same value, so the
      // sticky offset of the next pinned column stays aligned.
      expect(style.style.td.width).toBe(`${entry.size}px`)
      expect(style.style.th.width).toBe(`${entry.size}px`)
      expect(style.style.td.backgroundColor).toBe('var(--ui-bg)')
      expect(style.style.th.backgroundColor).toBe('var(--ui-bg)')
      offset += Number(entry.size)
    }

    // The metric columns keep their own width and stay transparent, so they
    // scroll behind the pinned identifiers.
    const metrics = column(table.columns, 'total_tokens')
    const metricStyle = metrics.meta as {
      style: { th: Record<string, string>; td: Record<string, string> }
    }
    expect(metricStyle.style.td.backgroundColor).toBeUndefined()
    expect(Number(metrics.size)).toBeGreaterThan(0)
    expect(offset).toBeGreaterThan(0)
  }
})

test('the popover follows the table width instead of a fixed cap', async () => {
  for (const perspective of ['model', 'upstream'] as const) {
    const { html, table } = await renderPopover(perspective)
    const width = table.columns.reduce(
      (sum, entry) => sum + Number(entry.size ?? 0),
      0,
    )
    expect(html).toContain(`--breakdown-table-width:${width}px`)
    // The popover is as wide as its table, up to the viewport minus the
    // margins, so the metric columns no longer scroll behind the pinned
    // identifiers on a desktop viewport.
    expect(html).toContain(`min(${width + 24}px, calc(100vw - 2rem))`)
    expect(html).not.toContain('38rem')
  }
})
