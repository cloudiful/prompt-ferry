import { useState } from 'react'
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
import { useLocale } from '@/lib/i18n'

type ModelRouteTargetsSummaryProps = {
  targets: ModelRouteTargetItemView[]
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

export function ModelRouteTargetsSummary({
  targets,
}: ModelRouteTargetsSummaryProps) {
  const { t } = useLocale()
  const [open, setOpen] = useState(false)
  const pairs = enabledRouteTargets(targets).map(toPair)
  const preview = routeTargetPreview(targets).map(toPair)
  const hidden = hiddenRouteTargetCount(targets)

  if (targets.length === 0)
    return <span className="text-xs text-muted-foreground">-</span>
  if (pairs.length === 0)
    return (
      <span className="text-xs text-muted-foreground">
        {t('modelRouteNoEnabledTargets')}
      </span>
    )

  return (
    <div className="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] gap-1">
      <div className="flex min-w-0 flex-wrap gap-1">
        {preview.map((pair) => (
          <span
            className="inline-flex min-w-0 max-w-full rounded-full border bg-background px-2 py-px text-xs"
            key={pair.target_id}
            title={pair.label}
          >
            <span className="min-w-0 truncate">{pair.endpoint}</span>
            {pair.override ? (
              <span className="ml-1 min-w-0 truncate text-muted-foreground">
                · {pair.override}
              </span>
            ) : null}
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
          <PopoverContent
            align="start"
            className="max-h-[min(60vh,18rem)] w-[min(18rem,calc(100vw-2rem))] overflow-y-auto p-2"
          >
            <ul className="grid gap-2">
              {pairs.map((pair) => (
                <li className="grid min-w-0 gap-0.5" key={pair.target_id}>
                  <span className="break-words font-medium">
                    {pair.endpoint}
                  </span>
                  {pair.override ? (
                    <span className="break-words text-xs text-muted-foreground">
                      {pair.override}
                    </span>
                  ) : null}
                </li>
              ))}
            </ul>
          </PopoverContent>
        </Popover>
      ) : null}
    </div>
  )
}
