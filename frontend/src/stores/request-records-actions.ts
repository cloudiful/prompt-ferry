import { computed } from 'vue'

import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import type { RequestRecordsClearResponse } from '@/generated/admin-api'
import type { RequestRecordClearForm } from '@/models'
import { clearUsageHistory, pruneUsageHistory } from './usage-api'
import { capturePayload } from './record-action-types'

export const USAGE_MAINTENANCE_KEY = 'usage:maintenance'

export function createRequestRecordsActions(deps: {
  guard: AsyncActionGuard
  refreshAll: () => Promise<void>
}) {
  const { guard, refreshAll } = deps

  async function clearHistory(
    form: RequestRecordClearForm,
  ): Promise<AsyncActionOutcome<RequestRecordsClearResponse>> {
    const capturedForm = capturePayload(form)
    return guard.run(
      USAGE_MAINTENANCE_KEY,
      async () => {
        const result = await clearUsageHistory(capturedForm)
        await refreshAll()
        return result
      },
      'clear',
    )
  }

  async function pruneHistory(): Promise<AsyncActionOutcome<number>> {
    return guard.run(
      USAGE_MAINTENANCE_KEY,
      async () => {
        const deleted = await pruneUsageHistory()
        await refreshAll()
        return deleted
      },
      'prune',
    )
  }

  return {
    clearHistory,
    maintenanceAction: computed(() =>
      guard.pendingAction(USAGE_MAINTENANCE_KEY),
    ),
    maintenancePending: computed(() => guard.isPending(USAGE_MAINTENANCE_KEY)),
    pruneHistory,
  }
}
