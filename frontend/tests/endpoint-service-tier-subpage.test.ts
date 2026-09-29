import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import * as adminMappers from '../src/admin-mappers'
import { createEmptyEndpointForm } from '../src/admin-mappers/forms/endpoint'
import type { EndpointForm } from '../src/models'
import * as serviceTierHelpers from '../src/models/endpoints/service-tier'

// Issue #644: SSR harness for the endpoint Settings subpage. The free-form
// service-tier control moved out of `EndpointProviderFields` into the dialog's
// settings card, which is one of the dialog's two internal views; SSR cannot
// click the gear, so the compiled source pins the initial view to `settings`
// for this harness only (a failed pin throws rather than silently passing).

type Component = Parameters<typeof h>[0]

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

function compileComponent(
  source: string,
  filename: string,
  resolve: (spec: string) => unknown,
): Component {
  const { descriptor } = parse(source, { filename })
  const script = compileScript(descriptor, {
    id: filename,
    inlineTemplate: true,
  })
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    rewriteSfcImports(script.content).replace(/export default/, 'return'),
  )
  return new Function('__resolve', `"use strict";\n${js}`)(resolve) as Component
}

function slotStub(name: string, slot = 'default') {
  return {
    name,
    render(this: { $slots: Record<string, (() => unknown) | undefined> }) {
      return h('div', { 'data-stub': name }, this.$slots[slot]?.())
    },
  }
}

// The gear's `aria-pressed` highlight is only observable when the stub forwards
// attrs, unlike the plain `slotStub`.
const buttonStub = {
  name: 'UButton',
  inheritAttrs: false,
  setup(_props: unknown, { attrs }: { attrs: Record<string, unknown> }) {
    return () => h('button', { ...attrs })
  },
}

const inputStub = {
  name: 'UInput',
  inheritAttrs: false,
  setup(_props: unknown, { attrs }: { attrs: Record<string, unknown> }) {
    return () => h('input', { ...attrs })
  },
}

const dialogSource = await Bun.file(
  new URL('../src/components/endpoints/EndpointDialog.vue', import.meta.url),
).text()

const VIEW_REF_RE = /ref<'main' \| 'settings'>\(\s*'main',?\s*\)/

function compileDialog(pinSettings: boolean): Component {
  const source = pinSettings
    ? dialogSource.replace(VIEW_REF_RE, "ref<'main' | 'settings'>('settings')")
    : dialogSource
  if (pinSettings && source === dialogSource) {
    throw new Error('could not pin EndpointDialog to the settings view')
  }
  return compileComponent(source, 'EndpointDialog.vue', (spec) => {
    if (spec === 'vue') return vue
    if (spec === '@/admin-mappers') return adminMappers
    if (spec === '@/models/endpoints/service-tier') return serviceTierHelpers
    return slotStub(spec.split('/').pop() ?? spec)
  })
}

interface DialogComponents {
  main: Component
  settings: Component
}

let components: DialogComponents | undefined

function dialogComponents(): DialogComponents {
  components ??= {
    main: compileDialog(false),
    settings: compileDialog(true),
  }
  return components
}

async function renderDialog(
  form: EndpointForm,
  view: 'main' | 'settings',
): Promise<string> {
  const component = dialogComponents()[view]
  const app = createSSRApp({
    render: () =>
      h(component, {
        form,
        visible: true,
        busy: false,
        header: 'endpoint',
        t: (key: string) => key,
        users: [],
      }),
  })
  app.component('UModal', slotStub('UModal', 'body'))
  app.component('USelect', slotStub('USelect'))
  app.component('USwitch', slotStub('USwitch'))
  app.component('UButton', buttonStub)
  app.component('UTooltip', slotStub('UTooltip'))
  app.component('UIcon', slotStub('UIcon'))
  app.component('UInput', inputStub)
  return renderToString(app)
}

const TIER_MARKER = 'data-stub="ServiceTierOverrideField.vue"'

const PROVIDERS = [
  'generic',
  'minimax',
  'command_code',
  'opencode_go',
  'openrouter',
  'glm',
  'deepseek',
  'openai',
] as const

test('the endpoint settings subpage hosts the tier control for every provider on JSON protocols', async () => {
  for (const provider of PROVIDERS) {
    for (const native_api_override of [
      'chat',
      'responses',
      'anthropic_messages',
    ] as const) {
      const html = await renderDialog(
        {
          ...createEmptyEndpointForm(),
          provider,
          protocol_mode: 'manual',
          native_api_override,
        },
        'settings',
      )
      expect(html).toContain(TIER_MARKER)
    }
    // Auto resolves to a JSON protocol at request time, so it stays eligible.
    const auto = await renderDialog(
      { ...createEmptyEndpointForm(), provider, protocol_mode: 'auto' },
      'settings',
    )
    expect(auto).toContain(TIER_MARKER)
  }
})

test('the endpoint settings subpage hides the tier control on Realtime', async () => {
  const html = await renderDialog(
    {
      ...createEmptyEndpointForm(),
      provider: 'minimax',
      protocol_mode: 'manual',
      native_api_override: 'realtime',
    },
    'settings',
  )
  expect(html).not.toContain(TIER_MARKER)
})

test('the main view keeps the tier control out of the provider fields', async () => {
  const html = await renderDialog(
    {
      ...createEmptyEndpointForm(),
      provider: 'openai',
      protocol_mode: 'manual',
      native_api_override: 'responses',
    },
    'main',
  )
  expect(html).not.toContain(TIER_MARKER)
})

test('a configured endpoint tier highlights the endpoint settings gear', async () => {
  const plain = await renderDialog(createEmptyEndpointForm(), 'main')
  expect(plain).toContain('aria-pressed="false"')

  const configured = await renderDialog(
    {
      ...createEmptyEndpointForm(),
      provider: 'generic',
      service_tier: 'priority',
    },
    'main',
  )
  expect(configured).toContain('aria-pressed="true"')
})
