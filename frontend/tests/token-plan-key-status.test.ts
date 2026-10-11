import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import type { TokenPlanQuotaStatus } from '../src/generated/admin-api'

const source = await Bun.file(
  new URL('../src/components/endpoints/TokenPlanKeyQuotaStatus.vue', import.meta.url),
).text()
const { descriptor } = parse(source, { filename: 'TokenPlanKeyQuotaStatus.vue' })
const script = compileScript(descriptor, { id: 'token-plan-key-status', inlineTemplate: true })

function rewriteImports(code: string): string {
  return code.replace(
    /import\s+([^;'\"]+?)\s+from\s+(['\"])([^'\"]+)\2;?/g,
    (_match, clause: string, _quote: string, specifier: string) => {
      if (clause.trim().startsWith('type ')) return ''
      const named = clause.match(/\{([^}]*)\}/)
      const statements: string[] = []
      for (const part of named?.[1].split(',') ?? []) {
        const entry = part.trim()
        if (!entry || entry.startsWith('type ')) continue
        const [sourceName, localName = sourceName] = entry.split(/\s+as\s+/)
        statements.push(
          `const ${localName.trim()} = __resolve(${JSON.stringify(specifier)})[${JSON.stringify(sourceName.trim())}];`,
        )
      }
      return statements.join(' ')
    },
  ).replace('export default', 'return')
}

const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(
  rewriteImports(script.content),
)
const component = new Function('__resolve', js)((specifier: string) => {
  if (specifier === 'vue') return vue
  throw new Error(`Unexpected import ${specifier}`)
}) as object
const badge = {
  props: ['label', 'color', 'title'],
  render(this: { label: string; color: string; title: string }) {
    return h('span', { 'data-color': this.color, title: this.title }, this.label)
  },
}

async function render(status: TokenPlanQuotaStatus | null): Promise<string> {
  const app = createSSRApp({
    render: () => h(component as never, {
      status,
      t: (key: string, values?: Record<string, string>) =>
        values ? `${key}:${values.time}` : key,
    }),
  })
  app.component('UBadge', badge as never)
  return renderToString(app)
}

test('per-key labels distinguish available, exhausted, unknown and cooling', async () => {
  const cases = [
    ['available', 'tokenPlanAvailable', 'success'],
    ['exhausted', 'tokenPlanUnavailable', 'error'],
    ['unknown', 'tokenPlanQuotaUnknown', 'neutral'],
    ['cooling', 'tokenPlanQuotaCooling', 'neutral'],
  ] as const
  for (const [state, label, color] of cases) {
    const html = await render({ state, scope: 'text_inference' })
    expect(html).toContain(label)
    expect(html).toContain(`data-color="${color}"`)
    expect(html).toContain('tokenPlanDefaultTextScope')
  }
})

test('status tooltip includes observation time, refresh failure and cooldown', async () => {
  const html = await render({
    state: 'cooling',
    scope: 'text_inference',
    observed_at: '2026-10-11T10:00:00Z',
    cooldown_until: '2026-10-11T10:01:00Z',
    refresh_failed: true,
  })
  expect(html).toContain('quotaObservedAt:')
  expect(html).toContain('quotaRefreshFailedCached')
  expect(html).toContain('quotaRetryAt:')
})
