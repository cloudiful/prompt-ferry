import type { RequestRecordFullResponse } from '@/generated/admin-api'
import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'
import { useLocale } from '@/lib/i18n'
import type {
  UsageConversationOverrideView,
  UsageDetailView,
  UsageRouteOptionsView,
} from '@/lib/usage-detail-model'
import { UsageContextSection } from './detail/UsageContextSection'
import { UsageErrorSection } from './detail/UsageErrorSection'
import { UsageSessionRoutingSection } from './detail/UsageSessionRoutingSection'
import { UsageTranscriptSection } from './detail/UsageTranscriptSection'

type UsageDetailDialogProps = {
  detail: UsageDetailView | null
  detailLoading: boolean
  isAdmin: boolean
  onClearOverride: () => void
  onLoadFirst: () => void
  onLoadMore: () => void
  onOpenChange: (open: boolean) => void
  onResetAffinity: () => void
  onSaveOverride: (endpointId: string, endpointKeyId: string | null) => void
  open: boolean
  override: UsageConversationOverrideView | null
  pending: boolean
  pendingOrigin: string | null
  requestFull: RequestRecordFullResponse | null
  requestFullLoading: boolean
  requestFullLoadingMore: boolean
  routeOptions: UsageRouteOptionsView | null
  routeOptionsLoading: boolean
}

/**
 * The AI request detail.
 *
 * While a session-routing write is pending the dialog cannot be dismissed by
 * its close button, Escape, an outside click or the parent; the controls and
 * the transcript stay read-only behind that gate, and only the running action
 * shows a spinner.
 */
export function UsageDetailDialog({
  detail,
  detailLoading,
  isAdmin,
  onClearOverride,
  onLoadFirst,
  onLoadMore,
  onOpenChange,
  onResetAffinity,
  onSaveOverride,
  open,
  override,
  pending,
  pendingOrigin,
  requestFull,
  requestFullLoading,
  requestFullLoadingMore,
  routeOptions,
  routeOptionsLoading,
}: UsageDetailDialogProps) {
  const { t } = useLocale()
  const showSession =
    Boolean(detail?.conversation_id) ||
    Boolean(routeOptions) ||
    Boolean(override)
  const resetKey = `${open ? 'open' : 'closed'}:${detail?.record_id ?? ''}`

  function confirmReset(): void {
    if (pending) return
    if (!window.confirm(t('resetAffinityConfirm'))) return
    onResetAffinity()
  }

  return (
    <Dialog
      onOpenChange={(next) => {
        if (pending && !next) return
        onOpenChange(next)
      }}
      open={open}
    >
      <DialogContent
        aria-describedby={undefined}
        className="max-h-[80vh] overflow-y-auto sm:max-w-6xl"
        onEscapeKeyDown={(event) => {
          if (pending) event.preventDefault()
        }}
        onInteractOutside={(event) => {
          if (pending) event.preventDefault()
        }}
        showCloseButton={!pending}
      >
        <DialogTitle>{t('requestDetails')}</DialogTitle>
        {detail ? (
          <div className="grid gap-3 text-xs">
            <UsageContextSection detail={detail} requestFull={requestFull} />
            {showSession && (
              <UsageSessionRoutingSection
                detail={detail}
                isAdmin={isAdmin}
                loading={routeOptionsLoading}
                onClear={onClearOverride}
                onReset={confirmReset}
                onSave={onSaveOverride}
                override={override}
                pending={pending}
                pendingOrigin={pendingOrigin}
                routeOptions={routeOptions}
              />
            )}
            <UsageErrorSection detail={detail} resetKey={resetKey} />
            <UsageTranscriptSection
              detail={detail}
              loading={requestFullLoading}
              loadingMore={requestFullLoadingMore}
              onLoadFirst={onLoadFirst}
              onLoadMore={onLoadMore}
              requestFull={requestFull}
              visible={open}
            />
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
