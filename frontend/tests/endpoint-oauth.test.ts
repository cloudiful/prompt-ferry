import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import * as adminMappers from '../src/admin-mappers'
import {
  createEmptyEndpointForm,
  endpointFormToRequest,
  endpointToForm,
  normalizeEndpointPlan,
  normalizeProviderPlan,
} from '../src/admin-mappers/forms/endpoint'
import type { ProviderEndpoint } from '../src/generated/admin-api'
import { endpointMessages } from '../src/i18n/modules/endpoints'
import type { EndpointForm } from '../src/models'

function endpointFixture(
  overrides: Partial<ProviderEndpoint> = {},
): ProviderEndpoint {
  return {
    api_keys: [],
    base_url: 'https://api.openai.com',
    created_at: '2026-09-26T00:00:00Z',
    enabled: true,
    endpoint_id: 'endpoint-1',
    key_lb_enabled: false,
    mcp_enabled: false,
    name: 'openai',
    native_api: 'responses',
    native_api_source: 'manual',
    provider: 'openai',
    scope: 'admin',
    updated_at: '2026-09-26T00:00:00Z',
    ...overrides,
  }
}

test('new endpoint forms start on the platform plan without a token', () => {
  const form = createEmptyEndpointForm()
  expect(form.plan).toBe('platform_api_key')
  expect(form.has_oauth_token).toBe(false)
})

test('normalizeEndpointPlan keeps subscription and falls back to platform', () => {
  expect(normalizeEndpointPlan('chatgpt_subscription')).toBe(
    'chatgpt_subscription',
  )
  expect(normalizeEndpointPlan('platform_api_key')).toBe('platform_api_key')
  expect(normalizeEndpointPlan(undefined)).toBe('platform_api_key')
  expect(normalizeEndpointPlan(null)).toBe('platform_api_key')
  expect(normalizeEndpointPlan('legacy-unknown')).toBe('platform_api_key')
})

test('normalizeProviderPlan forces the platform plan off OpenAI', () => {
  expect(normalizeProviderPlan('openai', 'chatgpt_subscription')).toBe(
    'chatgpt_subscription',
  )
  for (const provider of [
    'generic',
    'minimax',
    'command_code',
    'opencode_go',
    'openrouter',
    'glm',
    'deepseek',
  ] as const) {
    expect(normalizeProviderPlan(provider, 'chatgpt_subscription')).toBe(
      'platform_api_key',
    )
  }
})

test('endpointToForm carries the derived plan and token presence', () => {
  const subscribed = endpointToForm(
    endpointFixture({
      has_oauth_token: true,
      plan: 'chatgpt_subscription',
    }),
  )
  expect(subscribed.plan).toBe('chatgpt_subscription')
  expect(subscribed.has_oauth_token).toBe(true)

  const platform = endpointToForm(endpointFixture())
  expect(platform.plan).toBe('platform_api_key')
  expect(platform.has_oauth_token).toBe(false)

  // A stale plan on a non-OpenAI endpoint never survives the mapper.
  const legacy = endpointToForm(
    endpointFixture({ plan: 'chatgpt_subscription', provider: 'generic' }),
  )
  expect(legacy.plan).toBe('platform_api_key')
})

test('endpointFormToRequest sends the effective plan', () => {
  const subscribed = {
    ...createEmptyEndpointForm(),
    provider: 'openai' as const,
    plan: 'chatgpt_subscription' as const,
    has_oauth_token: true,
  }
  expect(endpointFormToRequest(subscribed).plan).toBe('chatgpt_subscription')
  expect(endpointFormToRequest(createEmptyEndpointForm()).plan).toBe(
    'platform_api_key',
  )
  const switched = {
    ...subscribed,
    provider: 'generic' as const,
  }
  expect(endpointFormToRequest(switched).plan).toBe('platform_api_key')
})

type Component = Parameters<typeof h>[0]

// Issue #599 R2e.1: the repo has no component-test setup, so the dialog SFC
// is compiled with the official compiler and rendered with stubbed children.
const dialogSource = await Bun.file(
  new URL('../src/components/endpoints/EndpointDialog.vue', import.meta.url),
).text()

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
        if (!entry) continue
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

const apiKeysStub = {
  name: 'EndpointApiKeysEditor',
  render: () => h('div', { 'data-testid': 'api-keys' }),
}

const dialogComponent = ((): Component => {
  const { descriptor } = parse(dialogSource, { filename: 'EndpointDialog.vue' })
  const script = compileScript(descriptor, {
    id: 'endpoint-dialog',
    inlineTemplate: true,
  })
  const source = rewriteSfcImports(script.content).replace(
    /export default/,
    'return',
  )
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(source)
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return vue
    if (spec === '@/admin-mappers') return adminMappers
    if (spec.endsWith('EndpointApiKeysEditor.vue')) return apiKeysStub
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve) as Component
})()

async function renderDialog(form: EndpointForm): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(dialogComponent, {
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
  app.component('UButton', slotStub('UButton'))
  app.component('UTooltip', slotStub('UTooltip'))
  app.component('UIcon', slotStub('UIcon'))
  return renderToString(app)
}

function subscriptionForm(): EndpointForm {
  return {
    ...createEmptyEndpointForm(),
    endpoint_id: 'endpoint-1',
    name: 'openai',
    provider: 'openai',
    base_url: 'https://api.openai.com',
    plan: 'chatgpt_subscription',
    has_oauth_token: true,
    api_keys: [
      {
        key_label: 'default',
        api_key: '',
        has_saved_key: true,
        enabled: true,
        key_id: 'key-1',
      },
    ],
  }
}

const API_KEYS_MARKER = 'data-testid="api-keys"'

test('the ChatGPT subscription plan hides the API-key editor', async () => {
  const html = await renderDialog(subscriptionForm())
  expect(html).not.toContain(API_KEYS_MARKER)
})

test('the platform plan shows the API-key editor', async () => {
  const html = await renderDialog({
    ...subscriptionForm(),
    plan: 'platform_api_key',
  })
  expect(html).toContain(API_KEYS_MARKER)
})

test('a stale subscription plan off OpenAI keeps the API-key editor', async () => {
  const html = await renderDialog({
    ...subscriptionForm(),
    provider: 'generic',
    plan: 'chatgpt_subscription',
  })
  expect(html).toContain(API_KEYS_MARKER)
})

test('saved endpoints keep the ChatGPT login section mounted', async () => {
  const html = await renderDialog(subscriptionForm())
  expect(html).toContain('data-stub="EndpointOAuthSection.vue"')
  expect(html).not.toContain('endpointOAuthSaveFirst')
})

test('new endpoints keep the save-first login guidance', async () => {
  const html = await renderDialog({
    ...createEmptyEndpointForm(),
    name: 'openai',
    provider: 'openai',
    base_url: 'https://api.openai.com',
  })
  expect(html).toContain('endpointOAuthSaveFirst')
  expect(html).not.toContain('data-stub="EndpointOAuthSection.vue"')
  expect(html).toContain(API_KEYS_MARKER)
})

test('ChatGPT OAuth copy exists in both locales', () => {
  const keys = [
    'endpointPlan',
    'endpointPlanHint',
    'endpointPlanPlatformApiKey',
    'endpointPlanChatgptSubscription',
    'endpointPlanLoginRequired',
    'endpointOAuth',
    'endpointOAuthSaveFirst',
    'endpointOAuthNotLoggedIn',
    'endpointOAuthLoggedIn',
    'endpointOAuthExpired',
    'endpointOAuthExpiresAt',
    'endpointOAuthRefresh',
    'endpointOAuthClear',
    'endpointOAuthQuota',
    'endpointOAuthQuotaHint',
    'endpointOAuthQuotaPrimary',
    'endpointOAuthQuotaSecondary',
    'endpointOAuthQuotaUnavailable',
    'endpointOAuthDeviceLogin',
    'endpointOAuthDeviceStart',
    'endpointOAuthDeviceHint',
    'endpointOAuthDeviceWaiting',
    'endpointOAuthBrowserLogin',
    'endpointOAuthBrowserStart',
    'endpointOAuthBrowserHint',
    'endpointOAuthAuthorizeUrl',
    'endpointOAuthRedirectUrl',
    'endpointOAuthRedirectPlaceholder',
    'endpointOAuthComplete',
    'endpointOAuthCopy',
  ] as const
  for (const locale of ['zh-CN', 'en-US'] as const) {
    const messages = endpointMessages[locale] as Record<string, string>
    for (const key of keys) {
      expect(messages[key]?.length ?? 0).toBeGreaterThan(0)
    }
  }
})
