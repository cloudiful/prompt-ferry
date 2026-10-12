import type { Ref } from 'vue'

import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import type { ApprovalRequest } from '@/generated/admin-api'
import { approveApproval, rejectApproval } from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { recordActionKey } from '@/stores/record-action-types'

/**
 * Key namespace for the guarded approval writes.
 *
 * Approve and reject of one request share a gate. They are two answers to the
 * same question, so running both would put an approval into a state no operator
 * chose, and a held Enter plus a click must not be able to send two.
 */
export const APPROVAL_KEY_PREFIX = 'approval'

export function approvalActionKey(approvalId: string): string {
  return recordActionKey(APPROVAL_KEY_PREFIX, approvalId)
}

export type ApprovalActionDeps = {
  detailApproval: Ref<ApprovalRequest | null>
  guard: AsyncActionGuard
  loadDetail: (approvalId: string) => Promise<unknown>
  refresh: () => Promise<void>
}

/**
 * The two approval answers.
 *
 * The gate is held across the refresh and the detail reload that follow, so the
 * button that was pressed keeps its spinner until the list the operator is
 * looking at has actually settled.
 */
export function createApprovalActions(deps: ApprovalActionDeps) {
  const { detailApproval, guard, loadDetail, refresh } = deps

  /** Reload the open detail only when it is the record that was answered. */
  async function reloadDetail(approvalId: string): Promise<void> {
    if (detailApproval.value?.approval_id === approvalId) {
      await loadDetail(approvalId)
    }
  }

  function approve(approvalId: string): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      approvalActionKey(approvalId),
      async () => {
        await approveApproval<true>(
          withData({ path: { approval_id: approvalId } }),
        )
        await refresh()
        await reloadDetail(approvalId)
      },
      'approve',
    )
  }

  function reject(approvalId: string): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      approvalActionKey(approvalId),
      async () => {
        await rejectApproval<true>(
          withData({ path: { approval_id: approvalId } }),
        )
        await refresh()
        await reloadDetail(approvalId)
      },
      'reject',
    )
  }

  return { approve, reject }
}

export type ApprovalActions = ReturnType<typeof createApprovalActions>
