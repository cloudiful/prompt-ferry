import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import type {
  DeepSeekBalance,
  EndpointProvider,
  TokenPlanKeyUsage,
  TokenPlanUsageResponse,
} from '../src/generated/admin-api'
import * as quota from '../src/models/endpoints/quota'
import * as tokenPlanFormatting from '../src/composables/useTokenPlanWindowEntries'
import * as tokenPlanBadgeRender from '../src/components/endpoints/tokenPlanBadgeRender'

// P3: the detail dialog renders the multi-currency `balances[]` contract. The
// payload fixtures below only carry `balances[]` — there is no flat
// `currency`/`total_balance` on the contract any more, so a render that still
// needed one could not even typecheck.
const componentSource = await Bun.file(
  new URL(
    '../src/components/endpoints/TokenPlanUsageDialog.vue',
    import.meta.url,
  ),
).text()

function slotStub(name: string) {
  return {
    name,
    render(this: {
      $slots: Record<string, ((scope?: unknown) => unknown) | undefined>
    }) {
      return h('div', { 'data-stub': name }, this.$slots.default?.())
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

// Repeated `balances[]` currencies must still get a unique Vue key, so the
// regression test reads the keys the template actually produced. The patched
// runtime helpers only record and forward — rendering is untouched — which lets
// a compiled SFC expose its vnode keys without reaching into Vue internals.
const capturedVNodeKeys: unknown[] = []

function recordVNodeKey(args: unknown[]): void {
  const props = args[1] as { key?: unknown } | null | undefined
  if (props && typeof props === 'object' && props.key != null) {
    capturedVNodeKeys.push(props.key)
  }
}

const instrumentedVue = {
  ...vue,
  createElementBlock: (...args: unknown[]) => {
    recordVNodeKey(args)
    return (vue.createElementBlock as (...rest: unknown[]) => unknown)(...args)
  },
  createElementVNode: (...args: unknown[]) => {
    recordVNodeKey(args)
    return (vue.createElementVNode as (...rest: unknown[]) => unknown)(...args)
  },
}

const component = (() => {
  const { descriptor } = parse(componentSource, {
    filename: 'TokenPlanUsageDialog.vue',
  })
  const script = compileScript(descriptor, {
    id: 'token-plan-usage-dialog',
    inlineTemplate: true,
  })
  const source = rewriteSfcImports(script.content).replace(
    /export default/,
    'return',
  )
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(source)
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return instrumentedVue
    if (spec === '@/composables/useTokenPlanWindowEntries') {
      return tokenPlanFormatting
    }
    if (spec === '@/components/endpoints/SubscriptionQuotaWindows.vue') {
      return {
        name: 'SubscriptionQuotaWindows',
        props: ['windows'],
        render(this: { windows: Array<{ source_window: string }> }) {
          return h(
            'div',
            { 'data-subscription-windows': true },
            this.windows.map((window) => window.source_window).join(','),
          )
        },
      }
    }
    if (spec === '@/components/endpoints/tokenPlanBadgeRender') {
      return tokenPlanBadgeRender
    }
    if (spec === '@/models/endpoints/quota') return quota
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve)
})()

// The modal and the collapsible only decide whether the body renders; both are
// stubbed open so the assertions read the payload surface itself.
const modalStub = {
  name: 'UModal',
  render(this: { $slots: Record<string, (() => unknown) | undefined> }) {
    return h('div', { 'data-stub': 'UModal' }, this.$slots.body?.())
  },
}

const badgeStub = {
  name: 'UBadge',
  props: ['label'],
  render(this: { label?: string }) {
    return h('span', { 'data-stub': 'UBadge' }, this.label)
  },
}

const collapsibleStub = {
  name: 'UCollapsible',
  render(this: {
    $slots: Record<string, ((scope?: unknown) => unknown) | undefined>
  }) {
    return h('div', { 'data-stub': 'UCollapsible' }, [
      this.$slots.default?.({ open: true }),
      this.$slots.content?.(),
    ])
  },
}

function deepseekKey(balance: DeepSeekBalance): TokenPlanKeyUsage {
  return {
    key_id: 'k-deepseek',
    key_label: 'deepseek key',
    ok: true,
    model_remains: [],
    deepseek_balance: balance,
  } as TokenPlanKeyUsage
}

function usagePayload(balance: DeepSeekBalance): TokenPlanUsageResponse {
  return {
    keys: [deepseekKey(balance)],
    provider: 'deepseek',
    provider_region: null,
  } as TokenPlanUsageResponse
}

async function render(
  usage: TokenPlanUsageResponse | null,
  options: { loading?: boolean; visible?: boolean } = {},
): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(component as never, {
        endpointId: 'endpoint-id',
        endpointName: 'deepseek endpoint',
        loading: options.loading ?? false,
        provider:
          (usage?.provider as EndpointProvider | undefined) ?? 'deepseek',
        t: (key: string) => key,
        usage,
        visible: options.visible ?? false,
      }),
  })
  app.component('UModal', modalStub as never)
  app.component('UCollapsible', collapsibleStub as never)
  app.component('UProgress', slotStub('UProgress') as never)
  app.component('UButton', slotStub('UButton') as never)
  app.component('UBadge', badgeStub as never)
  return renderToString(app)
}

test('reviewer: current quota refresh keeps the expanded history component mounted', async () => {
  const usage = {
    provider: 'openai',
    provider_region: null,
    keys: [],
  } as TokenPlanUsageResponse
  const before = await render(usage, { visible: true })
  const during = await render(usage, { visible: true, loading: true })
  expect(before).toContain('data-stub="SubscriptionQuotaHistory.vue"')
  expect(during).toContain('data-stub="SubscriptionQuotaHistory.vue"')
})

test('the dialog renders canonical windows instead of positional slots', async () => {
  const html = await render({
    keys: [
      {
        key_id: 'k-chatgpt',
        key_label: 'ChatGPT',
        ok: true,
        model_remains: [
          {
            model_name: 'plus',
            interval: { remaining_percent: 99 },
            weekly: { remaining_percent: 98 },
            windows: [
              {
                source_window: 'primary',
                window_seconds: 18_000,
                used_percent: 2,
                remaining_percent: 98,
                availability: 'known',
              },
              {
                source_window: 'secondary',
                window_seconds: 604_800,
                used_percent: null,
                remaining_percent: null,
                availability: 'unknown',
              },
            ],
          },
        ],
      },
    ],
    provider: 'openai',
    provider_region: null,
  })
  expect(html).toContain('data-subscription-windows')
  expect(html).toContain('primary,secondary')
  expect(html).not.toContain('5-hour window')
  expect(html).not.toContain('Weekly window')
})

test('a stale last-good ChatGPT snapshot is not labeled as currently available', async () => {
  const usage: TokenPlanUsageResponse = {
    keys: [
      {
        key_id: 'k-chatgpt',
        key_label: 'ChatGPT',
        ok: true,
        model_remains: [
          {
            model_name: 'plus',
            interval: null,
            weekly: null,
            windows: [],
            observation: {
              observed_at: '2026-10-10T12:00:00Z',
              source: 'manual',
              stale: true,
              last_error_code: 'upstream',
              next_retry_at: null,
              refreshing: false,
            },
          },
        ],
      },
    ],
    provider: 'openai',
    provider_region: null,
  } as TokenPlanUsageResponse

  const html = await render(usage)
  expect(html).toContain('quotaCachedObservation')
  expect(html).not.toContain('tokenPlanAvailable')
})

test('every reported currency is listed in payload order, never combined', async () => {
  const html = await render(
    usagePayload({
      balances: [
        {
          currency: 'CNY',
          total_balance: 12.5,
          granted_balance: 2,
          topped_up_balance: 10.5,
        },
        {
          currency: 'USD',
          total_balance: 3,
          granted_balance: 3,
          topped_up_balance: 0,
        },
      ],
      is_available: true,
    }),
  )

  expect(html).toContain('tokenPlanDeepSeekBalance')
  expect(html).toContain('tokenPlanAvailable')
  // Each currency keeps its own amounts; there is no summed cross-currency
  // figure anywhere in the dialog.
  expect(html).toContain('¥12.50')
  expect(html).toContain('¥2.00')
  expect(html).toContain('¥10.50')
  expect(html).toContain('$3.00')
  expect(html).toContain('CNY')
  expect(html).toContain('USD')
  expect(html).not.toContain('15.50')
  // CNY (first entry) is rendered before USD.
  expect(html.indexOf('CNY')).toBeLessThan(html.indexOf('USD'))
})

test('repeated currencies keep distinct row keys in payload order', async () => {
  capturedVNodeKeys.length = 0
  const html = await render(
    usagePayload({
      balances: [
        {
          currency: 'CNY',
          total_balance: 1,
          granted_balance: 1,
          topped_up_balance: 0,
        },
        {
          currency: 'CNY',
          total_balance: 2,
          granted_balance: 0,
          topped_up_balance: 2,
        },
      ],
      is_available: true,
    }),
  )

  // Two same-currency entries both render, in payload order, never merged.
  expect((html.match(/CNY/g) ?? []).length).toBe(2)
  expect(html).toContain('¥1.00')
  expect(html).toContain('¥2.00')
  expect(html.indexOf('¥1.00')).toBeLessThan(html.indexOf('¥2.00'))

  // The rows must be keyed uniquely: two `CNY` entries cannot share a key or
  // Vue would mis-patch/drop one when the payload updates.
  const currencyKeys = capturedVNodeKeys.filter(
    (key) => typeof key === 'string' && key.includes('CNY'),
  )
  expect(currencyKeys).toHaveLength(2)
  expect(new Set(currencyKeys).size).toBe(2)
})

test('an unknown amount is named instead of rendered as a zero', async () => {
  const html = await render(
    usagePayload({
      balances: [
        {
          currency: 'CNY',
          total_balance: 12.5,
          granted_balance: null,
          topped_up_balance: 12.5,
        },
        {
          currency: 'USD',
          total_balance: null,
          granted_balance: null,
          topped_up_balance: null,
        },
      ],
      is_available: true,
    }),
  )

  expect(html).toContain('tokenPlanBalanceUnknown')
  // The known currency is still shown; the unknown one is not turned into $0.
  expect(html).toContain('¥12.50')
  expect(html).not.toContain('$0.00')
})

test('a real zero stays a real zero and availability stays independent', async () => {
  const html = await render(
    usagePayload({
      balances: [
        {
          currency: 'CNY',
          total_balance: 0,
          granted_balance: 0,
          topped_up_balance: 0,
        },
      ],
      is_available: false,
    }),
  )

  expect(html).toContain('¥0.00')
  // `is_available` is the routing input and is reported on its own, not folded
  // into the amounts.
  expect(html).toContain('tokenPlanUnavailable')
  expect(html).not.toContain('tokenPlanBalanceUnknown')
})

test('a DeepSeek key without a balance payload renders no balance block', async () => {
  const usage: TokenPlanUsageResponse = {
    keys: [
      {
        key_id: 'k',
        key_label: 'k',
        ok: true,
        model_remains: [],
      } as unknown as TokenPlanKeyUsage,
    ],
    provider: 'deepseek',
    provider_region: null,
  } as TokenPlanUsageResponse
  const html = await render(usage)
  expect(html).not.toContain('tokenPlanDeepSeekBalance')
})
