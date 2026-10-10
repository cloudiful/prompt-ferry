import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h, ref } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import type {
  SubscriptionQuotaObservation,
  SubscriptionWindowUsage,
  TokenPlanKeyUsage,
} from '../src/generated/admin-api'
import * as subscriptionQuotaWindows from '../src/composables/subscriptionQuotaWindows'
import * as tokenPlanWindowEntries from '../src/composables/useTokenPlanWindowEntries'

const translate = ((key: string, options?: { duration?: string }) =>
  options?.duration ? `${key}:${options.duration}` : key) as TranslateFn

function windowUsage(
  source_window: string,
  window_seconds: number | null,
  remaining_percent: number | null,
  availability: 'known' | 'unknown' = 'known',
): SubscriptionWindowUsage {
  return {
    source_window,
    window_seconds,
    remaining_percent,
    used_percent: remaining_percent === null ? null : 100 - remaining_percent,
    availability,
    reset_at: null,
    reset_after_seconds: null,
  }
}

function quotaKey(windows: SubscriptionWindowUsage[]): TokenPlanKeyUsage {
  return {
    key_id: 'chatgpt',
    key_label: 'ChatGPT',
    ok: true,
    model_remains: [
      {
        model_name: 'subscription',
        interval: { remaining_percent: 99 },
        weekly: { remaining_percent: 98 },
        windows,
      },
    ],
  }
}

test('duration labels and badge bands use reported duration', () => {
  expect(
    subscriptionQuotaWindows.subscriptionWindowLabel(
      windowUsage('primary', 18_000, 40),
      translate,
    ),
  ).toBe('tokenPlanInterval')
  expect(
    subscriptionQuotaWindows.subscriptionWindowLabel(
      windowUsage('secondary', 604_800, 60),
      translate,
    ),
  ).toBe('tokenPlanWeekly')
  expect(
    subscriptionQuotaWindows.subscriptionWindowLabel(
      windowUsage('primary', 43_200, 50),
      translate,
    ),
  ).toBe('tokenPlanDurationWindow:12h')
  expect(
    subscriptionQuotaWindows.subscriptionWindowLabel(
      windowUsage('primary', null, 50),
      translate,
    ),
  ).toBe('tokenPlanWindowDurationUnknown')
  expect(
    subscriptionQuotaWindows.subscriptionWindowBand(
      windowUsage('primary', 43_200, 50),
    ),
  ).toBe('short')
  expect(
    subscriptionQuotaWindows.subscriptionWindowBand(
      windowUsage('secondary', 86_400, 50),
    ),
  ).toBe('long')
  expect(
    subscriptionQuotaWindows.subscriptionWindowBand(
      windowUsage('primary', null, 50),
    ),
  ).toBeNull()
})

test('historical relative reset countdowns stay anchored to the observation time', () => {
  const window = {
    ...windowUsage('primary', 18_000, 50),
    reset_after_seconds: 3_600,
  }
  expect(
    subscriptionQuotaWindows.subscriptionWindowResetMs(
      window,
      1_000_000,
      100_000,
    ),
  ).toBe(2_700_000)
  expect(
    subscriptionQuotaWindows.subscriptionWindowResetMs(
      window,
      3_800_000,
      100_000,
    ),
  ).toBe(-100_000)
})

test('canonical summary counts all windows but excludes unknown usage and keeps zero', () => {
  const key = quotaKey([
    windowUsage('primary', 18_000, 0),
    windowUsage('secondary', 604_800, null, 'unknown'),
    windowUsage('extra', 43_200, 70),
  ])
  const entries = tokenPlanWindowEntries.useTokenPlanWindowEntries(
    translate,
    ref(Date.now()),
  )
  expect(entries.keyWindowCount(key)).toBe(3)
  expect(entries.minimumRemainingPercent(key)).toBe(0)
  expect(
    subscriptionQuotaWindows.subscriptionQuotaMinimumForBand(key, 'short'),
  ).toBe(0)
  expect(
    subscriptionQuotaWindows.subscriptionQuotaMinimumForBand(key, 'long'),
  ).toBeNull()
})

function slotStub(name: string) {
  return {
    name,
    render(this: { $slots: Record<string, (() => unknown) | undefined> }) {
      return h('div', { 'data-stub': name }, this.$slots.default?.())
    },
  }
}

function rewriteImports(code: string): string {
  return code.replace(
    /import\s+([^;'\"]+?)\s+from\s+(['\"])([^'\"]+)\2;?/g,
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

const componentSource = await Bun.file(
  new URL(
    '../src/components/endpoints/SubscriptionQuotaWindows.vue',
    import.meta.url,
  ),
).text()
const subscriptionWindowComponent = (() => {
  const { descriptor } = parse(componentSource, {
    filename: 'SubscriptionQuotaWindows.vue',
  })
  const script = compileScript(descriptor, {
    id: 'subscription-quota-windows',
    inlineTemplate: true,
  })
  const source = rewriteImports(script.content).replace(
    /export default/,
    'return',
  )
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(source)
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return vue
    if (spec === '@/composables/subscriptionQuotaWindows') {
      return subscriptionQuotaWindows
    }
    if (spec === '@/composables/useTokenPlanWindowEntries') {
      return tokenPlanWindowEntries
    }
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve)
})()

const progressStub = {
  name: 'UProgress',
  render(this: { $attrs: Record<string, unknown> }) {
    return h('progress', { ...this.$attrs, 'data-stub': 'UProgress' })
  },
}

const badgeStub = {
  name: 'UBadge',
  props: ['label'],
  render(this: { label?: string }) {
    return h('span', { 'data-stub': 'UBadge' }, this.label)
  },
}

test('unknown canonical usage shows a dash without an exhausted progress bar', async () => {
  const app = createSSRApp({
    render: () =>
      h(subscriptionWindowComponent as never, {
        nowMs: Date.now(),
        t: translate,
        windows: [
          windowUsage('primary', 18_000, 0),
          windowUsage('secondary', 604_800, null, 'unknown'),
        ],
      }),
  })
  app.component('UProgress', progressStub as never)
  app.component('UBadge', badgeStub as never)
  const html = await renderToString(app)
  expect(html).toContain('tokenPlanInterval')
  expect(html).toContain('tokenPlanWeekly')
  expect(html).toContain('0.0%')
  expect(html).toContain('tokenPlanUsageUnknown')
  expect(html).toContain('>-</span>')
  expect((html.match(/data-stub="UProgress"/g) ?? []).length).toBe(1)
})

test('reviewer: current cached relative reset expires from observed time, not render time', async () => {
  const app = createSSRApp({
    render: () =>
      h(subscriptionWindowComponent as never, {
        nowMs: Date.parse('2026-10-10T14:00:00Z'),
        observation: {
          observed_at: '2026-10-10T12:00:00Z',
          source: 'manual',
          stale: true,
          refreshing: false,
        },
        t: translate,
        windows: [
          {
            ...windowUsage('primary', 18_000, 50),
            reset_after_seconds: 3_600,
          },
        ],
      }),
  })
  app.component('UProgress', progressStub as never)
  app.component('UBadge', badgeStub as never)
  const html = await renderToString(app)
  expect(html).toContain('tokenPlanExpired')
})

test('observation metadata identifies cached values and refresh failures', async () => {
  const observation: SubscriptionQuotaObservation = {
    observed_at: '2026-10-10T12:00:00Z',
    source: 'manual',
    stale: true,
    last_error_code: 'upstream',
    next_retry_at: '2026-10-10T12:01:00Z',
    refreshing: false,
  }
  const app = createSSRApp({
    render: () =>
      h(subscriptionWindowComponent as never, {
        nowMs: Date.parse('2026-10-10T12:00:30Z'),
        observation,
        t: translate,
        windows: [windowUsage('primary', 18_000, 0)],
      }),
  })
  app.component('UProgress', progressStub as never)
  app.component('UBadge', badgeStub as never)
  const html = await renderToString(app)
  expect(html).toContain('quotaObservedAt')
  expect(html).toContain('quotaSourceManual')
  expect(html).toContain('quotaCachedObservation')
  expect(html).toContain('quotaRefreshFailedCached')
  expect(html).toContain('quotaRetryAt')
  expect(html).toContain('0.0%')
})
