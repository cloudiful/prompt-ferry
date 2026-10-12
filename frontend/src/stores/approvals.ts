import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  APPROVAL_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'
import { getApproval, listApprovals } from '../generated/admin-api'
import type {
  ApprovalRequest,
  ApprovalStatusFilter,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import { useAsyncAction } from '../composables/useAsyncAction'
import { APPROVAL_KEY_PREFIX, createApprovalActions } from './approvals-actions'
import { pendingRowsById } from './record-action-types'

export const useApprovalsStore = defineStore('approvals', () => {
  const approvals = ref<ApprovalRequest[]>([])
  const loading = ref(false)
  const detailLoading = ref(false)
  const currentFilter = ref<ApprovalStatusFilter>('pending')
  const detailApproval = ref<ApprovalRequest | null>(null)
  const first = ref(0)
  const rows = useStoredPageSize('approvals', 10, APPROVAL_PAGE_SIZE_OPTIONS)
  const total = ref(0)

  async function refresh(
    nextFilter = currentFilter.value,
    nextFirst = first.value,
    nextRows = rows.value,
  ): Promise<void> {
    currentFilter.value = nextFilter
    first.value = nextFirst
    rows.value = nextRows
    loading.value = true
    try {
      const page = expectData(
        await listApprovals<true>(
          withData({
            query: {
              status: currentFilter.value,
              first: first.value,
              rows: rows.value,
            },
          }),
        ),
      )
      approvals.value = page.approvals
      total.value = page.total
      first.value = page.first
      rows.value = page.rows
      if (
        page.approvals.length === 0 &&
        page.total > 0 &&
        page.first >= page.total
      ) {
        const previousFirst =
          Math.floor((page.total - 1) / page.rows) * page.rows
        await refresh(currentFilter.value, previousFirst, page.rows)
      }
    } finally {
      loading.value = false
    }
  }

  async function loadDetail(approvalId: string): Promise<ApprovalRequest> {
    detailLoading.value = true
    try {
      const detail = expectData(
        await getApproval<true>(
          withData({ path: { approval_id: approvalId } }),
        ),
      )
      detailApproval.value = detail
      return detail
    } finally {
      detailLoading.value = false
    }
  }

  // Approve and reject of one request share a gate, so the two answers to the
  // same question cannot both be sent.
  const guard = useAsyncAction()
  const actions = createApprovalActions({
    detailApproval,
    guard,
    loadDetail: (approvalId) => loadDetail(approvalId),
    refresh: () => refresh(),
  })

  // Per-row view of the guard. `loading` stays the fetch-only busy flag, so
  // answering one request no longer blanks the whole page.
  const approvalPendingActions = computed(() =>
    pendingRowsById(guard.pendingActions.value, APPROVAL_KEY_PREFIX),
  )

  return {
    ...actions,
    approvals,
    approvalPendingActions,
    currentFilter,
    detailApproval,
    detailLoading,
    first,
    isPending: guard.isPending,
    loadDetail,
    loading,
    pendingActions: guard.pendingActions,
    refresh,
    rows,
    total,
  }
})
