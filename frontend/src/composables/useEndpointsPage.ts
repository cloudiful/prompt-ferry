import { computed, onMounted } from 'vue'

import { useEndpointPageActions } from '@/composables/useEndpointPageActions'
import { useEndpointTokenPlanUsage } from '@/composables/useEndpointTokenPlanUsage'
import { useLocale } from '@/composables/useLocale'
import { useNotifier } from '@/composables/useNotifier'
import { useEndpointsStore } from '@/stores/endpoints'
import { useUsersStore } from '@/stores/users'

export function useEndpointsPage() {
  const { t } = useLocale()
  const { notifyApiError, notifySuccess } = useNotifier()
  const endpointsStore = useEndpointsStore()
  const usersStore = useUsersStore()

  // The fetch-only busy flag. A write never raises it, so saving one record
  // does not blank the page or spin controls it has nothing to do with.
  const busy = computed(() => endpointsStore.loading)
  const endpointPendingActions = computed(
    () => endpointsStore.endpointPendingActions,
  )
  const modelRoutePendingActions = computed(
    () => endpointsStore.modelRoutePendingActions,
  )

  const dialogs = useEndpointPageActions({
    confirm: (message) => window.confirm(message),
    notifyApiError,
    notifySuccess,
    store: endpointsStore,
    t,
  })
  const tokenPlanUsage = useEndpointTokenPlanUsage(
    endpointsStore.findEndpointById,
    notifyApiError,
  )

  async function refresh(): Promise<void> {
    try {
      await Promise.all([endpointsStore.refresh(), usersStore.loadUsers()])
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function onEndpointPage(event: TablePageChange): Promise<void> {
    await endpointsStore.loadEndpoints(event.first, event.rows)
  }

  async function onModelRoutePage(event: TablePageChange): Promise<void> {
    await endpointsStore.loadModelRoutes(event.first, event.rows)
  }

  onMounted(async () => {
    await refresh()
  })

  return {
    busy,
    endpointPendingActions,
    endpointsStore,
    modelRoutePendingActions,
    onEndpointPage,
    onModelRoutePage,
    refresh,
    runEndpointOAuthAction: endpointsStore.runEndpointOAuthAction,
    t,
    usersStore,
    ...tokenPlanUsage,
    ...dialogs,
  }
}
