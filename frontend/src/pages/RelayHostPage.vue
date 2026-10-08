<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import {
  createEmptyRelayHostForm,
  relayHostToForm,
} from '@/admin-mappers/forms/relay-host'
import HostRoleCards from '@/components/relays/HostRoleCards.vue'
import RestartRequiredBanner from '@/components/relays/RestartRequiredBanner.vue'
import PageIntro from '../components/PageIntro.vue'
import { useLocale } from '../composables/useLocale'
import { useNotifier } from '../composables/useNotifier'
import type { HostRole } from '../generated/admin-api'
import { canManageWorker } from '../models/endpoints/capability'
import { useRelayHostStore } from '../stores/relay-host'
import { useRelaySessionStore } from '../stores/relay-session'

const { t } = useLocale()
const { notifyApiError, notifySuccess } = useNotifier()
const relaySession = useRelaySessionStore()
const hostStore = useRelayHostStore()

const form = ref(createEmptyRelayHostForm())
const tokenInput = ref('')
const showToken = ref(false)
const bootstrapping = ref(true)

const authenticated = computed(() => relaySession.authenticated)
const restartRequired = computed(
  () =>
    hostStore.restartRequired ||
    (hostStore.pendingRole != null && hostStore.pendingRole !== hostStore.role),
)
const workerAvailable = computed(() => canManageWorker(hostStore.status))
const statusLine = computed(() => {
  if (!hostStore.status) return ''
  const worker = hostStore.status.worker
  return `${worker.connected_workers}`
})

function syncForm(): void {
  form.value = relayHostToForm(hostStore.host, hostStore.settings)
}

async function bootstrap(): Promise<void> {
  bootstrapping.value = true
  try {
    const ok = await relaySession.bootstrap()
    if (ok) {
      await hostStore.refresh()
      syncForm()
    }
  } catch (cause) {
    notifyApiError(cause)
  } finally {
    bootstrapping.value = false
  }
}

async function loginWithToken(): Promise<void> {
  const token = tokenInput.value.trim()
  if (!token) return
  try {
    await relaySession.login(token)
    tokenInput.value = ''
    await hostStore.refresh()
    syncForm()
    notifySuccess(t('saved'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function selectRole(role: HostRole): Promise<void> {
  // The remote relay list (`/relays`) never flows through here: saving a
  // host role only calls the host role endpoint.
  try {
    form.value.role = role
    await hostStore.saveRole(role)
    syncForm()
    notifySuccess(t('relayRoleSaved'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function saveSettings(): Promise<void> {
  try {
    await hostStore.saveSettings(form.value.admin_bind)
    syncForm()
    notifySuccess(t('saved'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function requestRestart(): Promise<void> {
  try {
    await hostStore.requestRestart()
    notifySuccess(t('relayRestartRequested'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function relayLogout(): Promise<void> {
  await relaySession.logout()
}

onMounted(bootstrap)
</script>

<template>
  <div class="grid min-w-0 max-w-full gap-3">
    <PageIntro>
      <template #actions>
        <UBadge v-if="hostStore.role" :label="hostStore.role" />
        <UBadge
          v-if="hostStore.status"
          :label="statusLine"
          color="neutral"
          variant="subtle"
        />
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          :loading="hostStore.loading || bootstrapping"
          @click="bootstrap"
          >{{ t('refresh') }}</UButton
        >
        <UButton
          v-if="authenticated"
          size="sm"
          color="neutral"
          variant="ghost"
          @click="relayLogout"
          >{{ t('logout') }}</UButton
        >
      </template>
    </PageIntro>

    <UCard v-if="!authenticated && !bootstrapping">
      <template #header>
        <span class="text-sm font-semibold text-highlighted">{{
          t('relayHostLogin')
        }}</span>
      </template>
      <p class="text-sm text-muted">{{ t('relayHostLoginDesc') }}</p>
      <div class="mt-3 flex gap-2">
        <UInput
          v-model="tokenInput"
          :type="showToken ? 'text' : 'password'"
          :placeholder="t('relayHostTokenPlaceholder')"
          autocomplete="off"
          class="w-full"
        />
        <UButton
          :loading="relaySession.busy"
          :disabled="!tokenInput.trim()"
          @click="loginWithToken"
          >{{ t('login') }}</UButton
        >
      </div>
    </UCard>

    <template v-else-if="authenticated">
      <RestartRequiredBanner
        v-if="restartRequired"
        :busy="hostStore.restarting"
        @restart="requestRestart"
      />

      <UAlert
        v-if="hostStore.status && !workerAvailable"
        color="neutral"
        variant="subtle"
        icon="i-lucide-info"
        :title="t('relayWorkerDisconnected')"
        :description="t('relayWorkerDisconnectedDesc')"
      />

      <section class="grid min-w-0 max-w-full gap-2">
        <h2 class="text-sm font-semibold text-highlighted">
          {{ t('relayHostRoleTitle') }}
        </h2>
        <HostRoleCards
          :current-role="hostStore.role"
          :pending-role="hostStore.pendingRole"
          :busy="hostStore.savingRole"
          @select="selectRole"
        />
      </section>

      <UCard>
        <template #header>
          <span class="text-sm font-semibold text-highlighted">{{
            t('relayHostSettingsTitle')
          }}</span>
        </template>
        <UFormField
          :label="t('relayHostAdminBind')"
          :help="t('relayHostAdminBindHelp')"
        >
          <UInput
            v-model="form.admin_bind"
            placeholder="127.0.0.1:8790"
            class="w-full"
          />
        </UFormField>
        <template #footer>
          <div class="flex w-full justify-end gap-2">
            <UButton
              size="sm"
              :loading="hostStore.savingSettings"
              @click="saveSettings"
              >{{ t('save') }}</UButton
            >
            <UButton
              size="sm"
              color="warning"
              variant="outline"
              :loading="hostStore.restarting"
              @click="requestRestart"
              >{{ t('relayRestartNow') }}</UButton
            >
          </div>
        </template>
      </UCard>

      <UCard v-if="hostStore.status">
        <template #header>
          <span class="text-sm font-semibold text-highlighted">{{
            t('relayHostStatusTitle')
          }}</span>
        </template>
        <div class="grid gap-1 text-sm text-muted">
          <span
            >{{ t('relayHostRelayReady') }}:
            {{
              hostStore.relayReady
                ? t('relayStatusConnected')
                : t('relayStatusDisconnected')
            }}</span
          >
          <span
            >{{ t('relayHostWorker') }}:
            {{
              workerAvailable
                ? t('relayStatusConnected')
                : t('relayWorkerDisconnected')
            }}</span
          >
        </div>
      </UCard>
    </template>
  </div>
</template>
