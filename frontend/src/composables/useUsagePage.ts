import { computed, defineAsyncComponent, onMounted, ref, watch } from 'vue'

import { createDefaultRequestRecordClearForm } from '@/admin-mappers'
import { createRequestRecordFormatting } from '@/composables/useUsageFormatting'
import { useLocale } from '@/composables/useLocale'
import { useNotifier } from '@/composables/useNotifier'
import { useRequestRecordsPageActions } from '@/composables/useRequestRecordsPageActions'
import { useSessionRoutingActions } from '@/composables/useSessionRoutingActions'
import { useRequestOverviewMode } from '@/composables/useRequestOverviewMode'
import type { RequestRecordClearForm, RequestRecordRowView } from '@/models'
import type { RequestRecordOverviewRange } from '@/generated/admin-api'
import type { RequestOverviewDrilldown } from '@/request-overview'
import { useSessionStore } from '@/stores/session'
import { useRequestRecordsStore } from '@/stores/usage'
import { useUsersStore } from '@/stores/users'

export function useUsagePage() {
  const { t } = useLocale()
  const { notifyApiError, notifyInfo, notifySuccess } = useNotifier()
  const session = useSessionStore()
  const requestRecordsStore = useRequestRecordsStore()
  const usersStore = useUsersStore()

  const detailVisible = ref(false)
  const clearDialogVisible = ref(false)
  const clearForm = ref<RequestRecordClearForm>(
    createDefaultRequestRecordClearForm(),
  )
  const UsageClearDialog = defineAsyncComponent(
    () => import('@/components/usage/UsageClearDialog.vue'),
  )
  const pageActions = useRequestRecordsPageActions({
    store: requestRecordsStore,
    clearDialogVisible,
    clearForm,
    notifyApiError,
    notifySuccess,
    t,
  })
  const routingActions = useSessionRoutingActions({
    store: requestRecordsStore,
    confirm: (message) => window.confirm(message),
    notifyApiError,
    notifyInfo,
    notifySuccess,
    t,
  })
  const { activeMode, setActiveMode, syncMode } = useRequestOverviewMode(
    () => requestRecordsStore.requestCategory,
  )
  const formatting = computed(() => createRequestRecordFormatting(t))

  async function refresh(): Promise<void> {
    try {
      await Promise.all([
        requestRecordsStore.refreshAll(),
        session.isAdmin ? usersStore.loadUsers() : Promise.resolve(),
      ])
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function refreshRecords(): Promise<void> {
    try {
      await requestRecordsStore.refreshRecords()
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function applyRange(input: {
    range: RequestRecordOverviewRange
    start?: string
    end?: string
  }): Promise<void> {
    requestRecordsStore.range = input.range
    requestRecordsStore.start = input.start ?? ''
    requestRecordsStore.end = input.end ?? ''
    try {
      await Promise.all([
        requestRecordsStore.refreshOverview(),
        requestRecordsStore.refreshRecords(),
        requestRecordsStore.refreshFacets(),
      ])
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function openDetail(record: RequestRecordRowView): Promise<void> {
    requestRecordsStore.resetDetail()
    detailVisible.value = true
    try {
      await requestRecordsStore.loadDetail(record.record_id)
    } catch (cause) {
      detailVisible.value = false
      notifyApiError(cause)
    }
  }

  async function loadDetailRequestFull(): Promise<void> {
    const recordId = requestRecordsStore.detailRecord?.record_id
    if (!recordId) return
    try {
      await requestRecordsStore.loadRequestFull(recordId)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function loadMoreDetailRequestFull(): Promise<void> {
    const recordId = requestRecordsStore.detailRecord?.record_id
    if (!recordId) return
    try {
      await requestRecordsStore.loadMoreRequestFull(recordId)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function onPage(event: TablePageChange): Promise<void> {
    await requestRecordsStore.refreshRecords(event.first, event.rows)
  }

  async function onSort(event: TableSortChange): Promise<void> {
    requestRecordsStore.sortField = String(event.sortField || 'created_at')
    requestRecordsStore.sortOrder = Number(event.sortOrder || -1) as -1 | 0 | 1
    await refreshRecords()
  }

  async function onFilter(): Promise<void> {
    await refreshRecords()
  }

  async function handleOverviewDrilldown(
    filter: RequestOverviewDrilldown,
  ): Promise<void> {
    requestRecordsStore.applyDrilldown(filter)
    setActiveMode('records')
    await refreshRecords()
  }

  watch(detailVisible, (visible) => {
    if (!visible) requestRecordsStore.resetDetail()
  })

  watch(
    () => requestRecordsStore.requestCategory,
    async (_category, previous) => {
      syncMode(requestRecordsStore.requestCategory)
      if (previous == null) return
      await refresh()
    },
  )

  onMounted(async () => {
    syncMode(requestRecordsStore.requestCategory)
    await refresh()
  })

  return {
    ...pageActions,
    ...routingActions,
    UsageClearDialog,
    activeMode,
    applyRange,
    clearDialogVisible,
    clearForm,
    detailVisible,
    formatting,
    handleOverviewDrilldown,
    loadDetailRequestFull,
    loadMoreDetailRequestFull,
    onFilter,
    onPage,
    onSort,
    openDetail,
    refresh,
    refreshRecords,
    requestRecordsStore,
    session,
    setActiveMode,
    t,
    usersStore,
  }
}
