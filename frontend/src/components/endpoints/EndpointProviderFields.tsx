import { useStore } from '@tanstack/react-form'
import type { DeepKeys, DeepValue } from '@tanstack/form-core'
import { ProviderIcon } from '@/components/providers/ProviderIcon'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { SettingsFieldRow } from '@/components/shared/SettingsFieldRow'
import { useLocale } from '@/lib/i18n'
import {
  clearAdminApiKeyOutsideOpenAi,
  normalizeProviderPlan,
} from '@/admin-mappers/forms/endpoint'
import type { MessageKey } from '@/i18n'
import type {
  EndpointPlan,
  EndpointProvider,
  User,
} from '@/generated/admin-api'
import type { EndpointForm } from '@/models'
import type { EndpointFormApi } from '@/models/endpoints/form-api'

type EndpointProviderFieldsProps = {
  form: EndpointFormApi
  users: User[]
}

const PROVIDERS: Array<{ labelKey: MessageKey; value: EndpointProvider }> = [
  { labelKey: 'providerGeneric', value: 'generic' },
  { labelKey: 'providerMinimax', value: 'minimax' },
  { labelKey: 'providerCommandCode', value: 'command_code' },
  { labelKey: 'providerOpencodeGo', value: 'opencode_go' },
  { labelKey: 'providerOpenRouter', value: 'openrouter' },
  { labelKey: 'providerGlm', value: 'glm' },
  { labelKey: 'providerDeepSeek', value: 'deepseek' },
  { labelKey: 'providerOpenAi', value: 'openai' },
]

const PROTOCOLS: Array<{
  labelKey: MessageKey
  value:
    | 'auto'
    | 'anthropic_messages'
    | 'chat'
    | 'embeddings'
    | 'responses'
    | 'realtime'
}> = [
  { labelKey: 'endpointSourceAuto', value: 'auto' },
  { labelKey: 'nativeApiAnthropicMessages', value: 'anthropic_messages' },
  { labelKey: 'nativeApiChat', value: 'chat' },
  { labelKey: 'nativeApiEmbeddings', value: 'embeddings' },
  { labelKey: 'nativeApiResponses', value: 'responses' },
  { labelKey: 'nativeApiRealtime', value: 'realtime' },
]

export function EndpointProviderFields({
  form,
  users,
}: EndpointProviderFieldsProps) {
  const { t } = useLocale()
  const values = useStore(form.store, (state) => state.values)
  const provider = values.provider
  const plan = normalizeProviderPlan(values.provider, values.plan)
  const protocolValue =
    values.protocol_mode === 'auto'
      ? 'auto'
      : (values.native_api_override ?? 'responses')
  const hasVersionPath = /\/v1\/?$/.test(values.base_url.trim())

  function setField<K extends DeepKeys<EndpointForm>>(
    name: K,
    value: DeepValue<EndpointForm, K>,
  ): void {
    form.setFieldValue(name, value)
  }

  function changeProvider(value: EndpointProvider): void {
    const next: EndpointForm = { ...values, provider: value }
    next.plan = normalizeProviderPlan(value, values.plan)
    clearAdminApiKeyOutsideOpenAi(next)
    if (value !== 'minimax') {
      next.provider_region = null
      next.mcp_enabled = false
    } else {
      next.provider_region = values.provider_region ?? 'cn'
      if (!values.endpoint_id) next.mcp_enabled = true
    }
    setField('provider', next.provider)
    setField('plan', next.plan)
    setField('admin_api_key', next.admin_api_key)
    setField('admin_api_key_clear', next.admin_api_key_clear)
    setField('provider_region', next.provider_region)
    setField('mcp_enabled', next.mcp_enabled)
  }

  function changeProtocol(value: (typeof PROTOCOLS)[number]['value']): void {
    if (value === 'auto') {
      setField('protocol_mode', 'auto')
      setField('native_api_override', null)
      return
    }
    setField('protocol_mode', 'manual')
    setField('native_api_override', value)
  }

  return (
    <div className="grid gap-3">
      <div className="grid items-start gap-3 lg:grid-cols-[8rem_minmax(12rem,1fr)_minmax(12rem,1.4fr)_minmax(12rem,1fr)]">
        <label className="grid min-w-0 grid-rows-[1rem_auto] gap-1 text-xs font-medium">
          <span className="truncate">{t('scope')}</span>
          <Select
            onValueChange={(value) => {
              setField('scope', value as EndpointForm['scope'])
              if (value === 'admin') setField('owner_user_id', null)
            }}
            value={values.scope}
          >
            <SelectTrigger aria-label={t('scope')} className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="admin">{t('scopeAdmin')}</SelectItem>
              <SelectItem value="user">{t('scopeUser')}</SelectItem>
            </SelectContent>
          </Select>
        </label>
        <label className="grid min-w-0 grid-rows-[1rem_auto] gap-1 text-xs font-medium">
          <span className="truncate">{t('type')}</span>
          <Select
            onValueChange={(value) => changeProvider(value as EndpointProvider)}
            value={provider}
          >
            <SelectTrigger aria-label={t('type')} className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {PROVIDERS.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  <span className="flex items-center gap-2">
                    <ProviderIcon provider={option.value} size="sm" />
                    {t(option.labelKey)}
                  </span>
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>
        <label
          className="grid min-w-0 grid-rows-[1rem_auto] gap-1 text-xs font-medium"
          htmlFor="endpoint-name"
        >
          <span className="truncate">{t('name')}</span>
          <form.Field name="name">
            {(field) => (
              <Input
                autoComplete="off"
                id="endpoint-name"
                onChange={(event) =>
                  field.handleChange(event.currentTarget.value)
                }
                value={field.state.value}
              />
            )}
          </form.Field>
        </label>
        <label className="grid min-w-0 grid-rows-[1rem_auto] gap-1 text-xs font-medium">
          <span className="truncate">{t('modelRouteNativeApi')}</span>
          <Select
            onValueChange={(value) =>
              changeProtocol(value as (typeof PROTOCOLS)[number]['value'])
            }
            value={protocolValue}
          >
            <SelectTrigger
              aria-label={t('modelRouteNativeApi')}
              className="w-full"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {PROTOCOLS.map((option) => (
                <SelectItem key={option.value} value={option.value}>
                  {t(option.labelKey)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </label>
      </div>

      {values.scope === 'user' ? (
        <SettingsFieldRow label={t('owner')} htmlFor="endpoint-owner">
          <Select
            onValueChange={(value) => setField('owner_user_id', Number(value))}
            value={
              values.owner_user_id == null
                ? undefined
                : String(values.owner_user_id)
            }
          >
            <SelectTrigger
              aria-label={t('owner')}
              className="w-full"
              id="endpoint-owner"
            >
              <SelectValue placeholder={t('ownerUser')} />
            </SelectTrigger>
            <SelectContent>
              {users.map((user) => (
                <SelectItem key={user.user_id} value={String(user.user_id)}>
                  {user.display_name
                    ? `${user.login_name} / ${user.display_name}`
                    : user.login_name}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </SettingsFieldRow>
      ) : null}

      {provider === 'generic' ? (
        <SettingsFieldRow
          label={t('baseUrl')}
          hint={t('baseUrlHint')}
          htmlFor="endpoint-base-url"
        >
          <div className="grid gap-1">
            <form.Field name="base_url">
              {(field) => (
                <Input
                  autoComplete="url"
                  id="endpoint-base-url"
                  onChange={(event) =>
                    field.handleChange(event.currentTarget.value)
                  }
                  placeholder={t('baseUrl')}
                  value={field.state.value}
                />
              )}
            </form.Field>
            {hasVersionPath ? (
              <p className="text-xs text-muted-foreground">
                {t('baseUrlVersionWarning')}
              </p>
            ) : null}
          </div>
        </SettingsFieldRow>
      ) : null}

      {provider === 'minimax' ? (
        <SettingsFieldRow label={t('providerRegion')} htmlFor="endpoint-region">
          <Select
            onValueChange={(value) =>
              setField(
                'provider_region',
                value as NonNullable<EndpointForm['provider_region']>,
              )
            }
            value={values.provider_region ?? 'cn'}
          >
            <SelectTrigger
              aria-label={t('providerRegion')}
              className="w-full"
              id="endpoint-region"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="cn">{t('providerRegionCn')}</SelectItem>
              <SelectItem value="global">
                {t('providerRegionGlobal')}
              </SelectItem>
            </SelectContent>
          </Select>
        </SettingsFieldRow>
      ) : null}

      {provider === 'openai' ? (
        <SettingsFieldRow
          hint={
            values.has_oauth_token
              ? t('endpointPlanHint')
              : t('endpointPlanLoginRequired')
          }
          label={t('endpointPlan')}
          htmlFor="endpoint-plan"
        >
          <Select
            onValueChange={(value) =>
              setField(
                'plan',
                normalizeProviderPlan(values.provider, value as EndpointPlan),
              )
            }
            value={plan}
          >
            <SelectTrigger
              aria-label={t('endpointPlan')}
              className="w-full"
              id="endpoint-plan"
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="platform_api_key">
                {t('endpointPlanPlatformApiKey')}
              </SelectItem>
              <SelectItem
                disabled={!values.has_oauth_token}
                value="chatgpt_subscription"
              >
                {t('endpointPlanChatgptSubscription')}
              </SelectItem>
            </SelectContent>
          </Select>
        </SettingsFieldRow>
      ) : null}
    </div>
  )
}
