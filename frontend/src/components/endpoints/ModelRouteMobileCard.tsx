import { LoaderCircle, Pencil, RefreshCw, Trash2 } from 'lucide-react'
import {
  TestResultPopover,
  testResultKey,
  type TestResultSurface,
} from '@/components/shared/TestResultPopover'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useLocale } from '@/lib/i18n'
import {
  modelRouteActionKey,
  type ModelRouteListItemView,
} from '@/models/endpoints'
import {
  modelRouteTargetToggleOriginTarget,
  modelRouteTestCounts,
} from '@/models/endpoints/model-route-item'
import { ModelRouteTargetsSummary } from './ModelRouteTargetsSummary'

type ModelRouteMobileCardProps = {
  item: ModelRouteListItemView
  onDelete: (ruleId: string) => void
  onEdit: (ruleId: string) => void
  onProbe: (ruleId: string, surface: TestResultSurface) => void
  onResultOpenChange: (key: string, open: boolean) => void
  onShowDetails: (ruleId: string) => void
  onToggle: (ruleId: string, enabled: boolean) => void
  onToggleTarget: (ruleId: string, targetId: string, enabled: boolean) => void
  openResultKey: string | null
  pendingActions: Readonly<Record<string, string>>
}

export function ModelRouteMobileCard({
  item,
  onDelete,
  onEdit,
  onProbe,
  onResultOpenChange,
  onShowDetails,
  onToggle,
  onToggleTarget,
  openResultKey,
  pendingActions,
}: ModelRouteMobileCardProps) {
  const { t } = useLocale()
  const writeKey = modelRouteActionKey(item.rule_id)
  const pending = pendingActions[writeKey]
  const counts = modelRouteTestCounts(item.test_results)
  const resultKey = testResultKey('mobile', item.rule_id)

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
        <ModelRouteTargetsSummary
          onToggleTarget={(targetId, enabled) =>
            onToggleTarget(item.rule_id, targetId, enabled)
          }
          pending={Boolean(pending)}
          pendingTargetId={modelRouteTargetToggleOriginTarget(pending)}
          targets={item.targets}
        />
        {item.owner_label ? (
          <span className="text-xs text-muted-foreground">
            {item.owner_label}
          </span>
        ) : null}
      </div>
      <div className="flex min-w-0 items-center justify-end gap-2">
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
      <div className="grid grid-cols-3 gap-2">
        <TestResultPopover
          detailLabel={
            counts.total ? `${counts.passed}/${counts.total}` : undefined
          }
          message={item.test_message}
          onOpenChange={(open) => onResultOpenChange(resultKey, open)}
          onSelect={() => onShowDetails(item.rule_id)}
          open={openResultKey === resultKey}
          severity={item.test_severity}
        >
          <Button
            disabled={item.testing}
            onClick={() => onProbe(item.rule_id, 'mobile')}
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
        </TestResultPopover>
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
