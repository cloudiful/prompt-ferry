import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import * as adminMappers from '../src/admin-mappers'
import { createEmptyEndpointForm } from '../src/admin-mappers/forms/endpoint'
import { endpointMessages } from '../src/i18n/modules/endpoints'
import type { EndpointForm } from '../src/models'
import * as serviceTierHelpers from '../src/models/endpoints/service-tier'

// Issue #637: SSR harness for the endpoint service-tier control. The repo has
// no component-test setup, so the SFCs are compiled with the official compiler
// and rendered with stubbed children — the shape used by
// `endpoint-oauth.test.ts`. Kept adjacent to `endpoint-service-tier.test.ts`
// so the mapper cases stay readable.

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

const inputStub = {
  name: 'UInput',
  inheritAttrs: false,
  props: { modelValue: { type: String, default: '' } },
  setup(
    props: { modelValue: string },
    { attrs }: { attrs: Record<string, unknown> },
  ) {
    return () => h('input', { ...attrs, value: props.modelValue })
  },
}

// Issue #637: the free-text control must render the inherit placeholder.
const tierFieldComponent = compileComponent(
  await Bun.file(
    new URL(
      '../src/components/shared/ServiceTierOverrideField.vue',
      import.meta.url,
    ),
  ).text(),
  'ServiceTierOverrideField.vue',
  (spec) => {
    if (spec === 'vue') return vue
    if (spec === '@/admin-mappers') return adminMappers
    if (spec.endsWith('SettingsFieldRow.vue'))
      return slotStub('SettingsFieldRow')
    return slotStub(spec.split('/').pop() ?? spec)
  },
)

async function renderTierField(model: string | null): Promise<string> {
  const zh = endpointMessages['zh-CN'] as Record<string, string>
  const app = createSSRApp({
    render: () =>
      h(tierFieldComponent, {
        modelValue: model,
        t: (key: string) => zh[key] ?? key,
        inputId: 'endpoint-service-tier',
      }),
  })
  app.component('UInput', inputStub)
  app.component('UTooltip', slotStub('UTooltip'))
  app.component('UButton', slotStub('UButton'))
  return renderToString(app)
}

test('the service tier input renders the inherit placeholder', async () => {
  const html = await renderTierField(null)
  expect(html).toContain('继承')
  expect(html).toContain('endpoint-service-tier')
})

test('the service tier input carries a configured value', async () => {
  const html = await renderTierField('priority')
  expect(html).toContain('value="priority"')
})

const tierMarker = 'data-stub="ServiceTierOverrideField.vue"'

const providerFieldsComponent = compileComponent(
  await Bun.file(
    new URL(
      '../src/components/endpoints/EndpointProviderFields.vue',
      import.meta.url,
    ),
  ).text(),
  'EndpointProviderFields.vue',
  (spec) => {
    if (spec === 'vue') return vue
    if (spec === '@/admin-mappers') return adminMappers
    if (spec === '@/models/endpoints/service-tier') return serviceTierHelpers
    if (spec.endsWith('SettingsFieldRow.vue'))
      return slotStub('SettingsFieldRow')
    if (spec.endsWith('ServiceTierOverrideField.vue'))
      return slotStub('ServiceTierOverrideField.vue')
    return slotStub(spec.split('/').pop() ?? spec)
  },
)

async function renderProviderFields(
  overrides: Partial<EndpointForm>,
): Promise<string> {
  const form = { ...createEmptyEndpointForm(), ...overrides }
  const app = createSSRApp({
    render: () => h(providerFieldsComponent, { form, t: (key: string) => key }),
  })
  app.component('USelect', slotStub('USelect'))
  app.component('UInput', slotStub('UInput'))
  app.component('UTooltip', slotStub('UTooltip'))
  app.component('UButton', slotStub('UButton'))
  return renderToString(app)
}

test('the endpoint tier field follows the documented provider/protocol matrix', async () => {
  const eligible = [
    { provider: 'minimax' as const, native_api_override: 'chat' as const },
    {
      provider: 'minimax' as const,
      native_api_override: 'anthropic_messages' as const,
    },
    { provider: 'openai' as const, native_api_override: 'responses' as const },
  ]
  for (const overrides of eligible) {
    const html = await renderProviderFields({
      protocol_mode: 'manual',
      ...overrides,
    })
    expect(html).toContain(tierMarker)
  }
  // Auto resolves to the caller protocol, so a supported provider stays eligible.
  expect(
    await renderProviderFields({ provider: 'openai', protocol_mode: 'auto' }),
  ).toContain(tierMarker)

  const hidden = [
    { provider: 'minimax' as const, native_api_override: 'realtime' as const },
    {
      provider: 'openai' as const,
      native_api_override: 'anthropic_messages' as const,
    },
    { provider: 'generic' as const, native_api_override: null },
  ]
  for (const overrides of hidden) {
    const html = await renderProviderFields({
      protocol_mode: 'manual',
      ...overrides,
    })
    expect(html).not.toContain(tierMarker)
  }
})
