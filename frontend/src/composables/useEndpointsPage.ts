import { computed, onMounted, ref } from 'vue'

import {
  createEmptyEndpointForm,
  createEmptyModelRouteForm,
  endpointFormToRequest,
  endpointToForm,
  modelRouteFormToRequest,
  modelRouteToForm,
} from '@/admin-mappers'
import { useLocale } from '@/composables/useLocale'
import { useNotifier } from '@/composables/useNotifier'
import type { EndpointForm, ModelRouteForm } from '@/models'
import { useEndpointsStore } from '@/stores/endpoints'
import { useUsersStore } from '@/stores/users'

export function useEndpointsPage() {
  const { t } = useLocale()
  const { notifyApiError, notifySuccess } = useNotifier()
  const endpointsStore = useEndpointsStore()
  const usersStore = useUsersStore()

  const busy = computed(() => endpointsStore.loading)
  const endpointDialogVisible = ref(false)
  const modelRouteDialogVisible = ref(false)
  const endpointForm = ref<EndpointForm>(createEmptyEndpointForm())
  const modelRouteForm = ref<ModelRouteForm>(createEmptyModelRouteForm())
  const endpointDialogHeader = computed(() =>
    endpointForm.value.endpoint_id ? t('editEndpoint') : t('newEndpoint'),
  )
  const modelRouteDialogHeader = computed(() =>
    modelRouteForm.value.rule_id ? t('editModelRoute') : t('newModelRoute'),
  )

  async function refresh(): Promise<void> {
    try {
      await Promise.all([endpointsStore.refresh(), usersStore.loadUsers()])
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  function openEndpointDialog(): void {
    endpointForm.value = createEmptyEndpointForm()
    endpointDialogVisible.value = true
  }

  function editEndpoint(endpointId: string): void {
    const endpoint = endpointsStore.findEndpointById(endpointId)
    if (!endpoint) return
    endpointForm.value = endpointToForm(endpoint)
    endpointDialogVisible.value = true
  }

  async function saveEndpoint(): Promise<void> {
    try {
      await endpointsStore.saveEndpoint(
        endpointForm.value.endpoint_id || null,
        endpointFormToRequest(endpointForm.value),
      )
      endpointDialogVisible.value = false
      endpointForm.value = createEmptyEndpointForm()
      notifySuccess(t('endpointSaved'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function deleteEndpoint(endpointId: string): Promise<void> {
    const endpoint = endpointsStore.findEndpointById(endpointId)
    if (!endpoint) return
    if (!window.confirm(`${t('delete')} ${endpoint.name}?`)) return
    try {
      await endpointsStore.removeEndpoint(endpointId)
      notifySuccess(t('endpointDeleted'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function testEndpoint(endpointId: string): Promise<void> {
    try {
      await endpointsStore.runEndpointTest(endpointId)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function toggleEndpointEnabled(
    endpointId: string,
    enabled: boolean,
  ): Promise<void> {
    try {
      await endpointsStore.toggleEndpointEnabled(endpointId, enabled)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  function openModelRouteDialog(): void {
    modelRouteForm.value = createEmptyModelRouteForm()
    modelRouteDialogVisible.value = true
  }

  function editModelRoute(ruleId: string): void {
    const route = endpointsStore.findModelRouteById(ruleId)
    if (!route) return
    modelRouteForm.value = modelRouteToForm(route)
    modelRouteDialogVisible.value = true
  }

  async function saveModelRoute(): Promise<void> {
    try {
      await endpointsStore.saveModelRoute(
        modelRouteForm.value.rule_id || null,
        modelRouteFormToRequest(modelRouteForm.value),
      )
      modelRouteDialogVisible.value = false
      modelRouteForm.value = createEmptyModelRouteForm()
      notifySuccess(t('saved'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function deleteModelRoute(ruleId: string): Promise<void> {
    const route = endpointsStore.findModelRouteById(ruleId)
    if (!route) return
    if (!window.confirm(`${t('delete')} ${route.model_pattern}?`)) return
    try {
      await endpointsStore.removeModelRoute(ruleId)
      notifySuccess(t('saved'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function testModelRoute(ruleId: string): Promise<void> {
    try {
      await endpointsStore.runModelRouteTest(ruleId)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function toggleModelRouteEnabled(
    ruleId: string,
    enabled: boolean,
  ): Promise<void> {
    try {
      await endpointsStore.toggleModelRouteEnabled(ruleId, enabled)
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
    deleteEndpoint,
    deleteModelRoute,
    editEndpoint,
    editModelRoute,
    endpointDialogHeader,
    endpointDialogVisible,
    endpointForm,
    endpointsStore,
    modelRouteDialogHeader,
    modelRouteDialogVisible,
    modelRouteForm,
    onEndpointPage,
    onModelRoutePage,
    openEndpointDialog,
    openModelRouteDialog,
    refresh,
    saveEndpoint,
    saveModelRoute,
    t,
    testEndpoint,
    testModelRoute,
    toggleEndpointEnabled,
    toggleModelRouteEnabled,
    usersStore,
  }
}
