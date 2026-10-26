import type { ModelRouteTestTargetResult } from '@/generated/admin-api'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useLocale } from '@/lib/i18n'

type ModelRouteTestResultsDialogProps = {
  onOpenChange: (open: boolean) => void
  open: boolean
  results: ModelRouteTestTargetResult[]
}

export function ModelRouteTestResultsDialog({
  onOpenChange,
  open,
  results,
}: ModelRouteTestResultsDialogProps) {
  const { t } = useLocale()

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent className="max-h-[min(90vh,48rem)] max-w-2xl overflow-y-auto">
        <DialogHeader>
          <DialogTitle>{t('modelRouteTestDetails')}</DialogTitle>
        </DialogHeader>
        <ol className="grid gap-2 text-xs">
          {results.map((result) => (
            <li
              className="grid min-w-0 gap-1 rounded-md border p-3"
              key={result.target_id}
            >
              <div className="flex min-w-0 flex-wrap items-center gap-2">
                <span
                  aria-label={result.ok ? t('success') : t('failed')}
                  className={result.ok ? 'text-success' : 'text-destructive'}
                >
                  {result.ok ? '✓' : '×'}
                </span>
                <span className="min-w-0 break-words font-medium">
                  {result.endpoint_name}
                </span>
                {result.preferred ? (
                  <span className="rounded-full border px-2 py-0.5 text-muted-foreground">
                    {t('modelRouteTestPreferred')}
                  </span>
                ) : null}
              </div>
              <div className="break-words text-muted-foreground">
                {result.model ?? '-'}
              </div>
              <div className="text-muted-foreground">
                HTTP {result.status ?? '-'} / {result.duration_ms}ms
              </div>
              <div className="break-words whitespace-pre-wrap">
                {result.message}
              </div>
            </li>
          ))}
        </ol>
        <DialogFooter>
          <Button
            onClick={() => onOpenChange(false)}
            type="button"
            variant="outline"
          >
            {t('modelRouteTestClose')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
