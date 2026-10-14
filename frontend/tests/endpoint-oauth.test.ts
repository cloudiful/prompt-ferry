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

test('OpenAI endpoints mount the Admin API Key and organization usage sections', async () => {
  const html = await renderDialog({
    ...subscriptionForm(),
    plan: 'platform_api_key',
  })
  expect(html).toContain('data-stub="EndpointAdminApiKeyFields.vue"')
  expect(html).toContain('data-stub="EndpointOrganizationUsage.vue"')
})

test('non-OpenAI endpoints hide the Admin API Key and organization usage sections', async () => {
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
    const localized = endpointOpenAiMessages[locale] as Record<string, string>
    const merged = messages[locale] as Record<string, string>
    for (const key of keys) {
      expect(localized[key]?.length ?? 0).toBeGreaterThan(0)
      // Extracted keys stay flat in the merged locale object.
      expect(merged[key]).toBe(localized[key])
    }
  }
})

// P3: the OAuth quota section renders the same canonical rows as the dialog
// through the shared row component, so subscription `model.windows` data no
// longer depends on the old hardcoded `model_remains[0].interval/weekly` pair.
import { ref } from 'vue'
import type {
  CanonicalQuotaWindow,
  TokenPlanKeyUsage,
  TokenPlanUsageResponse,
} from '../src/generated/admin-api'
import * as canonicalQuota from '../src/composables/useCanonicalQuotaWindows'
import * as tokenPlanFormatting from '../src/composables/useTokenPlanWindowEntries'
import * as subscriptionQuota from '../src/composables/subscriptionQuotaWindows'

const FIXED_NOW = Date.parse('2026-10-11T00:00:00.000Z')
const FUTURE_ISO = new Date(FIXED_NOW + 3_600_000).toISOString()
const PAST_ISO = new Date(FIXED_NOW - 3_600_000).toISOString()

const oauthSource = await Bun.file(
  new URL(
    '../src/components/endpoints/EndpointOAuthSection.vue',
    import.meta.url,
  ),
).text()

const oauthRowSource = await Bun.file(
  new URL(
    '../src/components/endpoints/TokenPlanQuotaWindowRow.vue',
    import.meta.url,
  ),
).text()

// Controllable OAuth state: each test sets `status`/`quota` before render,
// so the section reads canonical rows without any network or store.
const mockStatus = ref<unknown>(null)
const mockQuota = ref<TokenPlanUsageResponse | null>(null)
const mockQuotaError = ref('')
const mockStatusError = ref('')

const mockOAuthActions = {
  useEndpointOAuthActions: () => ({
    browserFlow: {
      browser: ref(null),
      busy: ref(false),
      completeBrowser: async () => {},
      message: ref(''),
      redirectUrl: ref(''),
      startBrowser: async () => {},
    },
    clearToken: async () => {},
    clearing: ref(false),
    deviceFlow: {
      device: ref(null),
      message: ref(''),
      polling: ref(false),
      startDevice: async () => {},
      starting: ref(false),
    },
    quota: mockQuota,
    quotaError: mockQuotaError,
    refreshToken: async () => {},
    refreshing: ref(false),
    status: mockStatus,
    statusError: mockStatusError,
  }),
}

// The ticker is time, not quota logic: freeze it so reset freshness is
// deterministic while every other formatting helper stays real.
const mockWindowEntries = {
  ...tokenPlanFormatting,
  useTokenPlanTicker: () => ref(FIXED_NOW),
}

function compileSfc(source: string, filename: string, id: string) {
  const { descriptor } = parse(source, { filename })
  const script = compileScript(descriptor, { id, inlineTemplate: true })
  return rewriteSfcImports(script.content).replace(/export default/, 'return')
}

const oauthRowComponent = ((): unknown => {
  const js = new Bun.Transpiler({ loader: 'ts' }).transformSync(
    compileSfc(
      oauthRowSource,
      'TokenPlanQuotaWindowRow.vue',
      'oauth-quota-row',
    ),
  )
  const resolve = (spec: string): unknown => {
    if (spec === 'vue') return vue
    if (spec === '@/composables/useCanonicalQuotaWindows') return canonicalQuota
    if (spec === '@/composables/useTokenPlanWindowEntries') {
      return tokenPlanFormatting
    }
    return slotStub(spec.split('/').pop() ?? spec)
  }
  return new Function('__resolve', `"use strict";\n${js}`)(resolve)
})()

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
    if (spec === '@/composables/useEndpointOAuthActions')
      return mockOAuthActions
    if (spec === '@/composables/useCanonicalQuotaWindows') return canonicalQuota
    if (spec === '@/composables/useTokenPlanWindowEntries') {
      return mockWindowEntries
    }
    if (spec === '@/composables/subscriptionQuotaWindows') {
      return subscriptionQuota
    }
    if (spec === '@/components/endpoints/TokenPlanQuotaWindowRow.vue') {
      return oauthRowComponent
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
        t: (key: string, args?: Record<string, unknown>) => {
          if (key === 'tokenPlanDurationWindow') {
            return `Window (${String((args as Record<string, unknown>)?.duration ?? '')})`
          }
          return key
        },
      }),
  })
  app.component('UModal', slotStub('UModal', 'body'))
  app.component('UProgress', slotStub('UProgress') as never)
  app.component('UButton', slotStub('UButton') as never)
  app.component('UBadge', slotStub('UBadge') as never)
  return renderToString(app)
}

function oauthKey(
  overrides: Partial<TokenPlanKeyUsage> = {},
): TokenPlanKeyUsage {
  return {
    key_id: 'k-oauth',
    key_label: 'oauth key',
    ok: true,
    model_remains: [],
    ...overrides,
  } as TokenPlanKeyUsage
}

function loggedInWithQuota(quota: TokenPlanUsageResponse | null): void {
  mockStatus.value = {
    has_oauth_token: true,
    plan: 'chatgpt_subscription',
    expired: false,
    expires_at: null,
  }
  mockQuota.value = quota
  mockQuotaError.value = ''
  mockStatusError.value = ''
}

test('OAuth renders actual subscription windows with the same values as the dialog', async () => {
  loggedInWithQuota({
    keys: [
      oauthKey({
        model_remains: [
          {
            model_name: 'plus',
            interval: null,
            weekly: null,
            windows: [
              {
                source_window: 'primary',
                window_seconds: 18_000,
                used_percent: 2,
                remaining_percent: 98,
                reset_at: FUTURE_ISO,
                reset_after_seconds: null,
                availability: 'known',
              },
              {
                source_window: 'secondary',
                window_seconds: 604_800,
                used_percent: null,
                remaining_percent: null,
                reset_at: FUTURE_ISO,
                reset_after_seconds: null,
                availability: 'unknown',
              },
            ],
          },
        ],
      }),
    ],
    provider: 'openai',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse)
  const html = await renderOAuth()
  // The 5h window keeps its honest percent; the unknown secondary degrades
  // to the same dash the dialog shows instead of a fabricated bar.
  expect(html).toContain('98.0%')
  expect(html).toContain('—')
  expect(html).not.toContain(' endpointOAuthQuotaPrimary')
})

test('OAuth prefers canonical observations over legacy fields', async () => {
  loggedInWithQuota({
    keys: [
      {
        ...oauthKey({
          model_remains: [
            {
              model_name: 'plus',
              interval: null,
              weekly: null,
              windows: [
                {
                  source_window: 'primary',
                  window_seconds: 18_000,
                  used_percent: 2,
                  remaining_percent: 98,
                  reset_at: FUTURE_ISO,
                  reset_after_seconds: null,
                  availability: 'known',
                },
              ],
            },
          ],
        }),
        quota_windows: [
          {
            source_id: 'subscription:primary',
            kind: 'custom',
            source_model: 'plus',
            unit: null,
            duration_seconds: 18_000,
            is_default_scope: false,
            availability: 'known',
            remaining_percent: 60,
            current: null,
            limit: null,
            remaining: null,
            reset_at: FUTURE_ISO,
          },
        ],
      } as unknown as TokenPlanKeyUsage,
    ],
    provider: 'openai',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse)
  const html = await renderOAuth()
  // Canonical 60 wins over legacy 98: observations are preferred, never merged.
  expect(html).toContain('60.0%')
  expect(html).not.toContain('98.0%')
})

test('OAuth shows the actual custom duration instead of a hardcoded slot', async () => {
  loggedInWithQuota({
    keys: [
      oauthKey({
        model_remains: [
          {
            model_name: 'plus',
            interval: null,
            weekly: null,
            windows: [
              {
                source_window: 'extra',
                window_seconds: 3_600,
                used_percent: 10,
                remaining_percent: 90,
                reset_at: FUTURE_ISO,
                reset_after_seconds: null,
                availability: 'known',
              },
            ],
          },
        ],
      }),
    ],
    provider: 'openai',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse)
  const html = await renderOAuth()
  expect(html).toContain('90.0%')
  expect(html).toContain('Window (1h)')
})

test('OAuth renders an expired window as unknown without keeping its percent', async () => {
  loggedInWithQuota({
    keys: [
      oauthKey({
        quota_windows: [
          {
            source_id: 'subscription:primary',
            kind: 'custom',
            source_model: 'plus',
            unit: null,
            duration_seconds: 18_000,
            is_default_scope: false,
            availability: 'known',
            remaining_percent: 75,
            current: null,
            limit: null,
            remaining: null,
            reset_at: PAST_ISO,
          },
        ] as CanonicalQuotaWindow[],
      }),
    ],
    provider: 'openai',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse)
  const html = await renderOAuth()
  expect(html).toContain('—')
  expect(html).not.toContain('75.0%')
})

test('OAuth renders a genuine zero as an exhausted 0% bar', async () => {
  loggedInWithQuota({
    keys: [
      oauthKey({
        quota_windows: [
          {
            source_id: 'subscription:primary',
            kind: 'custom',
            source_model: 'plus',
            unit: null,
            duration_seconds: 18_000,
            is_default_scope: false,
            availability: 'known',
            remaining_percent: 0,
            current: null,
            limit: null,
            remaining: null,
            reset_at: FUTURE_ISO,
          },
        ] as CanonicalQuotaWindow[],
      }),
    ],
    provider: 'openai',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse)
  const html = await renderOAuth()
  expect(html).toContain('0.0%')
  expect(html).not.toContain('—')
})

test('OAuth keeps quota errors and the login surface untouched', async () => {
  mockStatus.value = {
    has_oauth_token: true,
    plan: 'chatgpt_subscription',
    expired: false,
    expires_at: null,
  }
  mockQuota.value = {
    keys: [],
    provider: 'openai',
    provider_region: null,
  } as unknown as TokenPlanUsageResponse
  mockQuotaError.value = 'quota boom'
  mockStatusError.value = ''
  const html = await renderOAuth()
  expect(html).toContain('quota boom')
  // Logging out still shows the device/browser login, not quota rows.
  mockStatus.value = {
    has_oauth_token: false,
    plan: 'platform_api_key',
    expired: false,
    expires_at: null,
  }
  mockQuota.value = null
  const loggedOut = await renderOAuth()
  expect(loggedOut).toContain('data-stub="EndpointOAuthDeviceLogin.vue"')
})
