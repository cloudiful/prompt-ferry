import { defineStore } from 'pinia'
import { ref } from 'vue'
import {
  APPROVAL_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'
import {
  approveApproval,
  getApproval,
  listApprovals,
  rejectApproval,
} from '../generated/admin-api'
import type {
  ApprovalRequest,
  ApprovalStatusFilter,
} from '../generated/admin-api'
import { expectData, withData } from '../api'

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

  async function approve(approvalId: string): Promise<void> {
    loading.value = true
    try {
      await approveApproval<true>(
        withData({ path: { approval_id: approvalId } }),
      )
      await refresh()
      if (detailApproval.value?.approval_id === approvalId) {
        await loadDetail(approvalId)
      }
    } finally {
      loading.value = false
    }
  }

  async function reject(approvalId: string): Promise<void> {
    loading.value = true
    try {
      await rejectApproval<true>(
        withData({ path: { approval_id: approvalId } }),
      )
      await refresh()
      if (detailApproval.value?.approval_id === approvalId) {
        await loadDetail(approvalId)
      }
    } finally {
      loading.value = false
    }
  }

  return {
    approvals,
    approve,
    currentFilter,
    detailApproval,
    detailLoading,
    first,
    loadDetail,
    loading,
    refresh,
    reject,
    rows,
    total,
  }
})
