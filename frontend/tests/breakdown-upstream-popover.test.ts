import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import { i18n } from '../src/i18n/plugin'
import * as requestOverview from '../src/request-overview'
import type { RequestRecordOverviewBreakdownRow } from '@/generated/admin-api'
import type { RequestRecordFormatting } from '../src/models/request-record-formatting'

// Issue #1004 P2: the upstream breakdown uses the available space. The
// popover follows its table instead of a fixed cap, the identifier columns
// follow their content, a narrow viewport keeps one identifier pinned so the
// metrics stay readable, and a touch device can open the popover at all.

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

// The component reads the real locale messages, so the header widths are the
// ones an operator sees.
const popoverComponent: Component = compileComponent(
  popoverSource,
  'BreakdownUpstreamPopover.vue',
  (spec) => {
    if (spec === 'vue') return vue
    if (spec === '@/composables/useLocale') {
      return {
        useLocale: () => ({
          t: (key: string) => i18n.global.t(key),
          locale: vue.computed(() => 'en-US'),
        }),
      }
    }
    if (spec === '@/request-overview') return requestOverview
    return slotStub(spec.split('/').pop() ?? spec)
  },
)

function upstreamRow(identifiers: {
  endpoint: string
  model: string
}): RequestRecordOverviewBreakdownRow {
  return {
    avg_output_tokens_per_second: 42,
    endpoint_id: null,
    error_count: 0,
    error_rate: 0,
    label: identifiers.model,
    mcp_server_id: null,
    model: identifiers.model,
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
        endpoint_id: null,
        endpoint_name: null,
        error_count: 0,
        error_rate: 0,
        model: identifiers.model,
        request_count: 7,
        request_share: 0.3,
        token_share: 0.3,
        total_tokens: 700,
      },
      {
        avg_output_tokens_per_second: 44,
        cache_rate: 0.4,
        endpoint_id: null,
        endpoint_name: null,
        error_count: 0,
        error_rate: 0,
        model: `${identifiers.model}-mini`,
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
        endpoint_name: identifiers.endpoint,
        error_count: 0,
        error_rate: 0,
        request_count: 7,
        request_share: 0.3,
        token_share: 0.3,
        total_tokens: 700,
        upstream_model: identifiers.model,
      },
      {
        avg_output_tokens_per_second: 44,
        cache_rate: 0.4,
        endpoint_id: 'endpoint-2',
        endpoint_name: `${identifiers.endpoint}-2`,
        error_count: 0,
        error_rate: 0,
        request_count: 5,
        request_share: 0.2,
        token_share: 0.2,
        total_tokens: 500,
        upstream_model: `${identifiers.model}-mini`,
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
  identifiers: { endpoint: string; model: string } = {
    endpoint: 'openai-platform',
    model: 'gpt-4o-mini',
  },
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
        row: upstreamRow(identifiers),
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

function withViewport(
  narrow: boolean,
  run: () => Promise<void>,
): Promise<void> {
  const original = globalThis.matchMedia
  globalThis.matchMedia = ((query: string) => ({
    matches: narrow,
    media: query,
    onchange: null,
    addEventListener: () => {},
    removeEventListener: () => {},
    addListener: () => {},
    removeListener: () => {},
    dispatchEvent: () => false,
  })) as unknown as typeof globalThis.matchMedia
  return run().finally(() => {
    globalThis.matchMedia = original
  })
}

function columnSize(table: TableProps, accessorKey: string): number {
  const found = table.columns.find((entry) => entry.accessorKey === accessorKey)
  if (!found) throw new Error(`column ${accessorKey} is missing`)
  return Number(found.size)
}

function tableWidth(table: TableProps): number {
  return table.columns.reduce((sum, entry) => sum + Number(entry.size ?? 0), 0)
}

test('the popover is as wide as its table and never past the viewport', async () => {
  const { html, table } = await renderPopover('model')
  const width = tableWidth(table)
  expect(html).toContain(`--breakdown-table-width:${width}px`)
  expect(html).toContain(`min(${width + 24}px, calc(100vw - 2rem))`)
  expect(html).not.toContain('38rem')
})

test('the compact columns keep every metric', async () => {
  const { table } = await renderPopover('model')
  const accessors = table.columns.map((entry) => entry.accessorKey)
  expect(accessors).toEqual([
    'endpoint',
    'model',
    'request_count',
    'request_share',
    'total_tokens',
    'token_share',
    'cache_rate',
    'error_rate',
    'avg_output_tokens_per_second',
  ])
  for (const entry of table.columns) {
    expect(Number(entry.size)).toBeGreaterThan(0)
  }
})

test('the declared widths, the rendered widths and the table sum agree', async () => {
  for (const perspective of ['model', 'upstream'] as const) {
    const { html, table } = await renderPopover(perspective)
    for (const entry of table.columns) {
      const style = entry.meta as {
        style: { th: Record<string, string>; td: Record<string, string> }
      }
      const size = `${entry.size}px`
      expect(style.style.td.width).toBe(size)
      expect(style.style.td.minWidth).toBe(size)
      expect(style.style.td.maxWidth).toBe(size)
      expect(style.style.th.width).toBe(size)
      expect(style.style.th.minWidth).toBe(size)
      expect(style.style.th.maxWidth).toBe(size)
    }
    expect(html).toContain(`--breakdown-table-width:${tableWidth(table)}px`)
  }
})

test('a desktop table with ordinary identifiers stays around 860 pixels', async () => {
  const { table } = await renderPopover('model')
  const width = tableWidth(table)
  expect(width).toBeGreaterThan(830)
  expect(width).toBeLessThan(890)
})

test('the table keeps its width in both locales', async () => {
  const original = i18n.global.locale.value
  try {
    i18n.global.locale.value = 'en-US'
    const english = await renderPopover('model')
    i18n.global.locale.value = 'zh-CN'
    const chinese = await renderPopover('model')
    expect(tableWidth(english.table)).toBe(tableWidth(chinese.table))
    expect(tableWidth(english.table)).toBeGreaterThan(830)
    expect(tableWidth(english.table)).toBeLessThan(890)
  } finally {
    i18n.global.locale.value = original
  }
})

test('a short identifier shrinks its column', async () => {
  const short = await renderPopover('model', {
    endpoint: 'a',
    model: 'b',
  })
  const long = await renderPopover('model', {
    endpoint: 'openai-platform-enterprise-eu',
    model: 'gpt-4o-mini-preview-2026',
  })
  expect(columnSize(short.table, 'model')).toBeLessThan(
    columnSize(long.table, 'model'),
  )
  expect(tableWidth(short.table)).toBeLessThan(tableWidth(long.table))
})

test('a long identifier stays bounded', async () => {
  const long = await renderPopover('model', {
    endpoint: 'openai-platform-enterprise-eu-west-production-cluster',
    model: 'gpt-4o-mini-preview-2026-06-01',
  })
  const longer = await renderPopover('model', {
    endpoint:
      'openai-platform-enterprise-eu-west-production-cluster-with-a-very-long-suffix',
    model: 'gpt-4o-mini-preview-2026-06-01-with-a-very-long-suffix',
  })
  expect(columnSize(longer.table, 'endpoint')).toBe(
    columnSize(long.table, 'endpoint'),
  )
  expect(columnSize(longer.table, 'model')).toBe(
    columnSize(long.table, 'model'),
  )
  expect(tableWidth(longer.table)).toBe(tableWidth(long.table))
})

test('a narrow viewport keeps one identifier pinned', async () => {
  await withViewport(true, async () => {
    const { table } = await renderPopover('model')
    expect(table.columnPinning.left).toEqual(['model'])
  })
  await withViewport(false, async () => {
    const { table } = await renderPopover('model')
    expect(table.columnPinning.left).toEqual(['endpoint', 'model'])
  })
})

test('a touch device can open the popover', () => {
  expect(popoverSource).toContain(':enable-touch="true"')
})

test('a long identifier keeps its full value in the title', () => {
  expect(popoverSource).toContain(':title="row.original.endpoint"')
  expect(popoverSource).toContain(':title="row.original.model"')
})
