import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { RefreshCw } from 'lucide-react'
import type {
  ApprovalRequest,
  ApprovalStatusFilter,
} from '@/generated/admin-api'
import {
  approveApprovalMutation,
  getApprovalOptions,
  listApprovalsOptions,
  rejectApprovalMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { ApprovalDetailDialog } from '@/components/approvals/ApprovalDetailDialog'
import { ApprovalsPanel } from '@/components/approvals/ApprovalsPanel'
import { useStoredPageSize } from '@/components/shared/TablePagination'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import {
  approvalActionKey,
  approvalsIdentityKey,
  APPROVAL_PAGE_SIZE_FALLBACK,
  refreshApprovalDetail,
  refreshApprovalList,
} from '@/lib/approvals'
import { useAsyncAction } from '@/lib/use-async-action'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { usePageTitle } from '@/lib/router'
import { useSessionStore } from '@/lib/session'
import {
  APPROVAL_PAGE_SIZE_OPTIONS,
  type TablePageChange,
} from '@/models/pagination'

export function ApprovalsPage() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const { notifyApiError, notifySuccess } = useNotifier()
  const asyncAction = useAsyncAction()
  const me = useSessionStore((state) => state.me)
  const identity = {
    user_id: me?.user_id ?? null,
    is_admin: me?.is_admin === true,
  }
  const identityKey = approvalsIdentityKey(identity)
  const initializedFor = useRef<string | null>(null)
  const notifiedErrors = useRef(new WeakSet<object>())
  const [status, setStatus] = useState<ApprovalStatusFilter>('pending')
  const [first, setFirst] = useState(0)
  const [rows, setRows] = useStoredPageSize(
    'approvals',
    APPROVAL_PAGE_SIZE_FALLBACK,
    APPROVAL_PAGE_SIZE_OPTIONS,
  )
  const [requestedDetailId, setRequestedDetailId] = useState<string | null>(
    null,
  )
  const firstRef = useRef(first)
  const rowsRef = useRef(rows)
  const requestedDetailIdRef = useRef(requestedDetailId)

  firstRef.current = first
  rowsRef.current = rows
  requestedDetailIdRef.current = requestedDetailId

  const listQuery = useQuery({
    ...listApprovalsOptions({ query: { status, first, rows } }),
    enabled: me !== null,
  })
  const detailQuery = useQuery({
    ...getApprovalOptions({
      path: { approval_id: requestedDetailId ?? '' },
    }),
    enabled: requestedDetailId !== null,
  })

  const approveMutation = useMutation(approveApprovalMutation())
  const rejectMutation = useMutation(rejectApprovalMutation())

  usePageTitle('approvals')

  function reportApiError(cause: unknown): void {
    if (cause && typeof cause === 'object') {
      if (notifiedErrors.current.has(cause)) return
      notifiedErrors.current.add(cause)
    }
    notifyApiError(cause)
  }

  useEffect(() => {
    if (listQuery.error) reportApiError(listQuery.error)
  }, [listQuery.error])

  useEffect(() => {
    if (detailQuery.error && requestedDetailId) {
      reportApiError(detailQuery.error)
      setRequestedDetailId(null)
    }
  }, [detailQuery.error, requestedDetailId])

  useEffect(() => {
    if (!me) return
    if (initializedFor.current !== identityKey) {
      initializedFor.current = identityKey
      setStatus('pending')
      setFirst(0)
      setRequestedDetailId(null)
    }
  }, [identityKey, me])

  useEffect(() => {
    const page = listQuery.data
    if (!page || page.rows <= 0) return
    if (page.approvals.length === 0 && page.first >= page.total) {
      const correctedFirst =
        page.total > 0
          ? Math.floor((page.total - 1) / page.rows) * page.rows
          : 0
      if (correctedFirst !== firstRef.current) {
        firstRef.current = correctedFirst
        setFirst(correctedFirst)
      }
      return
    }
    if (page.first !== firstRef.current) {
      firstRef.current = page.first
      setFirst(page.first)
    }
    if (page.rows !== rowsRef.current) {
      rowsRef.current = page.rows
      setRows(page.rows)
    }
  }, [listQuery.data, setRows])

  async function refresh(): Promise<void> {
    await listQuery.refetch()
  }

  function changeStatus(next: ApprovalStatusFilter): void {
    setStatus(next)
    setFirst(0)
  }

  function changePage(change: TablePageChange): void {
    firstRef.current = change.first
    rowsRef.current = change.rows
    setFirst(change.first)
    setRows(change.rows)
  }

  function openDetail(approval: ApprovalRequest): void {
    setRequestedDetailId(approval.approval_id)
  }

  function closeDetail(): void {
    setRequestedDetailId(null)
  }

  async function answer(
    approvalId: string,
    action: 'approve' | 'reject',
  ): Promise<void> {
    const capturedId = approvalId
    const capturedStatus = status
    const capturedFirst = firstRef.current
    const capturedRows = rowsRef.current
    try {
      const outcome = await asyncAction.run(
        approvalActionKey(capturedId),
        async () => {
          if (action === 'approve') {
            await approveMutation.mutateAsync({
              path: { approval_id: capturedId },
            })
          } else {
            await rejectMutation.mutateAsync({
              path: { approval_id: capturedId },
            })
          }
          await refreshApprovalList(
            queryClient,
            capturedStatus,
            capturedFirst,
            capturedRows,
          )
          if (requestedDetailIdRef.current === capturedId) {
            try {
              await refreshApprovalDetail(queryClient, capturedId)
            } catch (cause) {
              if (requestedDetailIdRef.current === capturedId) {
                setRequestedDetailId(null)
              }
              throw cause
            }
          }
        },
        action,
      )
      if (!outcome.executed) return
      notifySuccess(
        t(action === 'approve' ? 'approvalApproved' : 'approvalRejected'),
      )
    } catch (cause) {
      reportApiError(cause)
    }
  }

  const approvals = listQuery.data?.approvals ?? []
  const total = listQuery.data?.total ?? 0
  const loading = listQuery.isPending

  return (
    <section
      className="grid min-w-0 max-w-full gap-3"
      data-foundation-route-id="R-APPROVALS"
    >
      <HeaderActions>
        <Button
          aria-label={t('refresh')}
          disabled={listQuery.isFetching}
          onClick={() => void refresh()}
          size="sm"
          type="button"
          variant="outline"
        >
          {listQuery.isFetching ? (
            <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
          ) : (
            <RefreshCw aria-hidden="true" />
          )}
          {t('refresh')}
        </Button>
      </HeaderActions>

      <ApprovalsPanel
        approvals={approvals}
        first={listQuery.data?.first ?? first}
        loading={loading}
        onApprove={(approval) => void answer(approval.approval_id, 'approve')}
        onOpenDetail={openDetail}
        onPage={changePage}
        onReject={(approval) => void answer(approval.approval_id, 'reject')}
        onStatusChange={changeStatus}
        pendingActions={asyncAction.pendingActions}
        rows={listQuery.data?.rows ?? rows}
        status={status}
        total={total}
      />

      <ApprovalDetailDialog
        approval={
          requestedDetailId !== null ? (detailQuery.data ?? null) : null
        }
        loading={requestedDetailId !== null && detailQuery.isFetching}
        onApprove={(approval) => void answer(approval.approval_id, 'approve')}
        onOpenChange={(open) => {
          if (!open) closeDetail()
        }}
        onReject={(approval) => void answer(approval.approval_id, 'reject')}
        open={requestedDetailId !== null}
        pendingActions={asyncAction.pendingActions}
      />
    </section>
  )
}
