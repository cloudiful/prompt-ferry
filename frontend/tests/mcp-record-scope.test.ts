import { expect, test } from 'bun:test'
import { createSSRApp, h, reactive } from 'vue'
import * as vue from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'

import { usageMessages } from '../src/i18n/modules/usage'
import type {
  RequestRecordFacets,
  RequestRecordOverviewResponse,
} from '../src/generated/admin-api'
import type { RequestRecordRowView } from '../src/models'
import {
  deferred,
  fetchUsageFacets,
  fetchUsageOverview,
  fetchUsageRecords,
  loadUsageStore,
} from './usage-actions.fixtures'

const source = (relative: string) =>
  Bun.file(new URL(relative, import.meta.url)).text()

const t = (key: string) =>
  (usageMessages['zh-CN'] as Record<string, string>)[key] ?? key

type Page = {
  rows: RequestRecordRowView[]
  total: number
  first: number
  rowsPerPage: number
}

function row(id: string, scope: 'service' | 'gateway'): RequestRecordRowView {
  return {
    record_id: id,
    created_at: '2026-03-04T23:30:00Z',
    mcp_server_id: scope === 'service' ? 'srv-1' : null,
    mcp_server_name: scope === 'service' ? 'fixture-server' : null,
  } as unknown as RequestRecordRowView
}

function facets(model: string): RequestRecordFacets {
  return {
    dates: [],
    models: [model],
    request_model_options: [],
    request_client_key_options: [],
    request_redaction_options: [],
    request_state_options: [],
    request_user_options: [],
    states: [],
    users: [],
  } as unknown as RequestRecordFacets
}

type Slots = Record<string, (() => unknown) | undefined>

function slotStub(name: string) {
  return {
    name,
    render(this: { $slots: Slots }) {
      return h('div', { 'data-stub': name }, this.$slots.default?.())
    },
  }
}

/**
 * Minimal stand-in for the real `USelect`: it declares only props the
 * installed component actually has, so a test cannot pass by accepting an
 * invented one.
 */
const selectStub = {
  name: 'USelect',
  props: {
    ariaLabel: { type: String, default: undefined },
    items: { type: Array, default: () => [] },
    labelKey: { type: String, default: 'label' },
    modelValue: { type: String, default: null },
    valueKey: { type: String, default: 'value' },
  },
  render(this: {
    ariaLabel?: string
    items: Array<Record<string, unknown>>
    labelKey: string
    modelValue: string | null
  }) {
    return h(
      'div',
      {
        'data-stub': 'USelect',
        'aria-label': this.ariaLabel,
        'data-value': this.modelValue,
      },
      this.items.map((item) =>
        h(
          'span',
          { 'data-option': String(item[this.labelKey]) },
          String(item[this.labelKey]),
        ),
      ),
    )
  },
}

const panelStore = reactive({
  mcpScope: 'service',
  setMcpScope: async (next: string) => {
    panelStore.mcpScope = next
  },
})

function rewriteSfcImports(code: string): string {
  return code.replace(
    /import\s+(type\s+)?([^;'"]+?)\s+from\s+(['"])([^'"]+)\3;?/g,
    (
      _match,
      typeOnly: string | undefined,
      clause: string,
      _quote: string,
      spec: string,
    ) => {
      if (typeOnly) return ''
      const statements: string[] = []
      const named = clause.match(/\{([^}]*)\}/)
      const fallback = clause
        .replace(/\{[^}]*\}/, '')
        .replace(/^,|,$/g, '')
        .trim()
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

function loadPanel(componentSource: string) {
  const { descriptor } = parse(componentSource, {
    filename: 'RequestOverviewPanel.vue',
  })
  const script = compileScript(descriptor, {
    id: 'request-overview-panel',
    inlineTemplate: true,
  })
  const rewritten = rewriteSfcImports(script.content)
    .replace(
      /import\((['"])([^'"]+)\1\)/g,
      (_m, _q: string, spec: string) => `__resolve(${JSON.stringify(spec)})`,
    )
    .replace(/export default/, 'return')
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(rewritten)
  const vueModule = {
    ...vue,
    defineAsyncComponent: (loader: () => unknown) =>
      vue.defineAsyncComponent(() => Promise.resolve(loader()) as never),
  }
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return vueModule
    if (spec === '@/composables/useLocale') return { useLocale: () => ({ t }) }
    if (spec === '@/composables/useNotifier') {
      return { useNotifier: () => ({ notifyApiError: () => undefined }) }
    }
    if (spec === '@/stores/usage') {
      return { useRequestRecordsStore: () => panelStore }
    }
    if (spec === '../request-overview-charts') {
      return { createTrendOption: () => ({}) }
    }
    if (spec === '../request-overview') {
      return {
        breakdownDrilldownForPerspective: () => null,
        formatTrendBucketLabel: (_g: unknown, value: string) => value,
      }
    }
    if (spec === '../format/datetime') {
      return {
        formatAppDateOnly: (value: string) => String(value).slice(0, 10),
        formatAppDateTime: (value: string) => String(value),
      }
    }
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve)
}

let RequestOverviewPanel: unknown = null

async function renderPanel(options: {
  category: 'ai' | 'mcp'
  overview: RequestRecordOverviewResponse | null
}): Promise<string> {
  const componentSource = await source(
    '../src/components/RequestOverviewPanel.vue',
  )
  RequestOverviewPanel = loadPanel(componentSource)
  const app = createSSRApp({
    render: () =>
      h(RequestOverviewPanel as never, {
        category: options.category,
        formatting: {
          formatCount: (value: number) => String(value),
          formatMs: (value: number) => String(value),
          formatPercent: (value: number) => String(value),
          formatTokenQuantity: (value: number) => String(value),
          formatTokensPerSecond: (value: number) => String(value),
          formatTime: (value: string) => String(value),
        },
        loading: false,
        overview: options.overview,
        perspective: 'model',
        perspectivePending: false,
        t,
      }),
  })
  app.component('USelect', selectStub)
  app.component('UTable', slotStub('UTable'))
  app.component('UBadge', slotStub('UBadge'))
  app.component('UIcon', slotStub('UIcon'))
  return renderToString(app)
}

function overviewFixture(requestCount: number): RequestRecordOverviewResponse {
  const tokens = {
    cache_hit_rate: 0,
    cache_rate: 0,
    cache_read_tokens: 0,
    cache_write_tokens: 0,
    input_tokens: 0,
    output_tokens: 0,
    total_tokens: 0,
  }
  return {
    breakdown: [],
    summary: {
      avg_output_tokens_per_second: null,
      error_count: 0,
      method_count: 0,
      p95_first_token_ms: null,
      p95_total_ms: null,
      request_count: requestCount,
      success_count: requestCount,
      success_rate: 0,
      tokens,
    },
    trend: [],
    trend_granularity: 'hour',
  }
}

const scopeControlCount = (html: string) =>
  (html.match(/MCP 记录范围/g) ?? []).length

test('the MCP category starts on the service scope and every reader gets it', async () => {
  const store = await loadUsageStore()
  store.setRequestCategory('mcp')

  expect(store.mcpScope).toBe('service')

  await store.refreshAll()

  expect(fetchUsageRecords).toHaveBeenCalledTimes(1)
  expect(
    (fetchUsageRecords.mock.calls[0][0] as { mcpScope?: string }).mcpScope,
  ).toBe('service')
  expect((fetchUsageFacets.mock.calls[0] as unknown as string[])[3]).toBe(
    'service',
  )
  expect(
    (fetchUsageOverview.mock.calls[0][0] as { mcpScope?: string }).mcpScope,
  ).toBe('service')
})

test('entering the MCP category restores the service scope', async () => {
  const store = await loadUsageStore()
  store.setRequestCategory('mcp')
  await store.setMcpScope('all')
  expect(store.mcpScope).toBe('all')

  store.setRequestCategory('ai')
  store.setRequestCategory('mcp')

  expect(store.mcpScope).toBe('service')
})

test('a scope change resets the page and drops the incompatible service filters', async () => {
  const store = await loadUsageStore()
  store.setRequestCategory('mcp')
  store.filters.mcp_server_id!.value = 'server-1'
  store.filters.mcp_bearer_token_slot!.value = 2
  store.filters.model_key.value = 'scope-fixture-server'
  store.first = 40

  await store.setMcpScope('gateway')

  expect(store.mcpScope).toBe('gateway')
  expect(store.first).toBe(0)
  expect(store.filters.mcp_server_id!.value).toBeNull()
  expect(store.filters.mcp_bearer_token_slot!.value).toBeNull()
  expect(store.filters.model_key.value).toBeNull()
  expect(
    (fetchUsageRecords.mock.calls[0][0] as { mcpScope?: string }).mcpScope,
  ).toBe('gateway')
})

test('the row filter refresh path keeps the selected scope', async () => {
  const store = await loadUsageStore()
  store.setRequestCategory('mcp')
  await store.setMcpScope('all')

  // This is the callable path the records toolbar's filter event runs:
  // the filter is applied, then the page handler refreshes the records.
  store.filters.global.value = 'needle'
  await store.refreshRecords()

  expect(store.mcpScope).toBe('all')
  const query = fetchUsageRecords.mock.calls.at(-1)?.[0] as {
    filters: { global: { value: string | null } }
    mcpScope?: string
  }
  expect(query.mcpScope).toBe('all')
  expect(query.filters.global.value).toBe('needle')
})

test('a service list held until after the gateway response never replaces it', async () => {
  const store = await loadUsageStore()
  store.setRequestCategory('mcp')

  const servicePage = deferred<Page>()
  const gatewayPage = deferred<Page>()
  fetchUsageRecords.mockImplementationOnce(() => servicePage.promise)
  fetchUsageRecords.mockImplementationOnce(() => gatewayPage.promise)

  const staleRefresh = store.refreshRecords()
  const switchToGateway = store.setMcpScope('gateway')

  const gatewayRow = row('gateway-1', 'gateway')
  gatewayPage.resolve({
    rows: [gatewayRow],
    total: 1,
    first: 0,
    rowsPerPage: 25,
  })
  await switchToGateway
  expect(store.rows).toEqual([gatewayRow])
  expect(store.total).toBe(1)

  servicePage.resolve({
    rows: [row('service-1', 'service')],
    total: 9,
    first: 25,
    rowsPerPage: 25,
  })
  await staleRefresh

  expect(store.mcpScope).toBe('gateway')
  expect(store.rows).toEqual([gatewayRow])
  expect(store.total).toBe(1)
  expect(store.first).toBe(0)
  expect(store.recordsLoading).toBe(false)
})

test('a superseded records query reports neither its rows nor its error', async () => {
  const store = await loadUsageStore()
  store.setRequestCategory('mcp')

  const servicePage = deferred<Page>()
  const gatewayPage = deferred<Page>()
  fetchUsageRecords.mockImplementationOnce(() => servicePage.promise)
  fetchUsageRecords.mockImplementationOnce(() => gatewayPage.promise)

  const staleRefresh = store.refreshRecords()
  const switchToGateway = store.setMcpScope('gateway')

  const gatewayRow = row('gateway-1', 'gateway')
  gatewayPage.resolve({
    rows: [gatewayRow],
    total: 1,
    first: 0,
    rowsPerPage: 25,
  })
  await switchToGateway

  servicePage.reject(new Error('service query failed'))
  // The superseded caller resolves instead of reporting an error that would
  // describe a query the user already left.
  await staleRefresh

  expect(store.rows).toEqual([gatewayRow])
  expect(store.total).toBe(1)
  expect(store.recordsLoading).toBe(false)
})

test('a superseded facets query never replaces the newer scope options', async () => {
  const store = await loadUsageStore()
  store.setRequestCategory('mcp')

  const serviceFacets = deferred<RequestRecordFacets>()
  const gatewayFacets = deferred<RequestRecordFacets>()
  fetchUsageFacets.mockImplementationOnce(() => serviceFacets.promise)
  fetchUsageFacets.mockImplementationOnce(() => gatewayFacets.promise)

  const staleFacets = store.refreshFacets()
  const switchToGateway = store.setMcpScope('gateway')

  gatewayFacets.resolve(facets('gateway-model'))
  await switchToGateway
  expect(store.facets.models).toEqual(['gateway-model'])

  serviceFacets.resolve(facets('service-model'))
  await staleFacets

  expect(store.mcpScope).toBe('gateway')
  expect(store.facets.models).toEqual(['gateway-model'])
})

test('an older page refresh keeps the newer pending state', async () => {
  const store = await loadUsageStore()
  store.setRequestCategory('mcp')

  const servicePage = deferred<Page>()
  const gatewayPage = deferred<Page>()
  fetchUsageRecords.mockImplementationOnce(() => servicePage.promise)
  fetchUsageRecords.mockImplementationOnce(() => gatewayPage.promise)

  const staleRefresh = store.refreshAll()
  const switchToGateway = store.setMcpScope('gateway')
  expect(store.loading).toBe(true)

  servicePage.resolve({ rows: [], total: 0, first: 0, rowsPerPage: 25 })
  await staleRefresh
  // The newer refresh is still in flight, so its pending state must survive
  // the older query's teardown.
  expect(store.loading).toBe(true)
  expect(store.recordsLoading).toBe(true)

  const gatewayRow = row('gateway-1', 'gateway')
  gatewayPage.resolve({
    rows: [gatewayRow],
    total: 1,
    first: 0,
    rowsPerPage: 25,
  })
  await switchToGateway

  expect(store.loading).toBe(false)
  expect(store.recordsLoading).toBe(false)
  expect(store.rows).toEqual([gatewayRow])
})

test('the empty-page correction keeps its own pending state and error', async () => {
  const store = await loadUsageStore()
  store.setRequestCategory('mcp')

  fetchUsageRecords.mockImplementationOnce(async () => ({
    rows: [],
    total: 3,
    first: 25,
    rowsPerPage: 25,
  }))
  fetchUsageRecords.mockImplementationOnce(async () => ({
    rows: [row('service-1', 'service')],
    total: 3,
    first: 0,
    rowsPerPage: 25,
  }))

  await store.refreshRecords(25, 25)

  expect(store.first).toBe(0)
  expect(store.total).toBe(3)
  expect(store.rows).toHaveLength(1)
  expect(store.recordsLoading).toBe(false)
})

test('a gateway drilldown opens the gateway scope instead of every record', async () => {
  const store = await loadUsageStore()
  store.setRequestCategory('mcp')
  await store.setMcpScope('all')

  store.applyDrilldown({
    model: null,
    mcp_server_id: null,
    mcp_scope: 'gateway',
  })

  expect(store.mcpScope).toBe('gateway')
  expect(store.filters.mcp_server_id!.value).toBeNull()
  expect(store.filters.mcp_bearer_token_slot!.value).toBeNull()
  expect(store.filters.model_key.value).toBeNull()
  expect(store.first).toBe(0)
})

test('a name-only service drilldown opens the service scope by its stored name', async () => {
  const store = await loadUsageStore()
  store.setRequestCategory('mcp')
  await store.setMcpScope('all')

  store.applyDrilldown({
    model: 'scope-fixture-server',
    mcp_server_id: null,
    mcp_scope: 'service',
  })

  expect(store.mcpScope).toBe('service')
  expect(store.filters.model_key.value).toBe('scope-fixture-server')
  expect(store.filters.mcp_server_id!.value).toBeNull()
})

test('a service named like the gateway fallback keeps the service scope', async () => {
  const store = await loadUsageStore()
  store.setRequestCategory('mcp')
  await store.setMcpScope('gateway')

  store.applyDrilldown({
    model: '(unknown)',
    mcp_server_id: null,
    mcp_scope: 'service',
  })

  expect(store.mcpScope).toBe('service')
})

test('the overview and the records share one scope state', async () => {
  const store = await loadUsageStore()
  store.setRequestCategory('mcp')
  await store.setMcpScope('gateway')

  expect(store.usageWorkspaceView.mcp_scope).toBe('gateway')

  await store.refreshRecords()

  expect(
    (fetchUsageRecords.mock.calls.at(-1)?.[0] as { mcpScope?: string })
      .mcpScope,
  ).toBe('gateway')
})

test('the overview offers the MCP scope selector with no traffic and with traffic', async () => {
  // A service-default dataset can have zero service calls while gateway
  // history exists, and an empty scope must still let the user switch back.
  const empty = await renderPanel({
    category: 'mcp',
    overview: overviewFixture(0),
  })
  expect(scopeControlCount(empty)).toBe(1)
  expect(empty).toContain('aria-label="MCP 记录范围"')
  expect(empty).toContain('data-value="service"')
  for (const label of ['服务调用', '网关请求', '全部记录']) {
    expect(empty).toContain(`data-option="${label}"`)
  }

  const missing = await renderPanel({ category: 'mcp', overview: null })
  expect(scopeControlCount(missing)).toBe(1)
  expect(missing).toContain('暂无 MCP 调用记录')

  const filled = await renderPanel({
    category: 'mcp',
    overview: overviewFixture(7),
  })
  expect(scopeControlCount(filled)).toBe(1)
})

test('the AI overview has no MCP scope selector in either branch', async () => {
  const empty = await renderPanel({ category: 'ai', overview: null })
  expect(scopeControlCount(empty)).toBe(0)

  const filled = await renderPanel({
    category: 'ai',
    overview: overviewFixture(3),
  })
  expect(scopeControlCount(filled)).toBe(0)
  expect(filled).toContain('aria-label="视角"')
})

test('each view offers one scope control fed by the same store scope', async () => {
  const [toolbar, overview, panel, store] = await Promise.all([
    source('../src/components/usage/UsageRecordsToolbar.vue'),
    source('../src/components/RequestOverviewPanel.vue'),
    source('../src/components/mcp/McpUsagePanel.vue'),
    source('../src/stores/usage.ts'),
  ])

  expect(toolbar).toContain(`v-if="category === 'mcp'"`)
  expect(toolbar).toContain(':items="scopeOptions"')
  expect(overview).toContain(`v-if="category === 'mcp'"`)
  expect(overview).toContain(':items="scopeOptions"')
  expect(overview).toContain('requestRecordsStore.setMcpScope')
  expect(panel).toContain(':mcp-scope="workspace.mcp_scope"')
  expect(panel).toContain('requestRecordsStore.setMcpScope')
  expect(store).toContain('mcpScope: queryState.mcpScope.value')
})
