import { expect, test } from 'bun:test'
import * as vue from 'vue'
import { createSSRApp, h } from 'vue'
import { compileScript, parse } from 'vue/compiler-sfc'
import { renderToString } from 'vue/server-renderer'
import * as adminMappers from '../src/admin-mappers'
import * as endpointDialogValidation from '../src/composables/useEndpointDialogValidation'
import {
  createEmptyEndpointForm,
  endpointFormToRequest,
  endpointToForm,
  normalizeEndpointPlan,
  normalizeProviderPlan,
} from '../src/admin-mappers/forms/endpoint'
import type { ProviderEndpoint } from '../src/generated/admin-api'
import { endpointOpenAiMessages } from '../src/i18n/modules/endpoints.openai'
import { messages } from '../src/i18n'
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

/** A button that keeps the real disabled and loading state. */
function buttonStub() {
  return {
    name: 'UButton',
    inheritAttrs: false,
    props: ['disabled', 'loading', 'label', 'type', 'size', 'color', 'variant'],
    setup(
      props: Record<string, unknown>,
      {
        slots,
        attrs,
      }: {
        slots: Record<string, (() => unknown) | undefined>
        attrs: Record<string, unknown>
      },
    ) {
      return () =>
        h(
          'button',
          {
            ...attrs,
            type: 'button',
            disabled: props.disabled === true,
            'data-loading': props.loading === true ? 'true' : 'false',
          },
          slots.default?.(),
        )
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
    if (spec === '@/composables/useEndpointDialogValidation') {
      return endpointDialogValidation
    }
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

test('the editor no longer mounts the organization usage or Admin key blocks', async () => {
  const html = await renderDialog({
    ...subscriptionForm(),
    plan: 'platform_api_key',
  })
  expect(html).not.toContain('data-stub="EndpointAdminApiKeyFields.vue"')
  expect(html).not.toContain('data-stub="EndpointOrganizationUsage.vue"')
})

test('non-OpenAI endpoints hide the organization usage and Admin key blocks', async () => {
  const html = await renderDialog({
    ...createEmptyEndpointForm(),
    name: 'generic',
    provider: 'generic',
  })
  expect(html).not.toContain('data-stub="EndpointAdminApiKeyFields.vue"')
  expect(html).not.toContain('data-stub="EndpointOrganizationUsage.vue"')
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
    const localized = endpointOpenAiMessages[locale] as Record<string, string>
    const merged = messages[locale] as Record<string, string>
    for (const key of keys) {
      expect(localized[key]?.length ?? 0).toBeGreaterThan(0)
      // Extracted keys stay flat in the merged locale object.
      expect(merged[key]).toBe(localized[key])
    }
  }
})

// Issue #1004 P1: the editor shows the ChatGPT login state only. The
// subscription quota lives on the standalone quota surface, so the section
// neither fetches nor renders it.
import { ref } from 'vue'

const oauthSource = await Bun.file(
  new URL(
    '../src/components/endpoints/EndpointOAuthSection.vue',
    import.meta.url,
  ),
).text()

const statusSource = await Bun.file(
  new URL('../src/composables/useEndpointOAuthStatus.ts', import.meta.url),
).text()

const mockStatus = ref<unknown>(null)
const mockStatusError = ref('')
const mockDeviceStarting = ref(false)
const mockBrowserBusy = ref(false)
const mockRefreshing = ref(false)
const mockClearing = ref(false)

const mockOAuthActions = {
  useEndpointOAuthActions: () => ({
    browserFlow: {
      browser: ref(null),
      busy: mockBrowserBusy,
      completeBrowser: async () => {},
      message: ref(''),
      redirectUrl: ref(''),
      startBrowser: async () => {},
    },
    clearToken: async () => {},
    clearing: mockClearing,
    deviceFlow: {
      device: ref(null),
      message: ref(''),
      polling: ref(false),
      startDevice: async () => {},
      starting: mockDeviceStarting,
    },
    refreshToken: async () => {},
    refreshing: mockRefreshing,
    status: mockStatus,
    statusError: mockStatusError,
  }),
}

function compileSfc(source: string, filename: string, id: string) {
  const { descriptor } = parse(source, { filename })
  const script = compileScript(descriptor, { id, inlineTemplate: true })
  return rewriteSfcImports(script.content).replace(/export default/, 'return')
}

const oauthComponent = ((): Parameters<typeof h>[0] => {
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    compileSfc(
      oauthSource,
      'EndpointOAuthSection.vue',
      'endpoint-oauth-section',
    ),
  )
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return vue
    if (spec === '@/composables/useEndpointOAuthActions') {
      return mockOAuthActions
    }
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(
    resolve,
  ) as Parameters<typeof h>[0]
})()

async function renderOAuth(): Promise<string> {
  const app = createSSRApp({
    render: () =>
      h(oauthComponent, {
        endpointId: 'endpoint-1',
        t: (key: string) => key,
      }),
  })
  app.component('UModal', slotStub('UModal', 'body'))
  app.component('UProgress', slotStub('UProgress') as never)
  app.component('UButton', slotStub('UButton') as never)
  app.component('UBadge', slotStub('UBadge') as never)
  return renderToString(app)
}

test('the editor neither fetches nor renders the subscription quota', () => {
  expect(statusSource).not.toContain('getEndpointTokenPlanUsage')
  expect(oauthSource).not.toContain('endpointOAuthQuota')
})

test('the OAuth section shows the login state without a quota surface', async () => {
  mockStatus.value = {
    has_oauth_token: true,
    plan: 'chatgpt_subscription',
    expired: false,
    expires_at: null,
  }
  mockStatusError.value = ''
  const html = await renderOAuth()
  expect(html).toContain('endpointOAuthLoggedIn')
  expect(html).toContain('endpointPlanChatgptSubscription')
  expect(html).not.toContain('endpointOAuthQuota')
})

test('the OAuth section keeps the login controls for a logged-out endpoint', async () => {
  mockStatus.value = {
    has_oauth_token: false,
    plan: 'platform_api_key',
    expired: false,
    expires_at: null,
  }
  const html = await renderOAuth()
  expect(html).toContain('data-stub="EndpointOAuthDeviceLogin.vue"')
  expect(html).toContain('data-stub="EndpointOAuthBrowserLogin.vue"')
  expect(html).toContain('endpointOAuthNotLoggedIn')
})

// Issue #1004 P1: the parent page binds the OAuth action handler with a
// kebab-case attribute, which Vue normalizes to a camelCase prop name. The
// dialog has to declare that exact name, or the handler never arrives and the
// login controls stay disabled. The regression renders the real page binding
// against the real dialog and the real login section.
const pageSource = await Bun.file(
  new URL('../src/pages/EndpointsPage.vue', import.meta.url),
).text()

const deviceSource = await Bun.file(
  new URL(
    '../src/components/endpoints/EndpointOAuthDeviceLogin.vue',
    import.meta.url,
  ),
).text()

const browserSource = await Bun.file(
  new URL(
    '../src/components/endpoints/EndpointOAuthBrowserLogin.vue',
    import.meta.url,
  ),
).text()

/** The attribute the real parent binds the OAuth action handler with. */
function parentOAuthActionAttribute(): string {
  const template = parse(pageSource, { filename: 'EndpointsPage.vue' })
    .descriptor.template!.content
  const match = template.match(/:([a-z0-9-]+)="runEndpointOAuthAction"/)
  if (!match?.[1]) {
    throw new Error('the parent does not bind the OAuth action handler')
  }
  return match[1]
}

/** The prop name Vue derives from a kebab-case attribute. */
function normalizedPropName(attribute: string): string {
  return attribute.replace(/-([a-z0-9])/g, (_, character: string) =>
    character.toUpperCase(),
  )
}

function compileRealSfc(
  source: string,
  filename: string,
  id: string,
  resolve: (spec: string) => unknown,
): Component {
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    compileSfc(source, filename, id),
  )
  return new Function('__resolve', `"use strict";\n${js}`)(resolve) as Component
}

const deviceComponent = compileRealSfc(
  deviceSource,
  'EndpointOAuthDeviceLogin.vue',
  'endpoint-oauth-device-login',
  (spec) => (spec === 'vue' ? vue : slotStub(spec.split('/').pop() ?? spec)),
)

const browserComponent = compileRealSfc(
  browserSource,
  'EndpointOAuthBrowserLogin.vue',
  'endpoint-oauth-browser-login',
  (spec) => (spec === 'vue' ? vue : slotStub(spec.split('/').pop() ?? spec)),
)

const oauthSectionComponent = compileRealSfc(
  oauthSource,
  'EndpointOAuthSection.vue',
  'endpoint-oauth-section-real',
  (spec) => {
    if (spec === 'vue') return vue
    if (spec === '@/composables/useEndpointOAuthActions')
      return mockOAuthActions
    if (spec.endsWith('EndpointOAuthDeviceLogin.vue')) return deviceComponent
    if (spec.endsWith('EndpointOAuthBrowserLogin.vue')) return browserComponent
    return slotStub(spec.split('/').pop() ?? spec)
  },
)

const dialogWithRealSection = compileRealSfc(
  dialogSource,
  'EndpointDialog.vue',
  'endpoint-dialog-real',
  (spec) => {
    if (spec === 'vue') return vue
    if (spec === '@/admin-mappers') return adminMappers
    if (spec === '@/composables/useEndpointDialogValidation') {
      return endpointDialogValidation
    }
    if (spec.endsWith('EndpointApiKeysEditor.vue')) return apiKeysStub
    if (spec.endsWith('EndpointOAuthSection.vue')) return oauthSectionComponent
    return slotStub(spec.split('/').pop() ?? spec)
  },
)

async function renderPageBoundDialog(
  form: EndpointForm,
  runAction: unknown,
): Promise<string> {
  const attribute = parentOAuthActionAttribute()
  const app = createSSRApp({
    render: () =>
      h(dialogWithRealSection, {
        form,
        visible: true,
        busy: false,
        header: 'endpoint',
        t: (key: string) => key,
        users: [],
        // The parent binds the handler with the kebab-case attribute; Vue
        // normalizes it to the camelCase prop the dialog declares.
        [attribute]: runAction,
      }),
  })
  app.component('UModal', slotStub('UModal', 'body'))
  app.component('USelect', slotStub('USelect'))
  app.component('USwitch', slotStub('USwitch'))
  app.component('UButton', buttonStub())
  app.component('UTooltip', slotStub('UTooltip'))
  app.component('UIcon', slotStub('UIcon'))
  app.component('UInput', slotStub('UInput'))
  app.component('UBadge', slotStub('UBadge'))
  app.component('UProgress', slotStub('UProgress'))
  return renderToString(app)
}

/** Whether a control of the rendered section carries the disabled state. */
function controlStates(html: string): Record<string, boolean> {
  const buttons = Array.from(
    html.matchAll(/<button[^>]*>([\s\S]*?)<\/button>/g),
  ).map((match) => ({
    disabled: match[0]!.includes('disabled'),
    label: match[1]!.replace(/<[^>]*>/g, '').trim(),
  }))
  const states: Record<string, boolean> = {}
  for (const button of buttons) {
    states[button.label] = button.disabled
  }
  return states
}

test('the parent binding reaches the real login section', async () => {
  const attribute = parentOAuthActionAttribute()
  expect(attribute).toBe('run-oauth-action')
  // The dialog must declare the prop name Vue derives from that attribute.
  expect(normalizedPropName(attribute)).toBe('runOauthAction')
  expect(dialogSource).toContain('runOauthAction?: RunEndpointOAuthAction')
  expect(dialogSource).toContain(':run-action="runOauthAction"')
})

test('the idle login controls are enabled through the parent binding', async () => {
  mockStatus.value = {
    has_oauth_token: false,
    plan: 'platform_api_key',
    expired: false,
    expires_at: null,
  }
  const handler = async () => {}
  const html = await renderPageBoundDialog(subscriptionForm(), handler)
  const states = controlStates(html)
  expect(states['endpointOAuthDeviceStart']).toBe(false)
  expect(states['endpointOAuthBrowserStart']).toBe(false)
  // The section itself is the real one, not a stub.
  expect(html).toContain('endpointOAuthNotLoggedIn')
})

test('the refresh and clear controls are enabled through the parent binding', async () => {
  mockStatus.value = {
    has_oauth_token: true,
    plan: 'chatgpt_subscription',
    expired: false,
    expires_at: null,
  }
  const handler = async () => {}
  const html = await renderPageBoundDialog(subscriptionForm(), handler)
  const states = controlStates(html)
  expect(states['endpointOAuthRefresh']).toBe(false)
  expect(states['endpointOAuthClear']).toBe(false)
})

test('the login controls stay disabled without a handler', async () => {
  mockStatus.value = {
    has_oauth_token: true,
    plan: 'chatgpt_subscription',
    expired: false,
    expires_at: null,
  }
  const html = await renderPageBoundDialog(subscriptionForm(), undefined)
  const states = controlStates(html)
  expect(states['endpointOAuthRefresh']).toBe(true)
  expect(states['endpointOAuthClear']).toBe(true)
})

test('a pending login keeps its control disabled', async () => {
  mockStatus.value = {
    has_oauth_token: false,
    plan: 'platform_api_key',
    expired: false,
    expires_at: null,
  }
  mockDeviceStarting.value = true
  const html = await renderPageBoundDialog(subscriptionForm(), async () => {})
  // The device login reports the pending state instead of offering a second
  // start.
  expect(html).toContain('data-loading="true"')
  mockDeviceStarting.value = false
})

test('a busy browser login keeps its control disabled', async () => {
  mockStatus.value = {
    has_oauth_token: false,
    plan: 'platform_api_key',
    expired: false,
    expires_at: null,
  }
  mockBrowserBusy.value = true
  const html = await renderPageBoundDialog(subscriptionForm(), async () => {})
  const states = controlStates(html)
  expect(states['endpointOAuthBrowserStart']).toBe(true)
  mockBrowserBusy.value = false
})

test('a failed status read is reported next to the controls', async () => {
  mockStatus.value = {
    has_oauth_token: true,
    plan: 'chatgpt_subscription',
    expired: false,
    expires_at: null,
  }
  mockStatusError.value = 'the login state could not be read'
  const html = await renderPageBoundDialog(subscriptionForm(), async () => {})
  expect(html).toContain('the login state could not be read')
  mockStatusError.value = ''
})
