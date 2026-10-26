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

type EndpointProviderFieldsProps = {
  form: EndpointForm
  onChange: (update: (current: EndpointForm) => EndpointForm) => void
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
  onChange,
  users,
}: EndpointProviderFieldsProps) {
  const { t } = useLocale()
  const provider = form.provider
  const plan = normalizeProviderPlan(form.provider, form.plan)
  const protocolValue =
    form.protocol_mode === 'auto'
      ? 'auto'
      : (form.native_api_override ?? 'responses')
  const hasVersionPath = /\/v1\/?$/.test(form.base_url.trim())

  function changeProvider(value: EndpointProvider): void {
    onChange((current) => {
      const next = { ...current, provider: value }
      next.plan = normalizeProviderPlan(value, current.plan)
      clearAdminApiKeyOutsideOpenAi(next)
      if (value !== 'minimax') {
        next.provider_region = null
        next.mcp_enabled = false
      } else {
        next.provider_region = current.provider_region ?? 'cn'
        if (!current.endpoint_id) next.mcp_enabled = true
      }
      return next
    })
  }

  function changeProtocol(value: (typeof PROTOCOLS)[number]['value']): void {
    onChange((current) =>
      value === 'auto'
        ? { ...current, protocol_mode: 'auto', native_api_override: null }
        : { ...current, protocol_mode: 'manual', native_api_override: value },
    )
  }

  return (
    <div className="grid gap-3">
      <div className="grid gap-3 md:grid-cols-[8rem_minmax(12rem,1fr)_minmax(12rem,1.4fr)_minmax(12rem,1fr)]">
        <label className="grid min-w-0 gap-1 text-xs font-medium">
          <span>{t('scope')}</span>
          <Select
            onValueChange={(value) =>
              onChange((current) => ({
                ...current,
                scope: value as EndpointForm['scope'],
                owner_user_id: value === 'admin' ? null : current.owner_user_id,
              }))
            }
            value={form.scope}
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
        <label className="grid min-w-0 gap-1 text-xs font-medium">
          <span>{t('type')}</span>
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
          className="grid min-w-0 gap-1 text-xs font-medium"
          htmlFor="endpoint-name"
        >
          <span>{t('name')}</span>
          <Input
            autoComplete="off"
            id="endpoint-name"
            onChange={(event) => {
              const name = event.currentTarget.value
              onChange((current) => ({ ...current, name }))
            }}
            value={form.name}
          />
        </label>
        <label className="grid min-w-0 gap-1 text-xs font-medium">
          <span>{t('modelRouteNativeApi')}</span>
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

      {form.scope === 'user' ? (
        <SettingsFieldRow label={t('owner')} htmlFor="endpoint-owner">
          <Select
            onValueChange={(value) =>
              onChange((current) => ({
                ...current,
                owner_user_id: Number(value),
              }))
            }
            value={
              form.owner_user_id == null
                ? undefined
                : String(form.owner_user_id)
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
            <Input
              autoComplete="url"
              id="endpoint-base-url"
              onChange={(event) => {
                const baseUrl = event.currentTarget.value
                onChange((current) => ({ ...current, base_url: baseUrl }))
              }}
              placeholder={t('baseUrl')}
              value={form.base_url}
            />
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
              onChange((current) => ({
                ...current,
                provider_region: value as NonNullable<
                  EndpointForm['provider_region']
                >,
              }))
            }
            value={form.provider_region ?? 'cn'}
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
            form.has_oauth_token
              ? t('endpointPlanHint')
              : t('endpointPlanLoginRequired')
          }
          label={t('endpointPlan')}
          htmlFor="endpoint-plan"
        >
          <Select
            onValueChange={(value) =>
              onChange((current) => ({
                ...current,
                plan: normalizeProviderPlan(
                  current.provider,
                  value as EndpointPlan,
                ),
              }))
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
                disabled={!form.has_oauth_token}
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
