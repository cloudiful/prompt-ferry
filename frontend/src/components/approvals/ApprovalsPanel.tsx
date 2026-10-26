import { useEffect, useState } from 'react'
import { Clock, LoaderCircle } from 'lucide-react'
import type {
  ApprovalRequest,
  ApprovalStatusFilter,
} from '@/generated/admin-api'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { TablePagination } from '@/components/shared/TablePagination'
import { WorkspacePagerBar } from '@/components/shared/WorkspacePagerBar'
import type { PendingActions } from '@/lib/async-action'
import { approvalActionKey } from '@/lib/approvals'
import { useDisplayTimeZone } from '@/lib/display-timezone'
import { useLocale } from '@/lib/i18n'
import { formatDateTime } from '@/format/datetime-core'
import {
  APPROVAL_PAGE_SIZE_OPTIONS,
  type TablePageChange,
} from '@/models/pagination'

type ApprovalsPanelProps = {
  approvals: ApprovalRequest[]
  first: number
  loading: boolean
  onApprove: (approval: ApprovalRequest) => void
  onOpenDetail: (approval: ApprovalRequest) => void
  onPage: (change: TablePageChange) => void
  onReject: (approval: ApprovalRequest) => void
  onStatusChange: (status: ApprovalStatusFilter) => void
  pendingActions: PendingActions
  rows: number
  status: ApprovalStatusFilter
  total: number
}

export function ApprovalsPanel({
  approvals,
  first,
  loading,
  onApprove,
  onOpenDetail,
  onPage,
  onReject,
  onStatusChange,
  pendingActions,
  rows,
  status,
  total,
}: ApprovalsPanelProps) {
  const { locale, t } = useLocale()
  const timeZone = useDisplayTimeZone()
  const [now, setNow] = useState(() => Date.now())

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(timer)
  }, [])

  const canPrev = first > 0
  const canNext = first + rows < total
  const pageStart = total === 0 ? 0 : first + 1
  const pageEnd = Math.min(first + rows, total)

  function statusLabel(approval: ApprovalRequest): string {
    return approval.approval_status === 'pending'
      ? t('approvalStatusPending')
      : t('approvalStatusResolved')
  }

  function statusVariant(approval: ApprovalRequest): 'outline' | 'secondary' {
    return approval.approval_status === 'pending' ? 'outline' : 'secondary'
  }

  function statusClass(approval: ApprovalRequest): string | undefined {
    return approval.approval_status === 'pending'
      ? 'border-amber-600/40 text-amber-700 dark:text-amber-300'
      : undefined
  }

  function formatRemaining(approval: ApprovalRequest): string {
    if (approval.approval_status !== 'pending') return '-'
    const ms = Math.max(0, approval.wait_deadline_unix_ms - now)
    const seconds = Math.floor(ms / 1000)
    const minutes = Math.floor(seconds / 60)
    if (minutes > 0) return `${minutes}m ${seconds % 60}s`
    return `${seconds}s`
  }

  function formatCreated(approval: ApprovalRequest): string {
    return formatDateTime(
      approval.created_at,
      locale,
      undefined,
      timeZone.effective,
    )
  }

  function rowAction(approval: ApprovalRequest): string | null {
    return pendingActions[approvalActionKey(approval.approval_id)] ?? null
  }

  function renderActions(approval: ApprovalRequest, mobile = false) {
    const action = rowAction(approval)
    const gated = action !== null
    const pending = approval.approval_status === 'pending'
    return (
      <div
        className={
          mobile
            ? 'grid gap-2 [&>button]:w-full [&>button]:justify-center'
            : 'flex gap-2'
        }
        data-approval-actions={approval.approval_id}
      >
        <Button
          aria-label={t('viewDetails')}
          data-approval-action="details"
          onClick={() => onOpenDetail(approval)}
          size="sm"
          type="button"
          variant="outline"
        >
          {t('viewDetails')}
        </Button>
        {pending && (
          <Button
            aria-label={t('reject')}
            data-approval-action="reject"
            disabled={gated}
            onClick={() => onReject(approval)}
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
        {pending && (
          <Button
            aria-label={t('approve')}
            data-approval-action="approve"
            disabled={gated}
            onClick={() => onApprove(approval)}
            size="sm"
            type="button"
            variant="outline"
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
      </div>
    )
  }

  function renderFilter() {
    return (
      <Select
        onValueChange={(value) => onStatusChange(value as ApprovalStatusFilter)}
        value={status}
      >
        <SelectTrigger aria-label={t('approvals')} className="w-40" size="sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="pending">{t('pendingApprovals')}</SelectItem>
          <SelectItem value="resolved">{t('resolvedApprovals')}</SelectItem>
        </SelectContent>
      </Select>
    )
  }

  function mobilePage(direction: 'prev' | 'next'): void {
    const nextFirst =
      direction === 'prev' ? Math.max(0, first - rows) : first + rows
    onPage({
      first: nextFirst,
      page: Math.floor(nextFirst / rows),
      rows,
    })
  }

  return (
    <section className="grid min-w-0 max-w-full gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2 rounded-xl border bg-card px-3 py-3 md:hidden">
        <Badge variant="secondary">{`${pageStart}-${pageEnd} / ${total}`}</Badge>
        {renderFilter()}
      </div>

      <div className="grid gap-3 md:hidden" data-approval-mobile>
        {approvals.length === 0 ? (
          <div
            className="rounded-xl border bg-card px-4 py-6 text-sm text-muted-foreground"
            role={loading ? 'status' : undefined}
          >
            {loading ? t('loading') : t('noApprovals')}
          </div>
        ) : (
          approvals.map((approval) => (
            <article
              className="grid min-w-0 gap-3 rounded-lg border bg-card p-3"
              data-approval-row={approval.approval_id}
              key={approval.approval_id}
            >
              <div className="flex items-start justify-between gap-2">
                <div className="min-w-0">
                  <div className="text-sm font-semibold break-words">
                    {approval.model || '-'}
                  </div>
                  <div className="text-xs break-words text-muted-foreground">
                    {`${approval.user_login_name || '-'} / ${approval.client_key_label || '-'}`}
                  </div>
                </div>
                <Badge
                  className={statusClass(approval)}
                  variant={statusVariant(approval)}
                >
                  {statusLabel(approval)}
                </Badge>
              </div>
              <div className="grid gap-1.5">
                <div className="grid gap-px">
                  <div className="text-[0.7rem] font-bold tracking-wide text-muted-foreground uppercase">
                    {t('reviewReason')}
                  </div>
                  <div className="text-xs leading-[1.38] break-words">
                    {approval.review_reason || '-'}
                  </div>
                </div>
                <div className="grid gap-px">
                  <div className="text-[0.7rem] font-bold tracking-wide text-muted-foreground uppercase">
                    {t('createdAt')}
                  </div>
                  <div className="text-xs leading-[1.38] break-words">
                    {formatCreated(approval)}
                  </div>
                </div>
                <div className="grid gap-px">
                  <div className="text-[0.7rem] font-bold tracking-wide text-muted-foreground uppercase">
                    {t('remainingWait')}
                  </div>
                  <div
                    className="text-xs leading-[1.38]"
                    data-approval-remaining={approval.approval_id}
                  >
                    {formatRemaining(approval)}
                  </div>
                </div>
              </div>
              {renderActions(approval, true)}
            </article>
          ))
        )}
        {total > rows && (
          <div data-approval-mobile-pager>
            <WorkspacePagerBar
              canNext={canNext}
              canPrev={canPrev}
              currentPage={Math.floor(first / rows) + 1}
              end={pageEnd}
              onNext={() => mobilePage('next')}
              onPrev={() => mobilePage('prev')}
              start={pageStart}
              total={total}
            />
          </div>
        )}
      </div>

      <div className="hidden min-w-0 md:block" data-approval-desktop>
        <div className="flex flex-wrap items-center justify-between gap-2 border-b p-3">
          <div className="text-xs text-muted-foreground">
            {`${pageStart}-${pageEnd} / ${total}`}
          </div>
          {renderFilter()}
        </div>
        <div className="min-w-0 overflow-x-auto">
          <Table aria-busy={loading} className="min-w-[64rem]">
            <TableHeader>
              <TableRow>
                <TableHead>{t('user')}</TableHead>
                <TableHead>{t('clientKey')}</TableHead>
                <TableHead>{t('model')}</TableHead>
                <TableHead>{t('reviewReason')}</TableHead>
                <TableHead>{t('createdAt')}</TableHead>
                <TableHead>{t('remainingWait')}</TableHead>
                <TableHead>{t('status')}</TableHead>
                <TableHead>
                  <span className="sr-only">{t('viewDetails')}</span>
                </TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {approvals.length === 0 ? (
                <TableRow>
                  <TableCell
                    className="py-8 text-center text-muted-foreground"
                    colSpan={8}
                  >
                    {loading ? t('loading') : t('noApprovals')}
                  </TableCell>
                </TableRow>
              ) : (
                approvals.map((approval) => (
                  <TableRow
                    data-approval-row={approval.approval_id}
                    key={approval.approval_id}
                  >
                    <TableCell className="max-w-48 break-words">
                      {approval.user_login_name || '-'}
                    </TableCell>
                    <TableCell className="max-w-48 break-words">
                      {approval.client_key_label || '-'}
                    </TableCell>
                    <TableCell className="max-w-56 break-words">
                      {approval.model || '-'}
                    </TableCell>
                    <TableCell className="max-w-64">
                      <Button
                        className="max-w-full truncate"
                        data-approval-reason={approval.approval_id}
                        onClick={() => onOpenDetail(approval)}
                        size="sm"
                        type="button"
                        variant="link"
                      >
                        {approval.review_reason || '-'}
                      </Button>
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      {formatCreated(approval)}
                    </TableCell>
                    <TableCell className="whitespace-nowrap">
                      <div className="flex items-center gap-2">
                        <Clock
                          aria-hidden="true"
                          className="size-4 text-muted-foreground"
                        />
                        <span data-approval-remaining={approval.approval_id}>
                          {formatRemaining(approval)}
                        </span>
                      </div>
                    </TableCell>
                    <TableCell>
                      <Badge
                        className={statusClass(approval)}
                        variant={statusVariant(approval)}
                      >
                        {statusLabel(approval)}
                      </Badge>
                    </TableCell>
                    <TableCell>{renderActions(approval)}</TableCell>
                  </TableRow>
                ))
              )}
            </TableBody>
          </Table>
        </div>
        <div data-approval-desktop-pager>
          <TablePagination
            first={first}
            onChange={onPage}
            pageSizeOptions={APPROVAL_PAGE_SIZE_OPTIONS}
            rows={rows}
            total={total}
          />
        </div>
      </div>
    </section>
  )
}
