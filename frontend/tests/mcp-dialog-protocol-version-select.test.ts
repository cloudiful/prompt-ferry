import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'

import { createEmptyMcpForm } from '../src/admin-mappers/forms/mcp'
import type { McpForm } from '../src/models'

/**
 * Issue #950 P8: the protocol-version select used to carry an item with an
 * empty value for the "automatic" choice, which Reka UI rejects when the
 * create/edit MCP dialog mounts. The regression harness compiles and mounts the
 * real SFC (no source grep) with a capturing select, so the assertions cover
 * the actual items and the two-way mapping of the persisted form value.
 */

type Component = Parameters<typeof h>[0]
type SelectItem = string | { label: string; value: string }
type SelectProps = {
  modelValue: unknown
  items: SelectItem[]
}
type CapturedSelect = SelectProps & {
  update?: (value: string) => void
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

function slotStub(name: string, slot = 'default') {
  return {
    name,
    render(this: { $slots: Record<string, (() => unknown) | undefined> }) {
      return h('div', { 'data-stub': name }, this.$slots[slot]?.())
    },
  }
}

const dialogSource = await Bun.file(
  new URL('../src/components/mcp/McpDialog.vue', import.meta.url),
).text()

const component: Component = (() => {
  const { descriptor } = parse(dialogSource, {
    filename: 'McpDialog.vue',
  })
  const script = compileScript(descriptor, {
    id: 'mcp-dialog',
    inlineTemplate: true,
  })
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    rewriteSfcImports(script.content).replace(/export default/, 'return'),
  )
  return new Function('__resolve', `"use strict";\n${js}`)((spec: string) => {
    if (spec === 'vue') return vue
    return slotStub(spec.split('/').pop() ?? spec)
  }) as Component
})()

const CATALOG = { tools: [], resources: [], prompts: [] }

async function mount(form: McpForm): Promise<{
  html: string
  selects: CapturedSelect[]
}> {
  const selects: CapturedSelect[] = []
  const capturingSelect = {
    name: 'USelect',
    props: ['modelValue', 'items'],
    inheritAttrs: false,
    setup(props: SelectProps, { attrs }: { attrs: Record<string, unknown> }) {
      selects.push({
        modelValue: props.modelValue,
        items: props.items,
        update: attrs['onUpdate:modelValue'] as
          ((value: string) => void) | undefined,
      })
      return () => h('div', { 'data-select': 'USelect' })
    },
  }
  const app = createSSRApp({
    render: () =>
      h(component as never, {
        busy: false,
        catalogLoading: false,
        header: 'MCP',
        isAdmin: true,
        t: (key: string) => key,
        catalog: CATALOG,
        users: [],
        providers: [{ id: 'generic', display_name: 'Generic', auth: 'none' }],
        form,
        visible: true,
      }),
  })
  app.component('UModal', slotStub('UModal', 'body'))
  app.component('USelect', capturingSelect)
  app.component('USwitch', slotStub('USwitch'))
  app.component('UInput', slotStub('UInput'))
  app.component('UButton', slotStub('UButton'))
  app.component('UFormField', slotStub('UFormField'))
  app.component('UBadge', slotStub('UBadge'))
  app.component('UTooltip', slotStub('UTooltip'))
  app.component('UIcon', slotStub('UIcon'))
  app.component('UCheckbox', slotStub('UCheckbox'))
  app.component('USelectMenu', slotStub('USelectMenu'))
  const html = await renderToString(app)
  return { html, selects }
}

/** The protocol-version select is the one carrying version-like items. */
function protocolSelect(selects: CapturedSelect[]): CapturedSelect {
  const found = selects.find((select) =>
    (select.items ?? []).some((item) =>
      String(typeof item === 'string' ? item : item.value).startsWith('20'),
    ),
  )
  if (!found) throw new Error('protocol version select was not rendered')
  return found
}

const itemValues = (select: CapturedSelect) =>
  select.items.map((item) => (typeof item === 'string' ? item : item.value))

test('the create dialog mounts with no empty select item', async () => {
  const { selects } = await mount(createEmptyMcpForm())

  expect(selects.length).toBeGreaterThan(0)
  for (const select of selects) {
    for (const value of itemValues(select)) {
      expect(value).not.toBe('')
    }
  }
})

test('automatic is a non-empty sentinel and persists as null', async () => {
  const form = createEmptyMcpForm()
  const first = protocolSelect((await mount(form)).selects)

  expect(first.modelValue).toBe('auto')
  expect(itemValues(first)).toContain('auto')
  expect(itemValues(first)).not.toContain('')

  first.update?.('auto')
  expect(form.lifecycle_manual_protocol_version).toBeNull()
})

test('an explicit protocol version round-trips unchanged', async () => {
  const form = {
    ...createEmptyMcpForm(),
    lifecycle_manual_protocol_version: '2025-06-18',
  }
  const select = protocolSelect((await mount(form)).selects)

  expect(select.modelValue).toBe('2025-06-18')
  expect(itemValues(select)).toContain('2025-06-18')

  select.update?.('2025-11-25')
  expect(form.lifecycle_manual_protocol_version).toBe('2025-11-25')

  select.update?.('auto')
  expect(form.lifecycle_manual_protocol_version).toBeNull()
})
