import { useEffect, useMemo, useRef, useState } from 'react'
import { Clock, LoaderCircle } from 'lucide-react'
import type {
  ApprovalRequest,
  ApprovalStatusFilter,
} from '@/generated/admin-api'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
import { TablePagination } from '@/components/shared/TablePagination'
import { WorkspacePagerBar } from '@/components/shared/WorkspacePagerBar'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
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
  const callbacks = useRef({ onApprove, onOpenDetail, onReject })
  callbacks.current = { onApprove, onOpenDetail, onReject }

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
          onClick={() => callbacks.current.onOpenDetail(approval)}
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
            onClick={() => callbacks.current.onReject(approval)}
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
            onClick={() => callbacks.current.onApprove(approval)}
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

  function renderCard(approval: ApprovalRequest) {
    return (
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
    )
  }

  const columns = useMemo<Array<DataTableColumnDef<ApprovalRequest>>>(
    () => [
      {
        id: 'user',
        header: t('user'),
        cell: ({ row }) => (
          <div className="max-w-48 break-words">
            {row.original.user_login_name || '-'}
          </div>
        ),
      },
      {
        id: 'client-key',
        header: t('clientKey'),
        cell: ({ row }) => (
          <div className="max-w-48 break-words">
            {row.original.client_key_label || '-'}
          </div>
        ),
      },
      {
        id: 'model',
        header: t('model'),
        cell: ({ row }) => (
          <div className="max-w-56 break-words">
            {row.original.model || '-'}
          </div>
        ),
      },
      {
        id: 'review-reason',
        header: t('reviewReason'),
        cell: ({ row }) => (
          <div className="max-w-64">
            <Button
              className="max-w-full truncate"
              data-approval-reason={row.original.approval_id}
              onClick={() => callbacks.current.onOpenDetail(row.original)}
              size="sm"
              type="button"
              variant="link"
            >
              {row.original.review_reason || '-'}
            </Button>
          </div>
        ),
      },
      {
        id: 'created-at',
        header: t('createdAt'),
        cell: ({ row }) => (
          <div className="whitespace-nowrap">{formatCreated(row.original)}</div>
        ),
      },
      {
        id: 'remaining-wait',
        header: t('remainingWait'),
        cell: ({ row }) => (
          <div className="flex items-center gap-2 whitespace-nowrap">
            <Clock
              aria-hidden="true"
              className="size-4 text-muted-foreground"
            />
            <span data-approval-remaining={row.original.approval_id}>
              {formatRemaining(row.original)}
            </span>
          </div>
        ),
      },
      {
        id: 'status',
        header: t('status'),
        cell: ({ row }) => (
          <Badge
            className={statusClass(row.original)}
            variant={statusVariant(row.original)}
          >
            {statusLabel(row.original)}
          </Badge>
        ),
      },
      {
        id: 'actions',
        header: () => <span className="sr-only">{t('viewDetails')}</span>,
        cell: ({ row }) => renderActions(row.original),
      },
    ],
    [locale, now, pendingActions, t, timeZone.effective],
  )

  return (
    <section aria-busy={loading} className="grid min-w-0 max-w-full gap-3">
      <div className="flex flex-wrap items-center justify-between gap-2 rounded-xl border bg-card px-3 py-3 md:hidden">
        <Badge variant="secondary">{`${pageStart}-${pageEnd} / ${total}`}</Badge>
        {renderFilter()}
      </div>

      <div className="hidden flex-wrap items-center justify-between gap-2 border-b p-3 md:flex">
        <div className="text-xs text-muted-foreground">
          {`${pageStart}-${pageEnd} / ${total}`}
        </div>
        {renderFilter()}
      </div>

      <DataTable
        className="min-w-[64rem]"
        columns={columns}
        data={approvals}
        emptyLabel={t('noApprovals')}
        getRowId={(approval) => approval.approval_id}
        label={t('approvals')}
        loading={loading}
        renderCard={renderCard}
        rowAttributes={(approval) => ({
          'data-approval-row': approval.approval_id,
        })}
      />

      <div className="md:hidden" data-approval-mobile-pager>
        {total > rows && (
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
        )}
      </div>

      <div className="hidden md:block" data-approval-desktop-pager>
        <TablePagination
          first={first}
          onChange={onPage}
          pageSizeOptions={APPROVAL_PAGE_SIZE_OPTIONS}
          rows={rows}
          total={total}
        />
      </div>
    </section>
  )
}
