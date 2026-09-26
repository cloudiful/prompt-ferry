import { mock } from 'bun:test'
import * as vue from 'vue'
import {
  computed,
  createSSRApp,
  defineComponent,
  h,
  type ComputedRef,
} from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import type { EndpointListItemView } from '../../src/models/endpoints'
import { isQuotaEligible } from '../../src/models/endpoints/quota'

// SSR harness for the two upstream-list SFCs. The repo has no
// component-test setup, so the components are compiled with the official
// compiler and rendered with stubbed children — the same shape as
// `endpoint-oauth.test.ts`. Kept out of the spec file so the eligibility
// assertions stay readable.

const IDENTITY_T = (key: string) => key

export const prefetchTokenPlanBatch = mock(
  async (_ids: Iterable<string>, _concurrency?: number) => {},
)

function rewriteSfcImports(code: string): string {
  const pattern = /import\s+([^;'"]+?)\s+from\s+(['"])([^'"]+)\2;?/g
  return code.replace(
    pattern,
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
        // `type X` inside a value import is dropped by the compiler anyway.
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

// Marker labels carry the endpoint id so every assertion can attribute a
// badge to the exact row that rendered it.
const badgeStubs = {
  useTokenPlanBadges: (
    id: string | ComputedRef<string>,
  ): ComputedRef<{ marker: string }> =>
    computed(() => ({
      marker: `QUOTA:${typeof id === 'string' ? id : id.value}`,
    })),
  tokenPlanBadgePills: (source: { marker: string }) => [
    { label: source.marker, color: '', title: '' },
  ],
}

const stub = (name: string) =>
  defineComponent({
    name,
    setup(_props, { slots }) {
      return () => h('div', { 'data-stub': name }, slots.default?.())
    },
  })

const buttonStub = defineComponent({
  name: 'UButton',
  inheritAttrs: false,
  setup(_props, { slots, attrs }) {
    return () =>
      h('button', { ...attrs, 'data-stub': 'UButton' }, slots.default?.())
  },
})

// Minimal UTable: renders the columns marker and feeds each row through the
// usage/actions cells exactly like the real table's slot contract.
const tableStub = defineComponent({
  name: 'UTable',
  props: {
    columns: { type: Array, required: true },
    data: { type: Array, required: true },
  },
  setup(props, { slots }) {
    return () => {
      const columns = (
        props.columns as Array<{ id?: string; accessorKey?: string }>
      )
        .map((column) => column.id ?? column.accessorKey ?? '')
        .filter(Boolean)
        .join(',')
      const rows = (props.data as Array<Record<string, unknown>>).map((row) =>
        h('div', { 'data-row': String(row.endpoint_id) }, [
          h(
            'div',
            { 'data-cell': 'usage' },
            slots['usage-cell']?.({ row: { original: row } }),
          ),
          h(
            'div',
            { 'data-cell': 'actions' },
            slots['actions-cell']?.({ row: { original: row } }),
          ),
        ]),
      )
      return h('div', [h('div', { 'data-columns': columns }), ...rows])
    }
  },
})

const MODULE_STUBS: Record<string, unknown> = {
  '@/composables/useTokenPlanBadges': badgeStubs,
  '@/composables/useTokenPlanUsageCache': { prefetchTokenPlanBatch },
  '@/models/endpoints/quota': { isQuotaEligible },
  '@/table-pagination': { STANDARD_PAGE_SIZE_OPTIONS: [10, 25, 50] },
}

function resolveSpec(spec: string): unknown {
  if (spec === 'vue') return vue
  return MODULE_STUBS[spec] ?? stub(spec.split('/').pop() ?? spec)
}

function compileSfc(source: string, filename: string): Parameters<typeof h>[0] {
  const { descriptor } = parse(source, { filename })
  const { content } = compileScript(descriptor, {
    id: filename,
    inlineTemplate: true,
  })
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    rewriteSfcImports(content).replace(/export default/, 'return'),
  )
  return new Function('__resolve', `"use strict";\n${js}`)(
    resolveSpec,
  ) as Parameters<typeof h>[0]
}

async function loadListComponent(
  filename: string,
): Promise<Parameters<typeof h>[0]> {
  const url = new URL(
    `../../src/components/endpoints/${filename}`,
    import.meta.url,
  )
  return compileSfc(await Bun.file(url).text(), filename)
}

const tableComponent = await loadListComponent('EndpointsTable.vue')
const cardComponent = await loadListComponent('EndpointMobileCard.vue')

function render(
  component: Parameters<typeof h>[0],
  props: Record<string, unknown>,
): Promise<string> {
  const app = createSSRApp({ render: () => h(component, props) })
  app.component('UButton', buttonStub)
  app.component('UTable', tableStub)
  app.component('UTooltip', stub('UTooltip'))
  app.component('UIcon', stub('UIcon'))
  app.component('USwitch', stub('USwitch'))
  app.component('UBadge', stub('UBadge'))
  app.component('TablePagination', stub('TablePagination'))
  app.component('TestResultPopover', stub('TestResultPopover'))
  app.component('ProviderIcon', stub('ProviderIcon'))
  return renderToString(app)
}

export function renderEndpointsTable(
  items: EndpointListItemView[],
): Promise<string> {
  return render(tableComponent, {
    busy: false,
    first: 0,
    items,
    rows: 10,
    t: IDENTITY_T,
    total: items.length,
  })
}

export function renderEndpointMobileCard(
  item: EndpointListItemView,
): Promise<string> {
  return render(cardComponent, { busy: false, item, t: IDENTITY_T })
}
