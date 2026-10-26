import { Button } from '@/components/ui/button'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { copyText } from '@/lib/clipboard'
import { useLocale } from '@/lib/i18n'
import {
  parseUsageResponsePayload,
  usageJsonText,
  type UsageDetailView,
} from '@/lib/usage-detail-model'
import { UsageErrorSection } from './detail/UsageErrorSection'
import { UsageJsonSyntax } from './detail/UsageJsonSyntax'

type McpUsageDetailDialogProps = {
  detail: UsageDetailView | null
  detailLoading: boolean
  onOpenChange: (open: boolean) => void
  open: boolean
}

/**
 * The MCP request/response payload detail.
 *
 * The response text is parsed only when it looks like an object or array, and
 * a failed parse keeps the raw text. Both payloads render through the safe
 * element-based JSON view; there is no transcript, context or session block.
 */
export function McpUsageDetailDialog({
  detail,
  detailLoading,
  onOpenChange,
  open,
}: McpUsageDetailDialogProps) {
  const { t } = useLocale()
  const hasRequest = detail?.request_raw_json != null
  const responseValue = parseUsageResponsePayload(
    detail?.response_prompt ?? null,
  )
  const hasResponse = responseValue != null
  const resetKey = `${open ? 'open' : 'closed'}:${detail?.record_id ?? ''}`

  function copy(text: string): void {
    void copyText(text).catch(() => {})
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        aria-describedby={undefined}
        className="max-h-[80vh] overflow-y-auto sm:max-w-5xl"
      >
        <DialogTitle>{t('requestDetails')}</DialogTitle>
        {detail ? (
          <div className="grid gap-3 text-xs">
            <div className="grid items-stretch gap-3 lg:grid-cols-2">
              <section className="grid gap-2 rounded border p-3">
                <h2 className="text-sm font-semibold">
                  {t('mcpRequestPayload')}
                </h2>
                {hasRequest ? (
                  <>
                    <Button
                      className="justify-self-start"
                      onClick={() =>
                        copy(usageJsonText(detail.request_raw_json))
                      }
                      size="sm"
                      type="button"
                      variant="ghost"
                    >
                      {t('copy')}
                    </Button>
                    <UsageJsonSyntax value={detail.request_raw_json} />
                  </>
                ) : (
                  <div className="text-xs text-muted-foreground">
                    {t('contentLoggingOff')}
                  </div>
                )}
              </section>

              <section className="grid gap-2 rounded border p-3">
                <h2 className="text-sm font-semibold">
                  {t('mcpResponsePayload')}
                </h2>
                {hasResponse ? (
                  <>
                    <Button
                      className="justify-self-start"
                      onClick={() => copy(usageJsonText(responseValue))}
                      size="sm"
                      type="button"
                      variant="ghost"
                    >
                      {t('copy')}
                    </Button>
                    <UsageJsonSyntax value={responseValue} />
                  </>
                ) : (
                  <div className="text-xs text-muted-foreground">
                    {detailLoading ? t('loading') : t('contentLoggingOff')}
                  </div>
                )}
              </section>
            </div>
            <UsageErrorSection detail={detail} resetKey={resetKey} />
          </div>
        ) : (
          detailLoading && (
            <div className="py-6 text-xs text-muted-foreground">
              {t('loading')}
            </div>
          )
        )}
      </DialogContent>
    </Dialog>
  )
}
