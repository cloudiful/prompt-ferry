import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import type {
  CanonicalQuotaWindow,
  DeepSeekBalance,
  EndpointProvider,
  GlmWindowUsage,
  TokenPlanKeyUsage,
  TokenPlanUsageResponse,
} from '../src/generated/admin-api'
import * as quota from '../src/models/endpoints/quota'
import * as tokenPlanFormatting from '../src/composables/useTokenPlanWindowEntries'
import * as canonicalQuota from '../src/composables/useCanonicalQuotaWindows'
import * as subscriptionQuota from '../src/composables/subscriptionQuotaWindows'
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

const rowSource = await Bun.file(
  new URL(
    '../src/components/endpoints/TokenPlanQuotaWindowRow.vue',
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

// P3: the shared quota row is compiled for real so dialog assertions read
// actual percent/caption/reset output, not a stubbed placeholder. Its own
// imports resolve to the real adapter and formatting helpers.
const rowComponent = (() => {
  const { descriptor } = parse(rowSource, {
    filename: 'TokenPlanQuotaWindowRow.vue',
  })
  const script = compileScript(descriptor, {
    id: 'token-plan-quota-window-row',
    inlineTemplate: true,
  })
  const source = rewriteSfcImports(script.content).replace(
    /export default/,
    'return',
  )
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(source)
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return instrumentedVue
    if (spec === '@/composables/useCanonicalQuotaWindows') {
      return canonicalQuota
    }
    if (spec === '@/composables/useTokenPlanWindowEntries') {
      return tokenPlanFormatting
    }
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve)
})()

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
    if (spec === '@/composables/useCanonicalQuotaWindows') {
      return canonicalQuota
    }
    if (spec === '@/composables/subscriptionQuotaWindows') {
      return subscriptionQuota
    }
    if (spec === '@/components/endpoints/TokenPlanQuotaWindowRow.vue') {
      return rowComponent
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

function glmKey(
  fiveHour: GlmWindowUsage | null,
  weekly: GlmWindowUsage | null,
): TokenPlanKeyUsage {
  return {
    key_id: 'k-glm',
    key_label: 'glm key',
    ok: true,
    model_remains: [],
    glm_five_hour: fiveHour,
    glm_weekly: weekly,
  } as TokenPlanKeyUsage
}

function glmUsage(
  fiveHour: GlmWindowUsage | null,
  weekly: GlmWindowUsage | null,
): TokenPlanUsageResponse {
  return {
    keys: [glmKey(fiveHour, weekly)],
    provider: 'glm',
    provider_region: null,
  } as TokenPlanUsageResponse
}

// Canonical observation mirroring the P1 backend projection: amounts are
// only present when actually observed, so the caption is honest.
function glmObservation(
  sourceId: string,
  kind: CanonicalQuotaWindow['kind'],
  unit: string | null,
  current: number | null,
  limit: number | null,
  usedPercent: number | null,
): CanonicalQuotaWindow {
  return {
    source_id: sourceId,
    kind,
    source_model: null,
    unit,
    duration_seconds: null,
    is_default_scope: true,
    availability: usedPercent === null ? 'unknown' : 'known',
    remaining_percent: usedPercent === null ? null : 100 - usedPercent,
    current,
    limit,
    remaining: current,
    reset_at: null,
  }
}

function glmCanonicalUsage(
  fiveHour: GlmWindowUsage | null,
  weekly: GlmWindowUsage | null,
  observations: CanonicalQuotaWindow[],
): TokenPlanUsageResponse {
  return {
    keys: [
      {
        ...glmKey(fiveHour, weekly),
        quota_windows: observations,
      } as TokenPlanKeyUsage,
    ],
    provider: 'glm',
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

test('the dialog renders canonical model rows with actual durations', async () => {
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
  // No raw positional interval/weekly branch and no stubbed window list:
  // the account model renders its actual durations through the shared row.
  expect(html).not.toContain('data-subscription-windows')
  expect(html).toContain('plus')
  expect(html).toContain('tokenPlanInterval')
  expect(html).toContain('tokenPlanWeekly')
  expect(html).toContain('98.0%')
  expect(html).toContain('—')
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
  // Observation metadata stays visible even with no rows; no quota percent
  // is invented and no availability is claimed.
  expect(html).toContain('plus')
  expect(html).not.toContain('tokenPlanAvailable')
  expect(html).not.toContain('98.0%')
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

test('a GLM window with unknown quantity shows no 0.00 / 0.00 subline', async () => {
  const html = await render(
    glmUsage(
      { current_value: 0, limit: 0, remaining: 0, percentage: null },
      null,
    ),
  )
  expect(html).toContain('tokenPlanInterval')
  // The quantity is unknown, so no used/total caption may be manufactured;
  // the window itself stays visible.
  expect(html).not.toContain('0.00 / 0.00')
})

test('a GLM window with reported zero usage keeps its remaining percent', async () => {
  const html = await render(
    glmUsage(
      { current_value: 0, limit: 0, remaining: 100, percentage: 0 },
      null,
    ),
  )
  expect(html).toContain('100.0%')
  expect(html).not.toContain('0.00 / 0.00')
})

test('a legacy GLM window hides its ambiguous quantity caption', async () => {
  // Old backends default missing amounts to `0`, so a legacy `0 / 2000`
  // cannot be told apart from a true zero: the caption hides while the
  // window and its honestly reported percent stay visible.
  const html = await render(
    glmUsage(
      { current_value: 0, limit: 2000, remaining: 2000, percentage: 0 },
      null,
    ),
  )
  expect(html).toContain('tokenPlanInterval')
  expect(html).toContain('100.0%')
  expect(html).not.toContain('0.00 / 2000.00')
})

test('a canonical GLM window with observed amounts keeps its caption', async () => {
  const html = await render(
    glmCanonicalUsage(
      { current_value: 0, limit: 2000, remaining: 2000, percentage: 0 },
      null,
      [glmObservation('glm:five_hour', 'five_hour', 'tokens', 0, 2000, 0)],
    ),
  )
  expect(html).toContain('tokenPlanInterval')
  expect(html).toContain('100.0%')
  expect(html).toContain('0.00 / 2000.00')
})

test('an exhausted GLM weekly window keeps both labels and zero remaining', async () => {
  const html = await render(
    glmUsage(
      { current_value: 0, limit: 2000, remaining: 2000, percentage: 0 },
      {
        current_value: 10000,
        limit: 10000,
        remaining: 0,
        percentage: 100,
      },
    ),
  )
  expect(html).toContain('tokenPlanInterval')
  expect(html).toContain('tokenPlanWeekly')
  expect(html).toContain('0.0%')
  // Legacy quantities are ambiguous: no caption is manufactured for either
  // window, while both windows and the exhausted percent stay visible.
  expect(html).not.toContain('0.00 / 2000.00')
  expect(html).not.toContain('10000.00 / 10000.00')
})

test('a canonical exhausted GLM weekly window keeps both quantity captions', async () => {
  const html = await render(
    glmCanonicalUsage(
      { current_value: 0, limit: 2000, remaining: 2000, percentage: 0 },
      {
        current_value: 10000,
        limit: 10000,
        remaining: 0,
        percentage: 100,
      },
      [
        glmObservation('glm:five_hour', 'five_hour', 'tokens', 0, 2000, 0),
        glmObservation('glm:weekly', 'weekly', 'credits', 10000, 10000, 100),
      ],
    ),
  )
  expect(html).toContain('tokenPlanInterval')
  expect(html).toContain('tokenPlanWeekly')
  expect(html).toContain('0.00 / 2000.00')
  expect(html).toContain('10000.00 / 10000.00')
  expect(html).toContain('0.0%')
})

function commandCodeKey(
  fiveHour: {
    used: number
    cap: number
    remaining_percent?: number | null
    reset_at?: string | null
  } | null,
  weekly: {
    used: number
    cap: number
    remaining_percent?: number | null
    reset_at?: string | null
  } | null,
): TokenPlanKeyUsage {
  return {
    key_id: 'k-cc',
    key_label: 'cc key',
    ok: true,
    model_remains: [],
    five_hour: fiveHour,
    weekly,
  } as unknown as TokenPlanKeyUsage
}

function commandCodeUsage(
  fiveHour: {
    used: number
    cap: number
    remaining_percent?: number | null
    reset_at?: string | null
  } | null,
  weekly: {
    used: number
    cap: number
    remaining_percent?: number | null
    reset_at?: string | null
  } | null,
): TokenPlanUsageResponse {
  return {
    keys: [commandCodeKey(fiveHour, weekly)],
    provider: 'command_code',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse
}

test('a known CommandCode 5h and weekly pair renders both USD rows', async () => {
  const html = await render(
    commandCodeUsage(
      { used: 20, cap: 100, remaining_percent: 80, reset_at: null },
      { used: 30, cap: 100, remaining_percent: 70, reset_at: null },
    ),
  )
  expect(html).toContain('tokenPlanFiveHour')
  expect(html).toContain('tokenPlanWeeklyUsd')
  expect(html).toContain('80.0%')
  expect(html).toContain('70.0%')
  // The USD caption the CommandCode section has always shown.
  expect(html).toContain('20.00 / 100.00 USD')
  expect(html).toContain('30.00 / 100.00 USD')
})

test('an unknown CommandCode window shows a dash, never a fabricated bar', async () => {
  const html = await render(
    commandCodeUsage(
      {
        used: Number.NaN,
        cap: Number.NaN,
        remaining_percent: null,
        reset_at: null,
      } as unknown as {
        used: number
        cap: number
        remaining_percent?: number | null
        reset_at?: string | null
      },
      null,
    ),
  )
  expect(html).toContain('tokenPlanFiveHour')
  expect(html).toContain('—')
  expect(html).not.toContain('100.0%')
  expect(html).not.toContain('0.0%')
})

test('an expired reset hides the percent but keeps the amounts honest', async () => {
  const past = new Date(Date.now() - 3_600_000).toISOString()
  const html = await render({
    keys: [
      {
        key_id: 'k-expired',
        key_label: 'expired key',
        ok: true,
        model_remains: [],
        quota_windows: [
          {
            source_id: 'command_code:five_hour',
            kind: 'five_hour',
            source_model: null,
            unit: 'currency',
            duration_seconds: null,
            is_default_scope: true,
            availability: 'known',
            remaining_percent: 75,
            current: 25,
            limit: 100,
            remaining: 75,
            reset_at: past,
          },
        ],
      } as unknown as TokenPlanKeyUsage,
    ],
    provider: 'command_code',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse)
  // Expired: the bar degrades to the same dash as unknown, while the
  // observed amounts stay in the caption.
  expect(html).toContain('—')
  expect(html).not.toContain('75.0%')
  expect(html).toContain('25.00 / 100.00 USD')
})

test('a failed key renders its error and no quota rows', async () => {
  const html = await render({
    keys: [
      {
        key_id: 'k-failed',
        key_label: 'failed key',
        ok: false,
        error_message: 'upstream boom',
        model_remains: [],
        quota_windows: [
          glmObservation('glm:five_hour', 'five_hour', 'tokens', 0, 2000, 0),
        ],
      } as unknown as TokenPlanKeyUsage,
    ],
    provider: 'glm',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse)
  expect(html).toContain('upstream boom')
  // Failed keys carry no rows: neither the window nor its percent renders.
  expect(html).not.toContain('tokenPlanInterval')
  expect(html).not.toContain('100.0%')
})

async function renderRow(
  row: Parameters<typeof canonicalQuota.toCanonicalQuotaRows>[0] extends never
    ? never
    : import('../src/composables/useCanonicalQuotaWindows').CanonicalQuotaRow,
  label = 'row label',
): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(rowComponent as never, {
        row,
        label,
        t: (key: string) => key,
        nowMs: Date.now(),
        density: 'full' as const,
      }),
  })
  app.component('UProgress', slotStub('UProgress') as never)
  return renderToString(app)
}

test('balance and unlimited rows are text-only with no progress percent', async () => {
  const balanceHtml = await renderRow({
    sourceId: 'openrouter:balance',
    kind: 'balance',
    sourceModel: null,
    unit: 'credits',
    durationSeconds: null,
    isDefaultScope: true,
    availability: 'known',
    observedRemainingPercent: null,
    remainingPercent: null,
    current: 5,
    limit: 10,
    remaining: 5,
    resetAt: null,
    expired: false,
  })
  expect(balanceHtml).toContain('5.00 / 10.00')
  expect(balanceHtml).not.toContain('%')
  const unlimitedHtml = await renderRow({
    sourceId: 'openrouter:unlimited',
    kind: 'unlimited',
    sourceModel: null,
    unit: 'credits',
    durationSeconds: null,
    isDefaultScope: true,
    availability: 'known',
    observedRemainingPercent: null,
    remainingPercent: null,
    current: null,
    limit: null,
    remaining: null,
    resetAt: null,
    expired: false,
  })
  // No amounts and no synthetic percent: the same dash as unknown windows.
  expect(unlimitedHtml).toContain('—')
  expect(unlimitedHtml).not.toContain('%')
})

test('a capless balance row shows its observed total instead of a dash', async () => {
  const caplessHtml = await renderRow({
    sourceId: 'deepseek:CNY',
    kind: 'balance',
    sourceModel: null,
    unit: 'CNY',
    durationSeconds: null,
    isDefaultScope: true,
    availability: 'known',
    observedRemainingPercent: null,
    remainingPercent: null,
    current: 9.91,
    limit: null,
    remaining: null,
    resetAt: null,
    expired: false,
  })
  // The persisted DeepSeek total renders rather than degrading to `—`.
  expect(caplessHtml).toContain('9.91')
  expect(caplessHtml).not.toContain('—')
  expect(caplessHtml).not.toContain('%')

  const zeroHtml = await renderRow({
    sourceId: 'deepseek:USD',
    kind: 'balance',
    sourceModel: null,
    unit: 'USD',
    durationSeconds: null,
    isDefaultScope: true,
    availability: 'known',
    observedRemainingPercent: null,
    remainingPercent: null,
    current: 0,
    limit: null,
    remaining: null,
    resetAt: null,
    expired: false,
  })
  expect(zeroHtml).toContain('0.00')
  expect(zeroHtml).not.toContain('—')
})

function minimaxKey(
  modelName: string,
  remainingPercent: number | null,
): TokenPlanKeyUsage {
  return {
    key_id: `k-${modelName}`,
    key_label: `${modelName} key`,
    ok: true,
    model_remains: [
      {
        model_name: modelName,
        interval: { remaining_percent: remainingPercent },
        weekly: null,
        windows: null,
        observation: null,
      },
    ],
  } as unknown as TokenPlanKeyUsage
}

test('a valid MiniMax general model renders through the shared row', async () => {
  const html = await render({
    keys: [minimaxKey('general', 80)],
    provider: 'minimax',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse)
  // Previously crashed on an undefined raw helper; now the canonical model
  // row renders its honest percent with no stub involved.
  expect(html).toContain('general')
  expect(html).toContain('tokenPlanInterval')
  expect(html).toContain('80.0%')
})

test('the quota summary trigger wraps and cannot force the dialog grid wide', async () => {
  const html = await render({
    keys: [minimaxKey('general', 80)],
    provider: 'minimax',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse)

  // The key summary row is flexible and wraps its label/status/minimum instead
  // of holding a shrink-0 block that widens the auto grid track.
  expect(html).toContain('flex-wrap items-center justify-between')
  expect(html).toContain('tokenPlanMinRemaining')
  expect(html).toContain('tokenPlanWindowCount')
  expect(html).not.toContain('shrink-0 text-xs text-dimmed')

  // The dialog body pins a shrinkable grid track and its wrappers to min-w-0.
  expect(html).toContain('grid-cols-[minmax(0,1fr)]')
  expect(html.match(/min-w-0/g)?.length ?? 0).toBeGreaterThanOrEqual(3)
})

test('canonical subscription wins over raw windows in dialog and header', async () => {
  const html = await render({
    keys: [
      {
        key_id: 'k-conflict',
        key_label: 'conflict key',
        ok: true,
        model_remains: [
          {
            model_name: 'plus',
            interval: null,
            weekly: null,
            windows: [
              {
                source_window: 'primary',
                window_seconds: 18_000,
                used_percent: 20,
                remaining_percent: 80,
                reset_at: null,
                reset_after_seconds: null,
                availability: 'known',
              },
            ],
            observation: null,
          },
        ],
        quota_windows: [
          {
            source_id: 'subscription:primary',
            kind: 'custom',
            source_model: 'plus',
            unit: null,
            duration_seconds: 18_000,
            is_default_scope: true,
            availability: 'known',
            remaining_percent: 25,
            current: null,
            limit: null,
            remaining: null,
            reset_at: null,
          },
        ],
      } as unknown as TokenPlanKeyUsage,
    ],
    provider: 'openai',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse)
  expect(html).toContain('25.0%')
  expect(html).not.toContain('80.0%')
})

test('an authoritative empty shows no quota rows or header windows', async () => {
  const html = await render({
    keys: [
      {
        key_id: 'k-empty',
        key_label: 'empty key',
        ok: true,
        model_remains: [
          {
            model_name: 'plus',
            interval: null,
            weekly: null,
            windows: [
              {
                source_window: 'primary',
                window_seconds: 18_000,
                used_percent: 20,
                remaining_percent: 80,
                reset_at: null,
                reset_after_seconds: null,
                availability: 'known',
              },
            ],
            observation: null,
          },
        ],
        quota_windows: [],
      } as unknown as TokenPlanKeyUsage,
    ],
    provider: 'openai',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse)
  // Authoritative empty: assessed, not missing — no legacy fallback fills it.
  expect(html).toContain('empty key')
  expect(html).not.toContain('80.0%')
  expect(html).not.toContain('tokenPlanInterval')
})

test('an expired canonical window shows no raw fallback percent', async () => {
  const html = await render({
    keys: [
      {
        key_id: 'k-expired',
        key_label: 'expired key',
        ok: true,
        model_remains: [
          {
            model_name: 'plus',
            interval: null,
            weekly: null,
            windows: [
              {
                source_window: 'primary',
                window_seconds: 18_000,
                used_percent: 20,
                remaining_percent: 80,
                reset_at: null,
                reset_after_seconds: null,
                availability: 'known',
              },
            ],
            observation: null,
          },
        ],
        quota_windows: [
          {
            source_id: 'subscription:primary',
            kind: 'custom',
            source_model: 'plus',
            unit: null,
            duration_seconds: 18_000,
            is_default_scope: true,
            availability: 'known',
            remaining_percent: 75,
            current: null,
            limit: null,
            remaining: null,
            reset_at: '2000-01-01T00:00:00.000Z',
          },
        ],
      } as unknown as TokenPlanKeyUsage,
    ],
    provider: 'openai',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse)
  expect(html).toContain('—')
  expect(html).not.toContain('75.0%')
  expect(html).not.toContain('80.0%')
})

test('detail exhaustion never binds the dialog pool minimum', async () => {
  const html = await render({
    keys: [
      {
        key_id: 'k-scope',
        key_label: 'scope key',
        ok: true,
        model_remains: [],
        quota_windows: [
          {
            source_id: 'subscription:primary',
            kind: 'custom',
            source_model: 'chatgpt',
            unit: null,
            duration_seconds: 18_000,
            is_default_scope: true,
            availability: 'known',
            remaining_percent: 80,
            current: null,
            limit: null,
            remaining: null,
            reset_at: null,
          },
          {
            source_id: 'subscription:detail',
            kind: 'custom',
            source_model: 'detail-model',
            unit: null,
            duration_seconds: 18_000,
            is_default_scope: false,
            availability: 'known',
            remaining_percent: 0,
            current: null,
            limit: null,
            remaining: null,
            reset_at: null,
          },
        ],
      } as unknown as TokenPlanKeyUsage,
    ],
    provider: 'openai',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse)
  // Both rows stay visible, but the header pool minimum keeps the default 80.
  expect(html).toContain('80.0%')
  expect(html).toContain('0.0%')
  expect(html).toContain('tokenPlanMinRemaining')
})

test('capable providers mount the per-key observation history instead of the legacy panel', async () => {
  const usage = {
    provider: 'minimax',
    provider_region: null,
    keys: [],
  } as TokenPlanUsageResponse
  const html = await render(usage, { visible: true })
  expect(html).toContain('data-stub="KeyQuotaObservationHistory.vue"')
  expect(html).not.toContain('data-stub="SubscriptionQuotaHistory.vue"')
})

test('openai keeps the legacy history panel alongside the per-key history', async () => {
  const usage = {
    provider: 'openai',
    provider_region: null,
    keys: [],
  } as TokenPlanUsageResponse
  const html = await render(usage, { visible: true })
  expect(html).toContain('data-stub="SubscriptionQuotaHistory.vue"')
  expect(html).toContain('data-stub="KeyQuotaObservationHistory.vue"')
})

test('generic endpoints mount no history panel', async () => {
  const html = await render(
    {
      provider: 'generic',
      provider_region: null,
      keys: [],
    } as TokenPlanUsageResponse,
    { visible: true },
  )
  expect(html).not.toContain('data-stub="KeyQuotaObservationHistory.vue"')
  expect(html).not.toContain('data-stub="SubscriptionQuotaHistory.vue"')
})
