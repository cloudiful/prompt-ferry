import { computed, ref, type Ref } from 'vue'

import {
  relayHostToForm,
  type RelayHostForm,
} from '@/admin-mappers/forms/relay-host'
import { useNotifier } from '@/composables/useNotifier'
import type { HostRole } from '@/generated/admin-api'
import type { useRelayHostStore } from '@/stores/relay-host'

type RelayHostStore = ReturnType<typeof useRelayHostStore>

export function useRelayHostPageActions(options: {
  form: Ref<RelayHostForm>
  hostStore: RelayHostStore
  t: (key: string) => string
}) {
  const { form, hostStore, t } = options
  const { notifyApiError, notifySuccess } = useNotifier()
  const roleActionTarget = ref<HostRole | null>(null)
  const restartOrigin = ref<'banner' | 'settings' | null>(null)
  const hostPendingAction = computed(
    () => hostStore.relayHostPendingActions.local,
  )
  const hostWritePending = computed(() => hostPendingAction.value !== undefined)
  const bannerRestartPending = computed(
    () =>
      hostPendingAction.value === 'restart' && restartOrigin.value === 'banner',
  )
  const settingsRestartPending = computed(
    () =>
      hostPendingAction.value === 'restart' &&
      restartOrigin.value === 'settings',
  )

  function syncForm(submitted?: RelayHostForm): void {
    const confirmed = relayHostToForm(hostStore.host, hostStore.settings)
    const current = form.value
    form.value = {
      role:
        submitted && current.role !== submitted.role
          ? current.role
          : confirmed.role,
      admin_bind:
        submitted && current.admin_bind !== submitted.admin_bind
          ? current.admin_bind
          : confirmed.admin_bind,
    }
  }

  async function selectRole(role: HostRole): Promise<void> {
    if (hostWritePending.value) return
    const submitted = { ...form.value, role }
    form.value.role = role
    roleActionTarget.value = role
    try {
      const result = await hostStore.saveRole(role)
      if (!result.executed) return
      syncForm(submitted)
      notifySuccess(t('relayRoleSaved'))
    } catch (cause) {
      notifyApiError(cause)
    } finally {
      roleActionTarget.value = null
    }
  }

  async function saveSettings(): Promise<void> {
    const submitted = { ...form.value }
    try {
      const result = await hostStore.saveSettings(submitted.admin_bind)
      if (!result.executed) return
      syncForm(submitted)
      notifySuccess(t('saved'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function requestRestart(origin: 'banner' | 'settings'): Promise<void> {
    if (hostWritePending.value) return
    restartOrigin.value = origin
    try {
      const result = await hostStore.requestRestart()
      if (result.executed) notifySuccess(t('relayRestartRequested'))
    } catch (cause) {
      notifyApiError(cause)
    } finally {
      restartOrigin.value = null
    }
  }

  return {
    bannerRestartPending,
    hostPendingAction,
    hostWritePending,
    requestRestart,
    roleActionTarget,
    saveSettings,
    settingsRestartPending,
    selectRole,
  }
}
