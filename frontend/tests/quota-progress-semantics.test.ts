import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, defineComponent, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import type { CanonicalQuotaRow } from '../src/composables/useCanonicalQuotaWindows'

const rowSource = await Bun.file(
  new URL(
    '../src/components/endpoints/TokenPlanQuotaWindowRow.vue',
    import.meta.url,
  ),
).text()

const canonicalQuota =
  await import('../src/composables/useCanonicalQuotaWindows')
const windowEntries =
  await import('../src/composables/useTokenPlanWindowEntries')

type CapturedProgress = {
  modelValue: unknown
  style: Record<string, string> | undefined
}

const progress: CapturedProgress[] = []
const progressStub = defineComponent({
  name: 'UProgress',
  props: { modelValue: { type: Number, default: null } },
  setup(props, { attrs }) {
    return () => {
      progress.push({
        modelValue: props.modelValue,
        style: attrs.style as Record<string, string> | undefined,
      })
      return h('div', { 'data-stub': 'UProgress' })
    }
  },
})

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

const rowComponent = (() => {
  const { descriptor } = parse(rowSource, {
    filename: 'TokenPlanQuotaWindowRow.vue',
  })
  const script = compileScript(descriptor, {
    id: 'quota-progress-semantics',
    inlineTemplate: true,
  })
  const source = rewriteSfcImports(script.content).replace(
    /export default/,
    'return',
  )
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(source)
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return vue
    if (spec === '@/composables/useCanonicalQuotaWindows') {
      return canonicalQuota
    }
    if (spec === '@/composables/useTokenPlanWindowEntries') {
      return windowEntries
    }
    return { name: spec }
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve)
})()

function row(overrides: Partial<CanonicalQuotaRow> = {}): CanonicalQuotaRow {
  return {
    sourceId: 'glm:five_hour',
    kind: 'five_hour',
    sourceModel: null,
    unit: null,
    durationSeconds: null,
    isDefaultScope: true,
    availability: 'known',
    observedRemainingPercent: 99,
    remainingPercent: 99,
    current: null,
    limit: null,
    remaining: null,
    resetAt: null,
    expired: false,
    ...overrides,
  }
}

function t(key: string): string {
  return key === 'quotaRemainingPercent' ? 'Remaining' : key
}

async function renderRow(
  quotaRow: CanonicalQuotaRow,
  density: 'compact' | 'full' = 'compact',
): Promise<string> {
  progress.length = 0
  const app = createSSRApp({
    render: () =>
      h(rowComponent as never, {
        row: quotaRow,
        label: 'Window',
        t,
        nowMs: Date.parse('2026-10-16T00:00:00Z'),
        density,
      }),
  })
  app.component('UProgress', progressStub as never)
  return renderToString(app)
}

test('the percent bar fills with remaining quota, not consumed quota', async () => {
  const html = await renderRow(row({ remainingPercent: 99 }))
  expect(progress).toHaveLength(1)
  expect(progress[0]?.modelValue).toBe(99)
  expect(html).toContain('Remaining 99.0%')
})

test('remaining 22 keeps a short-window risk color and the same remaining text', async () => {
  const html = await renderRow(row({ remainingPercent: 22 }))
  expect(progress[0]?.modelValue).toBe(22)
  expect(
    progress[0]?.style?.['--token-plan-progress-color']?.startsWith('hsl(26.4'),
  ).toBe(true)
  expect(html).toContain('Remaining 22.0%')
})

test('remaining 99 is long/green and remaining 0 is empty/error', async () => {
  await renderRow(row({ remainingPercent: 99 }))
  expect(
    progress[0]?.style?.['--token-plan-progress-color']?.startsWith(
      'hsl(118.8',
    ),
  ).toBe(true)

  const empty = await renderRow(row({ remainingPercent: 0 }))
  expect(progress[0]?.modelValue).toBe(0)
  expect(progress[0]?.style?.['--token-plan-progress-color']).toBe(
    'hsl(0 80% 45%)',
  )
  expect(empty).toContain('Remaining 0.0%')
})

test('full density keeps the same remaining bar and label', async () => {
  const html = await renderRow(row({ remainingPercent: 42 }), 'full')
  expect(progress[0]?.modelValue).toBe(42)
  expect(html).toContain('Remaining 42.0%')
})

test('unknown and balance rows stay honest without a synthetic bar', async () => {
  const unknown = await renderRow(
    row({
      availability: 'unknown',
      observedRemainingPercent: null,
      remainingPercent: null,
    }),
  )
  expect(progress).toHaveLength(0)
  expect(unknown).toContain('—')

  const balance = await renderRow(
    row({
      kind: 'balance',
      unit: 'CNY',
      observedRemainingPercent: null,
      remainingPercent: null,
      current: 5,
    }),
  )
  expect(progress).toHaveLength(0)
  expect(balance).toContain('5.00')
})
