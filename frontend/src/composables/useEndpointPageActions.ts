import { computed, ref } from 'vue'

import {
  createEmptyEndpointForm,
  createEmptyModelRouteForm,
  endpointFormToRequest,
  endpointToForm,
  modelRouteFormToRequest,
  modelRouteToForm,
} from '@/admin-mappers'
import { useLocale } from '@/composables/useLocale'
import { CREATE_KEY } from '@/stores/endpoint-actions'
import type { useEndpointsStore } from '@/stores/endpoints'
import type { EndpointForm, ModelRouteForm } from '@/models'

type EndpointsStore = ReturnType<typeof useEndpointsStore>

export type EndpointPageActionDeps = {
  confirm: (message: string) => boolean
  notifyApiError: (cause: unknown) => void
  notifySuccess: (message: string) => void
  store: EndpointsStore
  t: ReturnType<typeof useLocale>['t']
}

/**
 * The page-owned half of the endpoints workspace: the dialogs, the drafts they
 * edit, and the single completion each dialog is allowed to perform.
 *
 * Every operation here is a thin wrapper around the store's guarded action, and
 * it reports success only when that action reports it ran. A refused call — the
 * guard swallowing a held Enter and a click — therefore produces no toast, no
 * error, and no close, which is the whole point of the guard reaching the UI.
 */
export function useEndpointPageActions(deps: EndpointPageActionDeps) {
  const { confirm, notifyApiError, notifySuccess, store, t } = deps

  const endpointDialogVisible = ref(false)
  const modelRouteDialogVisible = ref(false)
  const endpointForm = ref<EndpointForm>(createEmptyEndpointForm())
  const modelRouteForm = ref<ModelRouteForm>(createEmptyModelRouteForm())

  // A dialog is identified by the generation it was opened in. Every open, edit,
  // and successful save bumps it, so a save that settles late can tell that the
  // surface it was started from is no longer the one on screen and must not
  // close it.
  let endpointGeneration = 0
  let modelRouteGeneration = 0

  const endpointDialogHeader = computed(() =>
    endpointForm.value.endpoint_id ? t('editEndpoint') : t('newEndpoint'),
  )
  const modelRouteDialogHeader = computed(() =>
    modelRouteForm.value.rule_id ? t('editModelRoute') : t('newModelRoute'),
  )

  // The dialog's own write. It follows the record the draft belongs to, so a
  // write against some other record never freezes or spins this dialog. An
  // unsaved draft is keyed the same way the row map names a create.
  const endpointSavePending = computed(() =>
    Object.hasOwn(
      store.endpointPendingActions,
      endpointForm.value.endpoint_id || CREATE_KEY,
    ),
  )
  const modelRouteSavePending = computed(() =>
    Object.hasOwn(
      store.modelRoutePendingActions,
      modelRouteForm.value.rule_id || CREATE_KEY,
    ),
  )

  function openEndpointDialog(): void {
    endpointGeneration += 1
    endpointForm.value = createEmptyEndpointForm()
    endpointDialogVisible.value = true
  }

  function editEndpoint(endpointId: string): void {
    const endpoint = store.findEndpointById(endpointId)
    if (!endpoint) return
    endpointGeneration += 1
    endpointForm.value = endpointToForm(endpoint)
    endpointDialogVisible.value = true
  }

  async function saveEndpoint(): Promise<void> {
    const generation = endpointGeneration
    const endpointId = endpointForm.value.endpoint_id || null
    // Built from the draft as it stands before the request is claimed, so a
    // later edit cannot change what this save sends.
    const body = endpointFormToRequest(endpointForm.value)
    try {
      const result = await store.saveEndpoint(endpointId, body)
      if (!result.executed) return
      if (endpointGeneration === generation) {
        endpointDialogVisible.value = false
        endpointForm.value = createEmptyEndpointForm()
        endpointGeneration += 1
      }
      notifySuccess(t('endpointSaved'))
    } catch (cause) {
      // The dialog and its draft stay as they are so the save can be retried.
      notifyApiError(cause)
    }
  }

  async function deleteEndpoint(endpointId: string): Promise<void> {
    const endpoint = store.findEndpointById(endpointId)
    if (!endpoint) return
    // A cancelled confirmation is not an action: no request, no pending key,
    // no spinner, no toast.
    if (!confirm(`${t('delete')} ${endpoint.name}?`)) return
    try {
      const result = await store.removeEndpoint(endpointId)
      if (!result.executed) return
      notifySuccess(t('endpointDeleted'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function testEndpoint(endpointId: string): Promise<void> {
    try {
      await store.runEndpointTest(endpointId)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function toggleEndpointEnabled(
    endpointId: string,
    enabled: boolean,
  ): Promise<void> {
    try {
      await store.toggleEndpointEnabled(endpointId, enabled)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  function openModelRouteDialog(): void {
    modelRouteGeneration += 1
    modelRouteForm.value = createEmptyModelRouteForm()
    modelRouteDialogVisible.value = true
  }

  function editModelRoute(ruleId: string): void {
    const route = store.findModelRouteById(ruleId)
    if (!route) return
    modelRouteGeneration += 1
    modelRouteForm.value = modelRouteToForm(route)
    modelRouteDialogVisible.value = true
  }

  async function saveModelRoute(): Promise<void> {
    const generation = modelRouteGeneration
    const ruleId = modelRouteForm.value.rule_id || null
    const body = modelRouteFormToRequest(modelRouteForm.value)
    try {
      const result = await store.saveModelRoute(ruleId, body)
      if (!result.executed) return
      if (modelRouteGeneration === generation) {
        modelRouteDialogVisible.value = false
        modelRouteForm.value = createEmptyModelRouteForm()
        modelRouteGeneration += 1
      }
      notifySuccess(t('saved'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function deleteModelRoute(ruleId: string): Promise<void> {
    const route = store.findModelRouteById(ruleId)
    if (!route) return
    if (!confirm(`${t('delete')} ${route.model_pattern}?`)) return
    try {
      const result = await store.removeModelRoute(ruleId)
      if (!result.executed) return
      notifySuccess(t('saved'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function testModelRoute(ruleId: string): Promise<void> {
    try {
      await store.runModelRouteTest(ruleId)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function toggleModelRouteEnabled(
    ruleId: string,
    enabled: boolean,
  ): Promise<void> {
    try {
      await store.toggleModelRouteEnabled(ruleId, enabled)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  return {
    deleteEndpoint,
    deleteModelRoute,
    editEndpoint,
    editModelRoute,
    endpointDialogHeader,
    endpointDialogVisible,
    endpointForm,
    endpointSavePending,
    modelRouteDialogHeader,
    modelRouteDialogVisible,
    modelRouteForm,
    modelRouteSavePending,
    openEndpointDialog,
    openModelRouteDialog,
    saveEndpoint,
    saveModelRoute,
    testEndpoint,
    testModelRoute,
    toggleEndpointEnabled,
    toggleModelRouteEnabled,
  }
}
