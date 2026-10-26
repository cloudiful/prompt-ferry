import { LoaderCircle } from 'lucide-react'
import type { ApprovalRequest } from '@/generated/admin-api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import type { PendingActions } from '@/lib/async-action'
import { approvalActionKey } from '@/lib/approvals'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale } from '@/lib/i18n'
import { formatDateTime } from '@/format/datetime-core'

type ApprovalDetailDialogProps = {
  approval: ApprovalRequest | null
  loading: boolean
  onApprove: (approval: ApprovalRequest) => void
  onOpenChange: (open: boolean) => void
  onReject: (approval: ApprovalRequest) => void
  open: boolean
  pendingActions: PendingActions
}

function payloadText(value: unknown): string {
  return value == null ? '' : JSON.stringify(value, null, 2)
}

export function ApprovalDetailDialog({
  approval,
  loading,
  onApprove,
  onOpenChange,
  onReject,
  open,
  pendingActions,
}: ApprovalDetailDialogProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const action = approval
    ? (pendingActions[approvalActionKey(approval.approval_id)] ?? null)
    : null
  const pending = action !== null
  const isPending = approval?.approval_status === 'pending'

  function statusLabel(): string {
    return isPending ? t('approvalStatusPending') : t('approvalStatusResolved')
  }

  function handleOpenChange(next: boolean): void {
    // The answer is in flight: the dialog stays open until it has settled.
    if (!next && pending) return
    onOpenChange(next)
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-5xl">
        <DialogHeader>
          <DialogTitle>{t('approvalDetails')}</DialogTitle>
        </DialogHeader>
        {loading ? (
          <div
            aria-label={t('loading')}
            className="py-6 text-sm text-muted-foreground"
            role="status"
          >
            {t('loading')}
          </div>
        ) : approval ? (
          <div className="grid gap-3 text-xs">
            <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-4">
              <div>
                <div className="text-muted-foreground">{t('user')}</div>
                <div>{approval.user_login_name || '-'}</div>
              </div>
              <div>
                <div className="text-muted-foreground">{t('clientKey')}</div>
                <div>{approval.client_key_label || '-'}</div>
              </div>
              <div>
                <div className="text-muted-foreground">{t('model')}</div>
                <div>{approval.model || '-'}</div>
              </div>
              <div>
                <div className="text-muted-foreground">{t('status')}</div>
                <div>
                  <Badge
                    className={
                      isPending
                        ? 'border-amber-600/40 text-amber-700 dark:text-amber-300'
                        : undefined
                    }
                    variant={isPending ? 'outline' : 'secondary'}
                  >
                    {statusLabel()}
                  </Badge>
                </div>
              </div>
            </div>
            <div className="grid gap-2">
              <div className="text-muted-foreground">{t('reviewReason')}</div>
              <div className="rounded border bg-muted px-3 py-2">
                {approval.review_reason || '-'}
              </div>
            </div>
            <div className="grid gap-2">
              <div className="text-muted-foreground">
                {t('reviewCategories')}
              </div>
              <div className="flex flex-wrap gap-2">
                {approval.review_categories.map((category) => (
                  <Badge key={category} variant="secondary">
                    {category}
                  </Badge>
                ))}
                {approval.review_categories.length === 0 && (
                  <span className="text-muted-foreground">-</span>
                )}
              </div>
            </div>
            <div className="grid gap-2">
              <div className="text-muted-foreground">{t('requestPreview')}</div>
              <pre className="whitespace-pre-wrap">
                {approval.request_preview}
              </pre>
            </div>
            {approval.request_payload_json ? (
              <div className="grid gap-2">
                <div className="text-muted-foreground">
                  {t('reviewPayload')}
                </div>
                <pre className="max-h-80 overflow-auto">
                  {payloadText(approval.request_payload_json)}
                </pre>
              </div>
            ) : null}
            <div className="grid gap-2 sm:grid-cols-2">
              <div>
                <span className="text-muted-foreground">{t('createdAt')}</span>{' '}
                {formatDateTime(
                  approval.created_at,
                  locale,
                  undefined,
                  timeZone.effective,
                )}
              </div>
              <div>
                <span className="text-muted-foreground">{t('decidedAt')}</span>{' '}
                {approval.decided_at
                  ? formatDateTime(
                      approval.decided_at,
                      locale,
                      undefined,
                      timeZone.effective,
                    )
                  : '-'}
              </div>
            </div>
          </div>
        ) : null}
        <DialogFooter>
          <Button
            disabled={pending}
            onClick={() => handleOpenChange(false)}
            size="sm"
            type="button"
            variant="ghost"
          >
            {t('cancel')}
          </Button>
          {isPending && (
            <Button
              data-approval-action="reject"
              disabled={pending}
              onClick={() => approval && onReject(approval)}
              size="sm"
              type="button"
              variant="outline"
            >
              {action === 'reject' ? (
                <LoaderCircle
                  aria-hidden="true"
                  className="size-4 animate-spin"
                />
              ) : null}
              {t('reject')}
            </Button>
          )}
          {isPending && (
            <Button
              data-approval-action="approve"
              disabled={pending}
              onClick={() => approval && onApprove(approval)}
              size="sm"
              type="button"
            >
              {action === 'approve' ? (
                <LoaderCircle
                  aria-hidden="true"
                  className="size-4 animate-spin"
                />
              ) : null}
              {t('approve')}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
