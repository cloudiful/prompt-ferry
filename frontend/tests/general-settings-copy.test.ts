import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'

import { messages } from '../src/i18n'

const componentSource = await Bun.file(
  new URL('../src/components/settings/SettingsGeneralTab.vue', import.meta.url),
).text()

type Slots = Record<string, (() => unknown) | undefined>

function slotStub(name: string) {
  return {
    name,
    render(this: { $slots: Slots }) {
      return h('div', { 'data-stub': name }, this.$slots.default?.())
    },
  }
}

const settingsCardStub = {
  name: 'SettingsCard',
  render(this: { $slots: Slots }) {
    return h('section', [this.$slots.header?.(), this.$slots.default?.()])
  },
}

function rewriteSfcImports(code: string): string {
  return code.replace(
    /import\s+([^;'"]+?)\s+from\s+(['"])([^'"]+)\2;?/g,
    (_match, clause: string, _quote: string, spec: string) => {
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

const component = (() => {
  const { descriptor } = parse(componentSource, {
    filename: 'SettingsGeneralTab.vue',
  })
  const script = compileScript(descriptor, {
    id: 'settings-general-tab',
    inlineTemplate: true,
  })
  const source = rewriteSfcImports(script.content).replace(
    /export default/,
    'return',
  )
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(source)
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return vue
    if (spec === '@/i18n') return { localeOptions: [] }
    if (spec === './SettingsCard.vue') return settingsCardStub
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve)
})()

async function render(): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(component as never, {
        locale: 'zh-CN',
        themeMode: 'dark',
        t: (key: string) => (messages['zh-CN'] as Record<string, string>)[key],
      }),
  })
  app.component('USelect', slotStub('USelect'))
  app.component('UIcon', slotStub('UIcon'))
  return renderToString(app)
}

const visibleText = (html: string) => html.replace(/<[^>]*>/g, ' ')

test('language and theme cards keep one visible label each', async () => {
  const html = await render()
  const text = visibleText(html)

  expect(text.split('语言').length - 1).toBe(1)
  expect(text.split('主题').length - 1).toBe(1)
  expect(html).toContain('aria-label="语言"')
  expect(html).toContain('aria-label="主题"')
})
