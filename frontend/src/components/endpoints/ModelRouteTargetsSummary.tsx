import { useState } from 'react'
import { LoaderCircle } from 'lucide-react'
import type { ModelRouteTargetItemView } from '@/models/endpoints'
import {
  enabledRouteTargets,
  hiddenRouteTargetCount,
  routeTargetOverride,
  routeTargetPairLabel,
  routeTargetPreview,
} from '@/models/endpoints/model-route-item'
import { Button } from '@/components/ui/button'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { Switch } from '@/components/ui/switch'
import { useLocale } from '@/lib/i18n'

type ModelRouteTargetsSummaryProps = {
  targets: ModelRouteTargetItemView[]
  onToggleTarget?: (targetId: string, enabled: boolean) => void
  pending?: boolean
  pendingTargetId?: string | null
}

type TargetPair = {
  target_id: string
  endpoint: string
  override: string | null
  label: string
}

function toPair(target: ModelRouteTargetItemView): TargetPair {
  return {
    target_id: target.target_id,
    endpoint: target.endpoint_label,
    override: routeTargetOverride(target),
    label: routeTargetPairLabel(target),
  }
}

function PairChip({ pair }: { pair: TargetPair }) {
  return (
    <>
      <span className="min-w-0 truncate">{pair.endpoint}</span>
      {pair.override ? (
        <span className="ml-1 min-w-0 shrink-[100] truncate text-muted-foreground">
          · {pair.override}
        </span>
      ) : null}
    </>
  )
}

export function ModelRouteTargetsSummary({
  targets,
  onToggleTarget,
  pending = false,
  pendingTargetId = null,
}: ModelRouteTargetsSummaryProps) {
  const { t } = useLocale()
  const [open, setOpen] = useState(false)
  const enabled = enabledRouteTargets(targets)
  const preview = routeTargetPreview(targets).map(toPair)
  const hidden = hiddenRouteTargetCount(targets)

  if (targets.length === 0)
    return <span className="text-xs text-muted-foreground">-</span>

  const chipClassName =
    'inline-flex min-w-0 max-w-full items-center justify-self-start rounded-full border bg-background px-2 py-px text-xs'
  const firstPair = preview[0]
  const list = (
    <PopoverContent
      align="start"
      className="max-h-[min(60vh,18rem)] w-[min(20rem,calc(100vw-2rem))] overflow-y-auto p-2"
    >
      <ul className="grid gap-2">
        {targets.map((target) => {
          const override = routeTargetOverride(target)
          return (
            <li
              className="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-2"
              data-model-route-target={target.target_id}
              key={target.target_id}
            >
              <span className="grid min-w-0 gap-0.5">
                <span className="break-words font-medium">
                  {target.endpoint_label}
                </span>
                {override ? (
                  <span className="break-words text-xs text-muted-foreground">
                    {override}
                  </span>
                ) : null}
                {target.endpoint_enabled ? null : (
                  <span className="text-xs text-muted-foreground">
                    {t('modelRouteTargetEndpointDisabled')}
                  </span>
                )}
              </span>
              <span className="flex items-center gap-1">
                <Switch
                  aria-label={t('modelRouteTargetSwitch', {
                    target: routeTargetPairLabel(target),
                  })}
                  checked={target.enabled}
                  disabled={pending || !onToggleTarget}
                  onCheckedChange={(next) =>
                    onToggleTarget?.(target.target_id, next)
                  }
                  size="sm"
                />
                {pendingTargetId === target.target_id ? (
                  <LoaderCircle
                    aria-label={t('loading')}
                    className="size-4 animate-spin text-muted-foreground"
                  />
                ) : null}
              </span>
            </li>
          )
        })}
      </ul>
    </PopoverContent>
  )

  if (enabled.length === 0) {
    // Every configured target is off: the disclosure stays reachable so the
    // operator can switch one back on without opening the editor.
    return (
      <div className="grid min-w-0 max-w-[min(100%,24rem)] grid-cols-[minmax(0,1fr)_auto] gap-1">
        <Popover onOpenChange={setOpen} open={open}>
          <PopoverTrigger asChild>
            <Button
              className="h-7 justify-self-start px-2"
              size="xs"
              title={t('modelRouteMoreTargetsHint')}
              type="button"
              variant="outline"
            >
              {t('modelRouteNoEnabledTargets')}
            </Button>
          </PopoverTrigger>
          {list}
        </Popover>
      </div>
    )
  }

  if (hidden === 0 && firstPair) {
    // One enabled target: the single chip is the disclosure itself, so touch
    // and keyboard users can still read the complete endpoint and override.
    return (
      <div className="grid min-w-0 max-w-[min(100%,24rem)] grid-cols-[minmax(0,1fr)_auto] gap-1">
        <Popover onOpenChange={setOpen} open={open}>
          <PopoverTrigger asChild>
            <button
              aria-label={firstPair.label}
              className={`${chipClassName} cursor-pointer text-left`}
              title={firstPair.label}
              type="button"
            >
              <PairChip pair={firstPair} />
            </button>
          </PopoverTrigger>
          {list}
        </Popover>
      </div>
    )
  }

  return (
    <div className="grid min-w-0 max-w-[min(100%,24rem)] grid-cols-[minmax(0,1fr)_auto] gap-1">
      <div className="flex min-w-0 flex-nowrap gap-1 overflow-hidden">
        {preview.map((pair) => (
          <span
            className={chipClassName}
            key={pair.target_id}
            title={pair.label}
          >
            <PairChip pair={pair} />
          </span>
        ))}
      </div>
      {hidden > 0 ? (
        <Popover onOpenChange={setOpen} open={open}>
          <PopoverTrigger asChild>
            <Button
              aria-label={t('modelRouteMoreTargets', { count: hidden })}
              className="h-7 px-2"
              size="xs"
              type="button"
              variant="outline"
            >
              +{hidden}
            </Button>
          </PopoverTrigger>
          {list}
        </Popover>
      ) : null}
    </div>
  )
}
