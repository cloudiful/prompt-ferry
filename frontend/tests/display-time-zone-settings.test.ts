import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h, nextTick } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'

import { messages } from '../src/i18n'
import {
  AUTO_DISPLAY_TIME_ZONE,
  displayTimeZone,
  setDisplayTimeZone,
} from '../src/format/timezone'

const source = (relative: string) =>
  Bun.file(new URL(relative, import.meta.url)).text()

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

const timezone = await import('../src/format/timezone')

function translate(key: string, params?: Record<string, unknown>): string {
  const template =
    (messages['zh-CN'] as Record<string, string | undefined>)[key] ?? key
  return template.replace(/\{(\w+)\}/g, (_match, name: string) =>
    String(params?.[name] ?? ''),
  )
}

/**
 * Stand-in for `USelectMenu`, the Nuxt UI component whose built-in search
 * input makes the IANA list usable. It declares only props the installed
 * component has, so an invented prop would surface as an attribute instead of
 * silently passing.
 */
const selectMenuStub = {
  name: 'USelectMenu',
  props: {
    items: { type: Array, default: () => [] },
    labelKey: { type: String, default: 'label' },
    modelValue: { type: String, default: null },
    searchInput: { type: [Boolean, Object], default: true },
    valueKey: { type: String, default: 'value' },
  },
  render(this: {
    items: Array<Record<string, unknown>>
    labelKey: string
    modelValue: string | null
    searchInput: boolean | Record<string, unknown>
  }) {
    return h(
      'div',
      {
        'data-stub': 'USelectMenu',
        'data-search-input': this.searchInput === false ? 'false' : 'true',
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

const generalTab = await (async () => {
  const componentSource = await source(
    '../src/components/settings/SettingsGeneralTab.vue',
  )
  const { descriptor } = parse(componentSource, {
    filename: 'SettingsGeneralTab.vue',
  })
  const script = compileScript(descriptor, {
    id: 'settings-general-tab',
    inlineTemplate: true,
  })
  const rewritten = rewriteSfcImports(script.content).replace(
    /export default/,
    'return',
  )
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(rewritten)
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return vue
    if (spec === '@/i18n') return { localeOptions: [] }
    if (spec === '@/format/timezone') return timezone
    if (spec === './SettingsCard.vue') return settingsCardStub
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve)
})()
async function render(): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(generalTab as never, {
        locale: 'zh-CN',
        themeMode: 'dark',
        displayTimeZone: displayTimeZone.value,
        onSetDisplayTimeZone: (zone: string) => setDisplayTimeZone(zone),
        t: translate,
      }),
  })
  app.component('USelect', slotStub('USelect'))
  app.component('USelectMenu', selectMenuStub)
  app.component('UIcon', slotStub('UIcon'))
  return renderToString(app)
}

test('the general tab offers the display time zone with its one consequence line', async () => {
  const html = await render()
  const text = html.replace(/<[^>]*>/g, ' ')

  expect(text).toContain('显示时区')
  expect(text).toContain('仅影响时间显示，不改变统计周期。')
  expect(html).toContain('aria-label="显示时区"')
  expect(html).toContain('id="settings-display-time-zone"')
  expect(text).not.toContain('统计周期和日期范围')
})

test('the zone selector is the searchable component, not a stub-only prop', async () => {
  const html = await render()

  expect(html).toMatch(
    /<div data-stub="USelectMenu" data-search-input="true"[^>]*id="settings-display-time-zone"/,
  )
  expect(html).not.toContain('searchable')
  for (const label of [
    `自动（${timezone.browserTimeZone()}）`,
    'UTC',
    'Asia/Shanghai',
  ]) {
    expect(html).toContain(`data-option="${label}"`)
  }
  expect(html).toContain('data-value="auto"')
})

test('the selector shows the saved zone', async () => {
  const previous = displayTimeZone.value
  try {
    setDisplayTimeZone('Asia/Shanghai')
    const html = await render()
    expect(html).toContain('data-value="Asia/Shanghai"')
  } finally {
    setDisplayTimeZone(previous)
  }
})

test('the settings page binds the zone model and the change event', async () => {
  const page = await source('../src/pages/SettingsPage.vue')

  expect(page).toContain('v-model:display-time-zone="displayTimeZone"')
  expect(page).toContain('@set-display-time-zone="setDisplayTimeZone"')
  expect(page).toContain('useDisplayTimeZone()')
})

test('selecting a zone updates the shared preference and survives a reload', async () => {
  const previous = displayTimeZone.value
  const storage = new Map<string, string>()
  const original = Object.getOwnPropertyDescriptor(globalThis, 'localStorage')
  Object.defineProperty(globalThis, 'localStorage', {
    configurable: true,
    value: {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => {
        storage.set(key, value)
      },
    },
  })
  try {
    setDisplayTimeZone(AUTO_DISPLAY_TIME_ZONE)
    const html = await render()
    expect(html).toContain('settings-display-time-zone')

    setDisplayTimeZone('Asia/Shanghai')
    await nextTick()
    expect(displayTimeZone.value).toBe('Asia/Shanghai')
    expect(storage.get('prompt-ferry:display-time-zone')).toBe('Asia/Shanghai')
    expect(timezone.resolveDisplayTimeZone('Asia/Shanghai')).toBe(
      'Asia/Shanghai',
    )
  } finally {
    if (original) Object.defineProperty(globalThis, 'localStorage', original)
    setDisplayTimeZone(previous)
  }
})
