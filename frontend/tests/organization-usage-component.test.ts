import { expect, test } from 'bun:test'
import { ref } from 'vue'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import type {
  EndpointProvider,
  OpenAiOrganizationUsageResponse,
} from '../src/generated/admin-api'
import * as quota from '../src/models/endpoints/quota'
import * as tokenPlanFormatting from '../src/composables/useTokenPlanWindowEntries'
import type { OrganizationUsageSource } from '../src/composables/useEndpointOrganizationUsage'

// SSR SFC render harness: the repo has no component-test setup, so the
// component is compiled with the official compiler and its imports resolved
// to stubs/real helpers. The composable is stubbed with controllable refs.
const usage = ref<OpenAiOrganizationUsageResponse | null>(null)
const loading = ref(false)
const errorMessage = ref('')

const composableStub = {
  useEndpointOrganizationUsage: () => ({
    errorMessage,
    loadOrganizationUsage: async (_source: OrganizationUsageSource) => {},
    organizationUsage: usage,
    organizationUsageLoading: loading,
    resetOrganizationUsage: () => {},
  }),
}

const componentSource = await Bun.file(
  new URL(
    '../src/components/endpoints/EndpointOrganizationUsage.vue',
    import.meta.url,
  ),
).text()

function slotStub(name: string, slot = 'default') {
  return {
    name,
    render(this: { $slots: Record<string, (() => unknown) | undefined> }) {
      return h('div', { 'data-stub': name }, this.$slots[slot]?.())
    },
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
      if (fallback) {
        statements.push(
          `const ${fallback} = __resolve(${JSON.stringify(spec)});`,
        )
      }
      for (const part of named?.[1].split(',') ?? []) {
        const entry = part.trim()
        if (!entry) continue
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
    filename: 'EndpointOrganizationUsage.vue',
  })
  const script = compileScript(descriptor, {
    id: 'endpoint-organization-usage',
    inlineTemplate: true,
  })
  const source = rewriteSfcImports(script.content).replace(
    /export default/,
    'return',
  )
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(source)
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return vue
    if (spec === '@/composables/useTokenPlanWindowEntries') {
      return tokenPlanFormatting
    }
    if (spec === '@/composables/useEndpointOrganizationUsage') {
      return composableStub
    }
    if (spec === '@/models/endpoints/quota') return quota
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve)
})()

async function render(props: {
  endpointId?: string
  provider?: EndpointProvider
  hasAdminApiKey?: boolean
}): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(component as never, {
        endpointId: props.endpointId ?? 'endpoint-1',
        provider: props.provider ?? 'openai',
        hasAdminApiKey: props.hasAdminApiKey ?? true,
        t: (key: string) => key,
      }),
  })
  app.component('UButton', slotStub('UButton'))
  app.component('UBadge', slotStub('UBadge'))
  return renderToString(app)
}

const usageFixture: OpenAiOrganizationUsageResponse = {
  cached: true,
  cost_usd: 12.5,
  currency: 'usd',
  fetched_at: '2026-09-27T10:00:00Z',
  input_tokens: 1000,
  output_tokens: 250,
  period_end: '2026-09-27T10:00:00Z',
  period_start: '2026-09-01T00:00:00Z',
  provider: 'openai',
  total_tokens: 1250,
  truncated: true,
}

test('an unsaved endpoint asks for a save before querying usage', async () => {
  const html = await render({ endpointId: '' })
  expect(html).toContain('endpointOrganizationUsageSaveFirst')
  expect(html).not.toContain('endpointOrganizationUsageLoad')
})

test('a missing Admin API Key shows the actionable hint, no query button', async () => {
  const missingKey = await render({ hasAdminApiKey: false })
  expect(missingKey).toContain('endpointOrganizationUsageRequiredKey')
  expect(missingKey).not.toContain('endpointOrganizationUsageLoad')

  const nonOpenAi = await render({ provider: 'generic', hasAdminApiKey: true })
  expect(nonOpenAi).toContain('endpointOrganizationUsageRequiredKey')
  expect(nonOpenAi).not.toContain('endpointOrganizationUsageLoad')
})

test('an eligible endpoint offers the query button', async () => {
  const html = await render({})
  expect(html).toContain('endpointOrganizationUsageHint')
  expect(html).toContain('endpointOrganizationUsageLoad')
  expect(html).not.toContain('endpointOrganizationUsageRequiredKey')
})

test('loaded organization usage renders tokens, spend, period and flags', async () => {
  usage.value = usageFixture
  const html = await render({})
  expect(html).toContain('endpointOrganizationUsagePeriod')
  expect(html).toContain('endpointOrganizationUsageInputTokens')
  expect(html).toContain('1,000')
  expect(html).toContain('endpointOrganizationUsageOutputTokens')
  expect(html).toContain('endpointOrganizationUsageTotalTokens')
  expect(html).toContain('1,250')
  expect(html).toContain('endpointOrganizationUsageCost')
  expect(html).toContain('$12.50')
  expect(html).toContain('endpointOrganizationUsageCached')
  expect(html).toContain('endpointOrganizationUsageTruncated')
  expect(html).toContain('endpointOrganizationUsageReload')
  usage.value = null
})

test('errors are shown inline instead of placeholder numbers', async () => {
  errorMessage.value = 'organization usage unavailable'
  const html = await render({})
  expect(html).toContain('organization usage unavailable')
  expect(html).not.toContain('endpointOrganizationUsageTotalTokens')
  errorMessage.value = ''
})
