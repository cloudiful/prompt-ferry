import { computed, ref } from 'vue'

import {
  createEmptyRelayForm,
  createRelayPatchRequest,
  createRelayRequest,
  relayToForm,
  type RelayForm,
} from '@/admin-mappers/forms/relay'
import { useNotifier } from '@/composables/useNotifier'
import type { ManagedRelay } from '@/generated/admin-api'
import type { useRelaysStore } from '@/stores/relays'
import { CREATE_KEY } from '@/stores/record-action-types'

type RelaysStore = ReturnType<typeof useRelaysStore>

export function useRelaysPageActions(options: {
  relayStore: RelaysStore
  t: (key: string) => string
}) {
  const { relayStore, t } = options
  const { notifyApiError, notifySuccess } = useNotifier()
  const dialogOpen = ref(false)
  const dialogGeneration = ref(0)
  const dialogVisible = computed({
    get: () => dialogOpen.value,
    set: (visible: boolean) => {
      if (!visible && dialogOpen.value) dialogGeneration.value += 1
      dialogOpen.value = visible
    },
  })
  const dialogForm = ref<RelayForm>(createEmptyRelayForm())
  const currentDialogRelay = computed(
    () =>
      relayStore.relays.find(
        (item) => item.relay_id === dialogForm.value.relay_id,
      ) ?? null,
  )
  const dialogPendingAction = computed(
    () =>
      relayStore.relayPendingActions[dialogForm.value.relay_id || CREATE_KEY],
  )
  const dialogPending = computed(() => dialogPendingAction.value !== undefined)
  const dialogSavePending = computed(() => dialogPendingAction.value === 'save')
  const createPending = computed(
    () => relayStore.relayPendingActions[CREATE_KEY] !== undefined,
  )

  function openCreateDialog(): void {
    dialogGeneration.value += 1
    dialogForm.value = createEmptyRelayForm()
    dialogVisible.value = true
  }

  function openEditDialog(relay: ManagedRelay): void {
    dialogGeneration.value += 1
    dialogForm.value = relayToForm(relay)
    dialogVisible.value = true
  }

  async function saveRelay(): Promise<void> {
    const generation = dialogGeneration.value
    const relayId = dialogForm.value.relay_id || null
    const body = relayId
      ? createRelayPatchRequest(dialogForm.value)
      : createRelayRequest(dialogForm.value)
    try {
      const result = await relayStore.saveRelay(relayId, body)
      if (
        !result.executed ||
        !dialogVisible.value ||
        dialogGeneration.value !== generation ||
        (dialogForm.value.relay_id || null) !== relayId
      ) {
        return
      }
      dialogVisible.value = false
      dialogForm.value = createEmptyRelayForm()
      notifySuccess(t('saved'))
    } catch (cause) {
      if (dialogVisible.value && dialogGeneration.value === generation) {
        notifyApiError(cause)
      }
    }
  }

  async function deleteRelay(relay: ManagedRelay): Promise<void> {
    if (!window.confirm(t('relayDeleteConfirm').replace('{name}', relay.name)))
      return
    try {
      const result = await relayStore.removeRelay(relay.relay_id)
      if (result.executed) notifySuccess(t('delete'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function reconnectRelay(relay: ManagedRelay): Promise<void> {
    try {
      const result = await relayStore.reconnectRelay(relay.relay_id)
      if (result.executed) notifySuccess(t('relayReconnectRequested'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  return {
    createPending,
    currentDialogRelay,
    deleteRelay,
    dialogForm,
    dialogPending,
    dialogPendingAction,
    dialogSavePending,
    dialogVisible,
    openCreateDialog,
    openEditDialog,
    reconnectRelay,
    saveRelay,
  }
}
