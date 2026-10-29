import { Gauge, LoaderCircle, Pencil, RefreshCw, Trash2 } from 'lucide-react'
import { QuotaBadges } from '@/components/endpoints/QuotaBadges'
import { ProviderIcon } from '@/components/providers/ProviderIcon'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { TestResultPopover } from '@/components/shared/TestResultPopover'
import { useLocale } from '@/lib/i18n'
import { isQuotaEligible } from '@/models/endpoints/quota'
import {
  endpointActionKey,
  networkTestKey,
  type EndpointListItemView,
} from '@/models/endpoints'

type EndpointMobileCardProps = {
  item: EndpointListItemView
  onDelete: (endpointId: string) => void
  onEdit: (endpointId: string) => void
  onTest: (endpointId: string) => void
  onTokenPlanUsage: (endpointId: string) => void
  onToggle: (endpointId: string, enabled: boolean) => void
  pendingActions: Readonly<Record<string, string>>
}

export function EndpointMobileCard({
  item,
  onDelete,
  onEdit,
  onTest,
  onTokenPlanUsage,
  onToggle,
  pendingActions,
}: EndpointMobileCardProps) {
  const { t } = useLocale()
  const rowKey = endpointActionKey(item.endpoint_id)
  const probeKey = networkTestKey('endpoint', item.endpoint_id)
  const rowPending = Object.hasOwn(pendingActions, rowKey)
  const rowOrigin = pendingActions[rowKey]
  const probePending = Object.hasOwn(pendingActions, probeKey)

  return (
    <article
      className="grid min-w-0 gap-3 rounded-xl border bg-card p-3"
      data-endpoint-card={item.endpoint_id}
    >
      <div className="flex min-w-0 items-start gap-2">
        <ProviderIcon provider={item.provider} size="md" />
        <div className="min-w-0 flex-1">
          <div
            className="break-words font-semibold"
            data-endpoint-name
            title={item.name}
          >
            {item.name}
          </div>
        </div>
      </div>
      <div className="grid min-w-0 gap-2 text-sm">
        {item.owner_label ? (
          <div className="grid gap-0.5">
            <span className="text-xs text-muted-foreground">{t('owner')}</span>
            <span className="break-words" data-endpoint-owner>
              {item.owner_label}
            </span>
          </div>
        ) : null}
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground">{t('status')}</span>
          <Switch
            aria-label={t('endpointEnabledToggle', { name: item.name })}
            checked={item.enabled}
            disabled={rowPending}
            onCheckedChange={(enabled) => onToggle(item.endpoint_id, enabled)}
          />
          {rowOrigin === 'toggle' ? (
            <LoaderCircle
              aria-label={t('loading')}
              className="size-4 animate-spin"
            />
          ) : null}
        </div>
        <div className="flex min-w-0 items-center gap-2">
          <span className="text-xs text-muted-foreground">{t('test')}</span>
          <TestResultPopover
            message={item.test_message}
            severity={item.test_severity}
          />
        </div>
        {isQuotaEligible(item) ? (
          <div className="grid min-w-0 gap-1">
            <span className="text-xs text-muted-foreground">
              {t('tokenPlanUsage')}
            </span>
            <QuotaBadges
              density="card"
              eligible
              endpointId={item.endpoint_id}
            />
          </div>
        ) : null}
      </div>
      <div className="grid grid-cols-2 gap-2">
        {isQuotaEligible(item) ? (
          <Button
            aria-label={t('tokenPlanUsage')}
            onClick={() => onTokenPlanUsage(item.endpoint_id)}
            type="button"
            variant="outline"
          >
            <Gauge aria-hidden="true" />
            {t('tokenPlanUsage')}
          </Button>
        ) : null}
        <Button
          disabled={probePending}
          onClick={() => onTest(item.endpoint_id)}
          type="button"
          variant="outline"
        >
          {probePending ? (
            <LoaderCircle aria-hidden="true" className="animate-spin" />
          ) : (
            <RefreshCw aria-hidden="true" />
          )}
          {t('test')}
        </Button>
        <Button
          disabled={rowPending}
          onClick={() => onEdit(item.endpoint_id)}
          type="button"
          variant="outline"
        >
          <Pencil aria-hidden="true" />
          {t('edit')}
        </Button>
        <Button
          className="col-span-2"
          disabled={rowPending}
          onClick={() => onDelete(item.endpoint_id)}
          type="button"
          variant="outline"
        >
          {rowOrigin === 'delete' ? (
            <LoaderCircle aria-hidden="true" className="animate-spin" />
          ) : (
            <Trash2 aria-hidden="true" />
          )}
          {t('delete')}
        </Button>
      </div>
    </article>
  )
}
