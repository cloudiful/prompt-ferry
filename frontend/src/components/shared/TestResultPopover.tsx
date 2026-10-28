import { AlertTriangle, Check, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'

type TestResultPopoverProps = {
  detailLabel?: string
  message: string
  onSelect?: () => void
  severity: 'success' | 'warning' | 'error' | null
}

export function TestResultPopover({
  detailLabel,
  message,
  onSelect,
  severity,
}: TestResultPopoverProps) {
  const detail =
    detailLabel && onSelect ? (
      <Button onClick={onSelect} size="xs" type="button" variant="outline">
        {detailLabel}
      </Button>
    ) : null

  if (severity === null) {
    return detail === null ? null : (
      <div className="flex min-w-0 items-center gap-1">{detail}</div>
    )
  }

  const Icon =
    severity === 'success' ? Check : severity === 'warning' ? AlertTriangle : X
  const tone =
    severity === 'success'
      ? 'text-success'
      : severity === 'warning'
        ? 'text-warning'
        : 'text-destructive'

  return (
    <div className="flex min-w-0 items-center gap-1">
      <Popover>
        <PopoverTrigger asChild>
          <Button
            aria-label={message}
            className="rounded-full"
            size="icon-xs"
            title={message}
            type="button"
            variant="ghost"
          >
            <Icon aria-hidden="true" className={tone} />
          </Button>
        </PopoverTrigger>
        <PopoverContent
          align="start"
          className="max-h-[50vh] max-w-[min(22rem,calc(100vw-2rem))] overflow-auto break-words whitespace-pre-wrap leading-relaxed [overflow-wrap:anywhere]"
          side="bottom"
        >
          {message}
        </PopoverContent>
      </Popover>
      {detail}
    </div>
  )
}
