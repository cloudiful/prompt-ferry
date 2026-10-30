import { useEffect, useRef } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { RefreshCw } from 'lucide-react'
import type {
  ApprovalRequest,
  ApprovalStatusFilter,
} from '@/generated/admin-api'
import { ApprovalDetailDialog } from '@/components/approvals/ApprovalDetailDialog'
import { ApprovalsPanel } from '@/components/approvals/ApprovalsPanel'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { usePageTitle } from '@/lib/router'
import { useSessionStore } from '@/lib/session'
import { useAsyncAction } from '@/lib/use-async-action'
import { useApprovalsStore } from '@/lib/approvals'
import type { TablePageChange } from '@/models/pagination'

export function ApprovalsPage() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const { notifyApiError, notifySuccess } = useNotifier()
  const asyncAction = useAsyncAction()
  const me = useSessionStore((state) => state.me)
  const initializedFor = useRef<string | null>(null)

  const approvals = useApprovalsStore((state) => state.approvals)
  const total = useApprovalsStore((state) => state.total)
  const first = useApprovalsStore((state) => state.first)
  const rows = useApprovalsStore((state) => state.rows)
  const loading = useApprovalsStore((state) => state.loading)
  const status = useApprovalsStore((state) => state.status)
  const detail = useApprovalsStore((state) => state.detail)
  const detailId = useApprovalsStore((state) => state.detailId)
  const detailLoading = useApprovalsStore((state) => state.detailLoading)

  usePageTitle('approvals')

  useEffect(() => {
    if (!me) return
    const store = useApprovalsStore.getState()
    store.initializeIdentity({
      user_id: me.user_id,
      is_admin: me.is_admin === true,
    })
    const current = useApprovalsStore.getState()
    if (initializedFor.current === current.identityKey) return
    initializedFor.current = current.identityKey
    void refresh()
  }, [me])

  async function refresh(): Promise<void> {
    try {
      await useApprovalsStore.getState().refresh(queryClient)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  function changeStatus(next: ApprovalStatusFilter): void {
    useApprovalsStore.getState().setStatus(next)
    void useApprovalsStore
      .getState()
      .loadList(queryClient)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  function changePage(change: TablePageChange): void {
    useApprovalsStore.getState().setPage(change.first, change.rows)
    void useApprovalsStore
      .getState()
      .loadList(queryClient)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  async function openDetail(approval: ApprovalRequest): Promise<void> {
    const approvalId = approval.approval_id
    try {
      await useApprovalsStore.getState().openDetail(queryClient, approvalId)
    } catch (cause) {
      if (useApprovalsStore.getState().detailId === approvalId) {
        useApprovalsStore.getState().closeDetail()
      }
      notifyApiError(cause)
    }
  }

  async function approve(approval: ApprovalRequest): Promise<void> {
    try {
      const outcome = await useApprovalsStore
        .getState()
        .approve(queryClient, approval.approval_id)
      // A refused call is the guard collapsing a duplicate trigger: no toast.
      if (outcome.executed) notifySuccess(t('approvalApproved'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function reject(approval: ApprovalRequest): Promise<void> {
    try {
      const outcome = await useApprovalsStore
        .getState()
        .reject(queryClient, approval.approval_id)
      if (outcome.executed) notifySuccess(t('approvalRejected'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  return (
    <section
      className="grid min-w-0 max-w-full gap-3"
      data-foundation-route-id="R-APPROVALS"
    >
      <HeaderActions>
        <Button
          aria-label={t('refresh')}
          disabled={loading}
          onClick={() => void refresh()}
          size="sm"
          type="button"
          variant="outline"
        >
          {loading ? (
            <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
          ) : (
            <RefreshCw aria-hidden="true" />
          )}
          {t('refresh')}
        </Button>
      </HeaderActions>

      <ApprovalsPanel
        approvals={approvals}
        first={first}
        loading={loading}
        onApprove={(approval) => void approve(approval)}
        onOpenDetail={(approval) => void openDetail(approval)}
        onPage={changePage}
        onReject={(approval) => void reject(approval)}
        onStatusChange={changeStatus}
        pendingActions={asyncAction.pendingActions}
        rows={rows}
        status={status}
        total={total}
      />

      <ApprovalDetailDialog
        approval={detail}
        loading={detailLoading}
        onApprove={(approval) => void approve(approval)}
        onOpenChange={(open) => {
          if (!open) useApprovalsStore.getState().closeDetail()
        }}
        onReject={(approval) => void reject(approval)}
        open={detailId !== null}
        pendingActions={asyncAction.pendingActions}
      />
    </section>
  )
}
