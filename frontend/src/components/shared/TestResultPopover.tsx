import type { ReactNode } from 'react'
import { AlertTriangle, Check, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'

/**
 * The list surface a test result belongs to. A desktop table and its mobile
 * card list render the same rows, so a result is only ever shown on the
 * surface that started the test.
 */
export type TestResultSurface = 'desktop' | 'mobile'

export function testResultKey(
  surface: TestResultSurface,
  rowId: string,
): string {
  return `${surface}:${rowId}`
}

type TestResultPopoverProps = {
  children: ReactNode
  detailLabel?: string
  message: string
  onOpenChange?: (open: boolean) => void
  onSelect?: () => void
  open?: boolean
  severity: 'success' | 'warning' | 'error' | null
}

/**
 * The one-shot result of a list test. The row's own action opens it, and it
 * carries the raw server message plus an optional entry into the detailed
 * results of that same test.
 */
export function TestResultPopover({
  children,
  detailLabel,
  message,
  onOpenChange,
  onSelect,
  open = false,
  severity,
}: TestResultPopoverProps) {
  const Icon =
    severity === 'success'
      ? Check
      : severity === 'warning'
        ? AlertTriangle
        : severity === 'error'
          ? X
          : null
  const tone =
    severity === 'success'
      ? 'text-success'
      : severity === 'warning'
        ? 'text-warning'
        : 'text-destructive'
  const detail =
    detailLabel && onSelect ? (
      <Button onClick={onSelect} size="xs" type="button" variant="outline">
        {detailLabel}
      </Button>
    ) : null

  return (
    <Popover onOpenChange={onOpenChange} open={open}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent
        align="end"
        className="max-h-[50vh] max-w-[min(22rem,calc(100vw-2rem))] overflow-auto break-words whitespace-pre-wrap leading-relaxed [overflow-wrap:anywhere]"
        side="bottom"
      >
        <div className="flex min-w-0 items-start gap-2">
          {Icon ? (
            <Icon aria-hidden="true" className={`mt-0.5 size-4 ${tone}`} />
          ) : null}
          <span className="min-w-0 flex-1">{message}</span>
        </div>
        {detail ? <div className="mt-2">{detail}</div> : null}
      </PopoverContent>
    </Popover>
  )
}
