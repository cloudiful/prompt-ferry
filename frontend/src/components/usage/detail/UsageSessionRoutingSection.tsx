import { useEffect, useState } from 'react'
import { LoaderCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useLocale } from '@/lib/i18n'
import {
  usageAffinityStateKey,
  type UsageConversationOverrideView,
  type UsageDetailView,
  type UsageRouteOptionsView,
} from '@/lib/usage-detail-model'

export const USAGE_AUTOMATIC_KEY_VALUE = '__automatic__'

type UsageSessionRoutingSectionProps = {
  detail: UsageDetailView
  isAdmin: boolean
  loading: boolean
  onClear: () => void
  onReset: () => void
  onSave: (endpointId: string, endpointKeyId: string | null) => void
  override: UsageConversationOverrideView | null
  pending: boolean
  pendingOrigin: string | null
  routeOptions: UsageRouteOptionsView | null
}

/**
 * The session-routing block: the current route, the binding state, the current
 * override and, for an admin only, the override and affinity actions.
 *
 * The store owns the write guard and the payloads; this section reports the
 * chosen target and shows a spinner only for the action that is pending.
 */
export function UsageSessionRoutingSection({
  detail,
  isAdmin,
  loading,
  onClear,
  onReset,
  onSave,
  override,
  pending,
  pendingOrigin,
  routeOptions,
}: UsageSessionRoutingSectionProps) {
  const { t } = useLocale()
  const [endpointId, setEndpointId] = useState('')
  const [endpointKeyId, setEndpointKeyId] = useState(USAGE_AUTOMATIC_KEY_VALUE)

  useEffect(() => {
    setEndpointId(routeOptions?.override_endpoint_id ?? '')
    setEndpointKeyId(
      routeOptions?.override_endpoint_key_id ?? USAGE_AUTOMATIC_KEY_VALUE,
    )
  }, [
    routeOptions?.override_endpoint_id,
    routeOptions?.override_endpoint_key_id,
  ])

  const keyOptions = [
    { label: t('autoRouteKey'), value: USAGE_AUTOMATIC_KEY_VALUE },
    ...(routeOptions?.options
      .find((option) => option.endpoint_id === endpointId)
      ?.keys.map((key) => ({
        label: key.key_label,
        value: key.key_id,
      })) ?? []),
  ]

  function displayKey(
    keyLabel: string | null | undefined,
    keyId: string | null | undefined,
  ): string {
    return keyLabel || keyId || t('autoRouteKey')
  }

  const affinity = routeOptions?.affinity
  const showSave = pendingOrigin === 'save'
  const showClear = pendingOrigin === 'clear'
  const showReset = pendingOrigin === 'reset'

  return (
    <section
      className="grid gap-2 rounded border p-3"
      data-usage-session-section
    >
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
        <h2 className="text-sm font-semibold">{t('sessionRouting')}</h2>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
          <span>
            {t('sessionId')}:{' '}
            <span className="break-all">{detail.conversation_id || '-'}</span>
          </span>
          <span>
            {t('recordRoute')}:{' '}
            <span className="break-all">
              {routeOptions?.current_upstream_label || detail.upstream_label}
            </span>{' '}
            /{' '}
            <span className="break-all">
              {displayKey(
                routeOptions?.current_endpoint_key_label,
                routeOptions?.current_endpoint_key_id,
              )}
            </span>
          </span>
          <span data-usage-affinity-state={affinity?.state ?? 'unknown'}>
            {t('sessionBinding')}:{' '}
            <span
              className={
                affinity?.state === 'active'
                  ? 'text-emerald-500'
                  : affinity?.state === 'stale_endpoint' ||
                      affinity?.state === 'stale_key'
                    ? 'text-amber-500'
                    : undefined
              }
            >
              {t(usageAffinityStateKey(affinity?.state))}
            </span>
            {affinity?.endpoint_id && (
              <>
                {' / '}
                <span className="break-all">
                  {affinity.endpoint_name || affinity.endpoint_id}
                </span>
                {' / '}
                <span className="break-all">
                  {displayKey(affinity.key_label, affinity.key_id)}
                </span>
              </>
            )}
          </span>
          <span>
            {t('currentOverride')}:{' '}
            <span className="break-all">
              {override?.endpoint_name || override?.endpoint_id || '-'}
            </span>
            {override && (
              <>
                {' / '}
                <span className="break-all">
                  {displayKey(
                    override.endpoint_key_label,
                    override.endpoint_key_id,
                  )}
                </span>
              </>
            )}
          </span>
        </div>
      </div>

      {routeOptions ? (
        isAdmin && (
          <div className="grid gap-2 lg:flex lg:flex-row lg:items-center">
            <Select
              disabled={pending || loading}
              onValueChange={(value) => {
                setEndpointId(value)
                setEndpointKeyId(USAGE_AUTOMATIC_KEY_VALUE)
              }}
              value={endpointId}
            >
              <SelectTrigger
                aria-label={t('selectUpstream')}
                className="w-full lg:flex-1"
                size="sm"
              >
                <SelectValue placeholder={t('selectUpstream')} />
              </SelectTrigger>
              <SelectContent>
                {routeOptions.options.map((option) => (
                  <SelectItem
                    key={option.endpoint_id}
                    value={option.endpoint_id}
                  >
                    {option.endpoint_name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Select
              disabled={pending || loading}
              onValueChange={setEndpointKeyId}
              value={endpointKeyId}
            >
              <SelectTrigger
                aria-label={t('selectUpstreamKey')}
                className="w-full lg:flex-1"
                size="sm"
              >
                <SelectValue placeholder={t('selectUpstreamKey')} />
              </SelectTrigger>
              <SelectContent>
                {keyOptions.map((option) => (
                  <SelectItem key={option.value} value={option.value}>
                    {option.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            <Button
              disabled={pending || loading || !endpointId}
              onClick={() =>
                onSave(
                  endpointId,
                  endpointKeyId === USAGE_AUTOMATIC_KEY_VALUE
                    ? null
                    : endpointKeyId,
                )
              }
              size="sm"
              type="button"
            >
              {showSave && (
                <LoaderCircle
                  aria-hidden="true"
                  className="size-4 animate-spin"
                />
              )}
              {t('setOverride')}
            </Button>
            <Button
              disabled={pending || loading || !override}
              onClick={onClear}
              size="sm"
              type="button"
              variant="outline"
            >
              {showClear && (
                <LoaderCircle
                  aria-hidden="true"
                  className="size-4 animate-spin"
                />
              )}
              {t('clearOverride')}
            </Button>
            <Button
              disabled={pending || loading}
              onClick={onReset}
              size="sm"
              type="button"
              variant="outline"
            >
              {showReset && (
                <LoaderCircle
                  aria-hidden="true"
                  className="size-4 animate-spin"
                />
              )}
              {t('resetAffinity')}
            </Button>
          </div>
        )
      ) : detail.conversation_id ? (
        <div className="text-xs text-muted-foreground">
          {loading ? t('loading') : t('sessionRoutingUnavailable')}
        </div>
      ) : null}
    </section>
  )
}
