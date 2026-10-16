import { afterEach, beforeEach, expect, mock, test } from 'bun:test'
import { nextTick, ref } from 'vue'
import * as vue from 'vue'
import { createSSRApp, defineComponent, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import type {
  QuotaKeyObservationHistoryItem,
  QuotaKeyObservationHistoryResponse,
} from '../src/generated/admin-api'

const fetchPage = mock(
  async (
    _endpointId: string,
    _keyId: string | null,
    _limit?: number,
    _beforeId?: string,
  ): Promise<QuotaKeyObservationHistoryResponse> => ({
    items: [],
    next_cursor: null,
    retention_days: 30,
  }),
)

const { useKeyQuotaObservationHistory } =
  await import('../src/composables/useKeyQuotaObservationHistory')

function item(
  observation_id: string,
  key_id = 'key-a',
): QuotaKeyObservationHistoryItem {
  return {
    observation_id,
    key_id,
    key_label: `label-${key_id}`,
    provider: 'minimax',
    observed_at: '2026-10-10T12:00:00Z',
    ok: true,
    status: '200',
    error_code: null,
    plan_type: null,
    limit_reached: false,
    windows: [],
    source: 'request',
  }
}

function page(
  ids: string[],
  nextCursor: string | null = null,
  keyId = 'key-a',
): QuotaKeyObservationHistoryResponse {
  return {
    items: ids.map((id) => item(id, keyId)),
    next_cursor: nextCursor,
    retention_days: 30,
  }
}

beforeEach(() => {
  fetchPage.mockReset()
  fetchPage.mockImplementation(async () => page([]))
})

afterEach(() => {
  fetchPage.mockReset()
})

test('key pages use string cursors and de-duplicate observation IDs', async () => {
  fetchPage
    .mockResolvedValueOnce(page(['90', '80'], '80'))
    .mockResolvedValueOnce(page(['80', '70']))
  const history = useKeyQuotaObservationHistory(
    ref('endpoint-a'),
    ref<string | null>(null),
    fetchPage,
  )

  await history.loadFirstPage()
  await history.loadMore()

  expect(fetchPage.mock.calls).toEqual([
    ['endpoint-a', null, 50],
    ['endpoint-a', null, 50, '80'],
  ])
  expect(history.items.value.map((row) => row.observation_id)).toEqual([
    '90',
    '80',
    '70',
  ])
  expect(history.nextCursor.value).toBeNull()
})

test('key filter scopes reads and reloads when the key changes', async () => {
  fetchPage.mockImplementation(async (_endpoint, keyId) =>
    keyId === 'key-b' ? page(['7'], null, 'key-b') : page(['9', '8']),
  )
  const keyId = ref<string | null>(null)
  const expanded = ref(true)
  const history = useKeyQuotaObservationHistory(
    ref('endpoint-a'),
    keyId,
    fetchPage,
    expanded,
  )

  await history.loadFirstPage()
  expect(history.items.value.map((row) => row.observation_id)).toEqual([
    '9',
    '8',
  ])

  keyId.value = 'key-b'
  await nextTick()
  await Promise.resolve()
  await Promise.resolve()
  expect(fetchPage.mock.calls.at(-1)).toEqual(['endpoint-a', 'key-b', 50])
  expect(history.items.value.map((row) => row.observation_id)).toEqual(['7'])
  expect(history.items.value[0].key_id).toBe('key-b')
})

test('history is lazy with zero initial fetch and refreshes on expand', async () => {
  fetchPage.mockImplementation(async () => page(['50']))
  const expanded = ref(false)
  const history = useKeyQuotaObservationHistory(
    ref('endpoint-a'),
    ref<string | null>(null),
    fetchPage,
    expanded,
  )

  await nextTick()
  expect(fetchPage).not.toHaveBeenCalled()
  expect(history.loaded.value).toBe(false)

  expanded.value = true
  await nextTick()
  await Promise.resolve()
  await Promise.resolve()
  expect(fetchPage).toHaveBeenCalledTimes(1)

  expanded.value = false
  await nextTick()
  expect(history.items.value).toEqual([])
  expect(history.loaded.value).toBe(false)
})

test('history failures remain distinct from empty and can be retried', async () => {
  fetchPage
    .mockRejectedValueOnce(new Error('unavailable'))
    .mockResolvedValueOnce(page(['10']))
  const history = useKeyQuotaObservationHistory(
    ref('endpoint-a'),
    ref<string | null>(null),
    fetchPage,
  )

  await history.loadFirstPage()
  expect(history.failed.value).toBe(true)
  expect(history.loaded.value).toBe(false)
  expect(history.items.value).toEqual([])

  await history.retry()
  expect(history.failed.value).toBe(false)
  expect(history.loaded.value).toBe(true)
  expect(history.items.value.map((row) => row.observation_id)).toEqual(['10'])
})

test('late responses from an endpoint and key switch cannot replace the selection', async () => {
  let resolveFirst!: (value: QuotaKeyObservationHistoryResponse) => void
  fetchPage
    .mockImplementationOnce(
      () => new Promise((resolve) => (resolveFirst = resolve)),
    )
    .mockResolvedValueOnce(page(['endpoint-b-snapshot'], null, 'key-b'))
  const endpointId = ref('endpoint-a')
  const keyId = ref<string | null>('key-a')
  const history = useKeyQuotaObservationHistory(endpointId, keyId, fetchPage)

  const firstRequest = history.loadFirstPage()
  endpointId.value = 'endpoint-b'
  keyId.value = 'key-b'
  await history.loadFirstPage()
  resolveFirst(page(['endpoint-a-snapshot'], null, 'key-a'))
  await firstRequest

  expect(history.items.value.map((row) => row.observation_id)).toEqual([
    'endpoint-b-snapshot',
  ])
  expect(history.retentionDays.value).toBe(30)
})

// --- SSR integration: the history component defaults to the chart ---
// The repo has no component-interaction test setup, so this harness compiles
// the SFC and stubs its children/composables; it can prove the default view
// and the lazy list, while the mode-switch state machine itself is covered by
// the trend composable's activation tests.

const canonicalQuota =
  await import('../src/composables/useCanonicalQuotaWindows')
const trendModule =
  await import('../src/composables/useKeyQuotaObservationTrend')

const historyComponentSource = await Bun.file(
  new URL(
    '../src/components/endpoints/KeyQuotaObservationHistory.vue',
    import.meta.url,
  ),
).text()

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

const loadFirstPage = mock(async () => {})
const loadMore = mock(async () => {})
const clearHistory = mock(() => {})
const chartStub = defineComponent({
  name: 'KeyQuotaObservationChart',
  setup() {
    return () => h('div', { 'data-stub': 'KeyQuotaObservationChart' })
  },
})
const stub = (name: string) =>
  defineComponent({
    name,
    setup(_props, { slots }) {
      return () => h('div', { 'data-stub': name }, slots.default?.())
    },
  })
const collapsibleStub = defineComponent({
  name: 'UCollapsible',
  setup(_props, { slots }) {
    return () =>
      h('div', [
        slots.default?.({ open: true }),
        slots.content?.({ close: () => {} }),
      ])
  },
})

const instrumentedVue = {
  ...vue,
  defineAsyncComponent: () => chartStub,
}

const historyModuleStub = {
  useKeyQuotaObservationHistory: () => ({
    clear: clearHistory,
    failed: ref(false),
    items: ref<QuotaKeyObservationHistoryItem[]>([]),
    loaded: ref(false),
    loading: ref(false),
    loadingMore: ref(false),
    loadFirstPage,
    loadMore,
    nextCursor: ref<string | null>(null),
    retentionDays: ref(30),
    retry: mock(async () => {}),
  }),
}
const trendModuleStub = {
  KEY_QUOTA_TREND_RANGES: trendModule.KEY_QUOTA_TREND_RANGES,
  useKeyQuotaObservationTrend: () => ({
    clear: mock(() => {}),
    failed: ref(false),
    load: mock(async () => {}),
    loaded: ref(false),
    loading: ref(false),
    reload: mock(() => {}),
    retry: mock(async () => {}),
    trend: ref(null),
  }),
}
const apiStub = {
  fetchKeyQuotaObservationHistory: mock(async () => ({
    items: [],
    next_cursor: null,
    retention_days: 30,
  })),
  fetchKeyQuotaObservationTrend: mock(async () => ({})),
}

const historyComponent = (() => {
  const { descriptor } = parse(historyComponentSource, {
    filename: 'KeyQuotaObservationHistory.vue',
  })
  const script = compileScript(descriptor, {
    id: 'key-quota-observation-history',
    inlineTemplate: true,
  })
  const source = rewriteSfcImports(script.content).replace(
    /export default/,
    'return',
  )
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(source)
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return instrumentedVue
    if (spec === '@/components/endpoints/TokenPlanQuotaWindowRow.vue') {
      return stub('TokenPlanQuotaWindowRow')
    }
    if (spec === '@/composables/useCanonicalQuotaWindows') {
      return canonicalQuota
    }
    if (spec === '@/composables/useKeyQuotaObservationTrend') {
      return trendModuleStub
    }
    if (spec === '@/composables/useKeyQuotaObservationHistory') {
      return historyModuleStub
    }
    if (spec === '@/stores/endpoints-api') return apiStub
    return stub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve)
})()

async function renderHistory(): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(historyComponent as never, {
        endpointId: 'endpoint-a',
        t: (key: string) => key,
      }),
  })
  app.component('UCollapsible', collapsibleStub as never)
  app.component('UButton', stub('UButton') as never)
  app.component('UBadge', stub('UBadge') as never)
  app.component('UProgress', stub('UProgress') as never)
  app.component('USelect', stub('USelect') as never)
  return renderToString(app)
}

test('the history defaults to the chart and never preloads a keyset page', async () => {
  loadFirstPage.mockClear()
  const html = await renderHistory()
  expect(html).toContain('data-stub="KeyQuotaObservationChart"')
  expect(html).toContain('quotaObservationModeChart')
  expect(html).toContain('quotaObservationModeList')
  expect(html).toContain('quotaObservationModeLabel')
  // Chart view: the list branch and its keyset fetcher stay untouched.
  expect(loadFirstPage).not.toHaveBeenCalled()
  expect(html).not.toContain('quotaHistoryLoadMore')
})

test('the shared key selector and the chart range picker both render', async () => {
  const html = await renderHistory()
  // Key selector plus the 24h/7d/30d range picker, both design-system selects.
  expect((html.match(/data-stub="USelect"/g) ?? []).length).toBe(2)
  expect(html).toContain('modelvalue="24h"')
})

// --- Reactive interaction harness (issue #911 P3) ---
// The repo has no DOM test setup, so the SFC is compiled and mounted through
// Vue's custom renderer with capturing child stubs. This exercises the real
// request-owning composables, so it can prove the atomic endpoint reset and
// the mode-switch paging behaviour instead of only static SSR source.

const realHistoryModule =
  await import('../src/composables/useKeyQuotaObservationHistory')
const subscriptionQuota =
  await import('../src/composables/subscriptionQuotaWindows')

const apiHolder = {
  history: mock(async () => ({
    items: [],
    next_cursor: null,
    retention_days: 30,
  })),
  trend: mock(async () => ({
    bucket_count: 96,
    bucket_seconds: 900,
    failed_observations: 0,
    from: '2026-10-15T00:00:00Z',
    keys_truncated: false,
    range: '24h',
    sampled_observations: 0,
    series: [],
    series_truncated: false,
    to: '2026-10-16T00:00:00Z',
  })),
}
const apiResolver = {
  fetchKeyQuotaObservationHistory: (...args: unknown[]) =>
    apiHolder.history(...args),
  fetchKeyQuotaObservationTrend: (...args: unknown[]) =>
    apiHolder.trend(...args),
}

const chartStubForInteraction = defineComponent({
  name: 'KeyQuotaObservationChart',
  setup() {
    return () => h('div', { 'data-stub': 'KeyQuotaObservationChart' })
  },
})
const interactionVue = {
  ...vue,
  defineAsyncComponent: () => chartStubForInteraction,
}

type CapturedButton = {
  label: string
  pressed: unknown
  onClick: (() => void) | undefined
}
type CapturedSelect = {
  ariaLabel: unknown
  modelValue: unknown
  onUpdate: ((value: unknown) => void) | undefined
}
const capture: {
  buttons: CapturedButton[]
  selects: CapturedSelect[]
  open: ((value: boolean) => void) | undefined
} = { buttons: [], selects: [], open: undefined }

// The stub has no DOM, so the label is read from the default slot the compiled
// template passed; that is how a mode button is addressed without relying on
// render order.
function slotText(slots: { default?: () => unknown }): string {
  const nodes = slots.default?.() ?? []
  const list = Array.isArray(nodes) ? nodes : [nodes]
  return list
    .map((entry) => {
      if (typeof entry === 'string' || typeof entry === 'number') {
        return String(entry)
      }
      if (entry && typeof entry === 'object') {
        const children = (entry as { children?: unknown }).children
        if (typeof children === 'string') return children
        if (Array.isArray(children)) {
          return children
            .map((child) => (typeof child === 'string' ? child : ''))
            .join('')
        }
      }
      return ''
    })
    .join('')
}

const uButtonStub = defineComponent({
  name: 'UButton',
  inheritAttrs: false,
  setup(_props, { attrs, slots }) {
    return () => {
      capture.buttons.push({
        label: slotText(slots),
        pressed: attrs['aria-pressed'],
        onClick: attrs.onClick as (() => void) | undefined,
      })
      return h('button', {}, slots.default?.())
    }
  },
})
const uSelectStub = defineComponent({
  name: 'USelect',
  inheritAttrs: false,
  setup(_props, { attrs }) {
    return () => {
      capture.selects.push({
        ariaLabel: attrs['aria-label'],
        modelValue: attrs.modelValue,
        onUpdate: attrs['onUpdate:modelValue'] as
          ((value: unknown) => void) | undefined,
      })
      return h('div', { 'data-stub': 'USelect' })
    }
  },
})
const uCollapsibleStub = defineComponent({
  name: 'UCollapsible',
  inheritAttrs: false,
  setup(_props, { attrs, slots }) {
    return () => {
      capture.open = attrs['onUpdate:open'] as
        ((value: boolean) => void) | undefined
      const open = Boolean(attrs.open)
      return h('div', [
        slots.default?.({ open }),
        open ? slots.content?.({ close: () => {} }) : null,
      ])
    }
  },
})

type HostNode = {
  type?: string
  text?: string
  children: HostNode[]
  parent: HostNode | null
}

function hostNodeOps() {
  const node = (type?: string, text?: string): HostNode => ({
    type,
    text,
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
    patchProp: () => {},
    insert: (child: HostNode, parent: HostNode, anchor?: HostNode | null) => {
      detach(child)
      child.parent = parent
      const at = anchor ? parent.children.indexOf(anchor) : -1
      if (at >= 0) parent.children.splice(at, 0, child)
      else parent.children.push(child)
    },
    remove: (child: HostNode) => detach(child),
    createElement: (type: string) => node(type),
    createText: (text: string) => node(undefined, text),
    createComment: (text: string) => node(undefined, text),
    setText: () => {},
    setElementText: () => {},
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

const interactionComponent = (() => {
  const { descriptor } = parse(historyComponentSource, {
    filename: 'KeyQuotaObservationHistory.vue',
  })
  const script = compileScript(descriptor, {
    id: 'key-quota-observation-history-interaction',
    inlineTemplate: true,
  })
  const source = rewriteSfcImports(script.content).replace(
    /export default/,
    'return',
  )
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(source)
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return interactionVue
    if (spec === '@/components/endpoints/TokenPlanQuotaWindowRow.vue') {
      return stub('TokenPlanQuotaWindowRow')
    }
    if (spec === '@/composables/useCanonicalQuotaWindows') {
      return canonicalQuota
    }
    if (spec === '@/composables/useKeyQuotaObservationTrend') {
      return trendModule
    }
    if (spec === '@/composables/useKeyQuotaObservationHistory') {
      return realHistoryModule
    }
    if (spec === '@/composables/subscriptionQuotaWindows') {
      return subscriptionQuota
    }
    if (spec === '@/stores/endpoints-api') return apiResolver
    return stub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve)
})()

function mountHistory(endpointId: ReturnType<typeof ref<string>>) {
  const Root = defineComponent({
    setup() {
      return () =>
        h(interactionComponent as never, {
          endpointId: endpointId.value,
          t: (key: string) => key,
        })
    },
  })
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const renderer = (vue as any).createRenderer(hostNodeOps())
  const app = renderer.createApp(Root)
  app.component('UCollapsible', uCollapsibleStub as never)
  app.component('UButton', uButtonStub as never)
  app.component('UBadge', stub('UBadge') as never)
  app.component('UProgress', stub('UProgress') as never)
  app.component('USelect', uSelectStub as never)
  app.mount({ children: [], parent: null } as never)
  return app
}

async function settle(): Promise<void> {
  for (let index = 0; index < 6; index += 1) await Promise.resolve()
  await nextTick()
  await Promise.resolve()
}

function keySelect() {
  return [...capture.selects]
    .reverse()
    .find((entry) => entry.ariaLabel === 'quotaObservationKeyLabel')
}

function modeButton(label: string) {
  return capture.buttons.find((entry) => entry.label === label)
}

const CHART_BUTTON = 'quotaObservationModeChart'
const LIST_BUTTON = 'quotaObservationModeList'

test('changing only the key keeps normal filtering', async () => {
  apiHolder.trend.mockClear()
  capture.buttons.length = 0
  capture.selects.length = 0
  const endpointId = ref('endpoint-a')
  mountHistory(endpointId)
  await settle()

  capture.open?.(true)
  await settle()
  expect(apiHolder.trend.mock.calls[0]).toEqual(['endpoint-a', null, '24h'])

  keySelect()?.onUpdate?.('key-a')
  await settle()
  expect(apiHolder.trend.mock.calls.at(-1)).toEqual([
    'endpoint-a',
    'key-a',
    '24h',
  ])
})

test('changing the endpoint resets the key and never dispatches the old key', async () => {
  apiHolder.trend.mockClear()
  capture.buttons.length = 0
  capture.selects.length = 0
  const endpointId = ref('endpoint-a')
  mountHistory(endpointId)
  await settle()

  capture.open?.(true)
  await settle()
  keySelect()?.onUpdate?.('key-a')
  await settle()
  expect(apiHolder.trend.mock.calls.at(-1)).toEqual([
    'endpoint-a',
    'key-a',
    '24h',
  ])

  capture.selects.length = 0
  endpointId.value = 'endpoint-b'
  await settle()

  // The very next request already carries the new endpoint with a null key.
  expect(apiHolder.trend.mock.calls.at(-1)).toEqual(['endpoint-b', null, '24h'])
  // No request ever pairs the new endpoint with the previous endpoint's key.
  expect(
    apiHolder.trend.mock.calls.some(
      ([endpoint, key]) => endpoint === 'endpoint-b' && key === 'key-a',
    ),
  ).toBe(false)
  // The selector itself is reset, so it can never hold an unselectable id.
  expect(keySelect()?.modelValue ?? null).toBeNull()
})

test('switching to the list preserves loaded pages across a chart round-trip', async () => {
  apiHolder.history.mockReset()
  apiHolder.history
    .mockResolvedValueOnce({
      items: [item('90'), item('80')],
      next_cursor: '80',
      retention_days: 30,
    })
    .mockResolvedValueOnce({
      items: [item('70')],
      next_cursor: null,
      retention_days: 30,
    })
  capture.buttons.length = 0
  capture.selects.length = 0
  const endpointId = ref('endpoint-a')
  mountHistory(endpointId)
  await settle()

  capture.open?.(true)
  await settle()
  // Chart is the default: the keyset list is never fetched before activation.
  expect(apiHolder.history).not.toHaveBeenCalled()

  modeButton(LIST_BUTTON)?.onClick?.()
  await settle()
  expect(apiHolder.history.mock.calls).toEqual([['endpoint-a', null, 50]])

  // A real "load more" asks the backend for the next DB cursor page.
  modeButton('quotaHistoryLoadMore')?.onClick?.()
  await settle()
  expect(apiHolder.history.mock.calls).toEqual([
    ['endpoint-a', null, 50],
    ['endpoint-a', null, 50, '80'],
  ])

  // Back to chart, then list again: the already loaded list must not refetch,
  // which would also drop the cursor and duplicate pages.
  modeButton(CHART_BUTTON)?.onClick?.()
  await settle()
  modeButton(LIST_BUTTON)?.onClick?.()
  await settle()
  expect(apiHolder.history).toHaveBeenCalledTimes(2)
})

test('the list refetches with the new endpoint and null key, never the old key', async () => {
  apiHolder.history.mockClear()
  capture.buttons.length = 0
  capture.selects.length = 0
  const endpointId = ref('endpoint-a')
  mountHistory(endpointId)
  await settle()

  capture.open?.(true)
  await settle()
  modeButton(LIST_BUTTON)?.onClick?.()
  await settle()
  keySelect()?.onUpdate?.('key-a')
  await settle()
  expect(apiHolder.history.mock.calls.at(-1)).toEqual([
    'endpoint-a',
    'key-a',
    50,
  ])

  endpointId.value = 'endpoint-b'
  await settle()
  expect(apiHolder.history.mock.calls.at(-1)).toEqual(['endpoint-b', null, 50])
  expect(
    apiHolder.history.mock.calls.some(
      ([endpoint, key]) => endpoint === 'endpoint-b' && key === 'key-a',
    ),
  ).toBe(false)
})
