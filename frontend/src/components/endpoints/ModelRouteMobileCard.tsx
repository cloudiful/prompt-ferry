import { LoaderCircle, Pencil, RefreshCw, Trash2 } from 'lucide-react'
import { TestResultPopover } from '@/components/shared/TestResultPopover'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useLocale } from '@/lib/i18n'
import {
  modelRouteActionKey,
  type ModelRouteListItemView,
} from '@/models/endpoints'
import { modelRouteTestCounts } from '@/models/endpoints/model-route-item'
import { ModelRouteTargetsSummary } from './ModelRouteTargetsSummary'

type ModelRouteMobileCardProps = {
  item: ModelRouteListItemView
  onDelete: (ruleId: string) => void
  onEdit: (ruleId: string) => void
  onProbe: (ruleId: string) => void
  onShowDetails: (ruleId: string) => void
  onToggle: (ruleId: string, enabled: boolean) => void
  pendingActions: Readonly<Record<string, string>>
}

export function ModelRouteMobileCard({
  item,
  onDelete,
  onEdit,
  onProbe,
  onShowDetails,
  onToggle,
  pendingActions,
}: ModelRouteMobileCardProps) {
  const { t } = useLocale()
  const writeKey = modelRouteActionKey(item.rule_id)
  const pending = pendingActions[writeKey]
  const counts = modelRouteTestCounts(item.test_results)

  return (
    <article
      className="grid min-w-0 gap-3 rounded-xl border bg-card p-3"
      data-model-route-card={item.rule_id}
    >
      <div
        className="min-w-0 break-words font-semibold"
        aria-label={item.model_pattern}
        title={item.model_pattern}
      >
        {item.model_pattern}
      </div>
      <div className="grid min-w-0 gap-1">
        <span className="text-xs font-medium text-muted-foreground">
          {t('endpoint')}
        </span>
        <ModelRouteTargetsSummary targets={item.targets} />
        <span className="text-xs text-muted-foreground">
          {item.scope_label}
          {item.owner_label ? ` / ${item.owner_label}` : ''}
        </span>
      </div>
      <div className="flex min-w-0 items-center justify-between gap-2">
        <TestResultPopover
          detailLabel={
            counts.total ? `${counts.passed}/${counts.total}` : undefined
          }
          message={item.test_message}
          onSelect={() => onShowDetails(item.rule_id)}
          severity={item.test_severity}
        />
        <div className="flex items-center gap-2">
          <Switch
            aria-label={`${t('status')} ${item.model_pattern}`}
            checked={item.enabled}
            disabled={Boolean(pending)}
            onCheckedChange={(enabled) => onToggle(item.rule_id, enabled)}
          />
          {pending === 'toggle' ? (
            <LoaderCircle
              aria-label={t('loading')}
              className="size-4 animate-spin"
            />
          ) : null}
        </div>
      </div>
      <div className="grid grid-cols-3 gap-2">
        <Button
          disabled={item.testing}
          onClick={() => onProbe(item.rule_id)}
          type="button"
          variant="outline"
        >
          {item.testing ? (
            <LoaderCircle className="animate-spin" />
          ) : (
            <RefreshCw />
          )}{' '}
          {t('test')}
        </Button>
        <Button
          disabled={Boolean(pending)}
          onClick={() => onEdit(item.rule_id)}
          type="button"
          variant="outline"
        >
          <Pencil /> {t('edit')}
        </Button>
        <Button
          disabled={Boolean(pending)}
          onClick={() => onDelete(item.rule_id)}
          type="button"
          variant="outline"
        >
          {pending === 'delete' ? (
            <LoaderCircle className="animate-spin" />
          ) : (
            <Trash2 />
          )}{' '}
          {t('delete')}
        </Button>
      </div>
    </article>
  )
}
