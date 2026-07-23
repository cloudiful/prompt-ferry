<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed, onMounted, ref } from 'vue'
import {
  createEmptyRelayForm,
  createRelayPatchRequest,
  createRelayRequest,
  relayToForm,
  type RelayForm,
  type RelaySecretAction,
} from '@/admin-mappers/forms/relay'
import RelayDialog from '@/components/relays/RelayDialog.vue'
import PageIntro from '../components/PageIntro.vue'
import { useLocale } from '../composables/useLocale'
import { useNotifier } from '../composables/useNotifier'
import type {
  BridgeEncryptionMode,
  ManagedRelay,
  TlsMode,
} from '../generated/admin-api'
import type { Option } from '../models'
import { useRelaysStore } from '../stores/relays'

const { t } = useLocale()
const { notifyApiError, notifySuccess } = useNotifier()
const relayStore = useRelaysStore()

const dialogVisible = ref(false)
const dialogForm = ref<RelayForm>(createEmptyRelayForm())

const tlsOptions = computed(() => [
  { label: t('relayTlsOff'), value: 'off' satisfies TlsMode },
  { label: t('relayTlsServer'), value: 'server' satisfies TlsMode },
  { label: t('relayTlsMtls'), value: 'mtls' satisfies TlsMode },
])
const bridgeOptions = computed(() => [
  {
    label: t('relayBridgeOff'),
    value: 'off' satisfies BridgeEncryptionMode,
  },
  {
    label: t('relayBridgeRequired'),
    value: 'required' satisfies BridgeEncryptionMode,
  },
])
const secretActionOptions = computed<Option<RelaySecretAction>[]>(() => [
  { label: t('relaySecretKeep'), value: 'keep' satisfies RelaySecretAction },
  {
    label: t('relaySecretReplace'),
    value: 'replace' satisfies RelaySecretAction,
  },
  { label: t('relaySecretClear'), value: 'clear' satisfies RelaySecretAction },
])
const dialogHeader = computed(() =>
  dialogForm.value.relay_id ? t('relayEdit') : t('relayNew'),
)
const currentDialogRelay = computed(
  () =>
    relayStore.relays.find(
      (item) => item.relay_id === dialogForm.value.relay_id,
    ) ?? null,
)
const relaySummary = computed(
  () => `${relayStore.connectedCount}/${relayStore.enabledCount}`,
)
const relayColumns = computed<TableColumn<ManagedRelay>[]>(() => [
  { accessorKey: 'name', header: t('name') },
  { accessorKey: 'relay_url', header: t('relayUrl') },
  { id: 'status', header: t('status') },
  { accessorKey: 'tls_mode', header: t('relayTlsMode') },
  { accessorKey: 'bridge_encryption_mode', header: t('relayBridgeMode') },
  { id: 'secrets', header: t('relaySecrets') },
  { id: 'lastError', header: t('relayLastError') },
  { id: 'actions' },
])
function openCreateDialog(): void {
  dialogForm.value = createEmptyRelayForm()
  dialogVisible.value = true
}

function openEditDialog(relay: ManagedRelay): void {
  dialogForm.value = relayToForm(relay)
  dialogVisible.value = true
}

function secretStateLabel(exists: boolean): string {
  return exists ? t('relaySecretStored') : t('relaySecretMissing')
}

async function refresh(): Promise<void> {
  try {
    await relayStore.refresh()
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function saveRelay(): Promise<void> {
  try {
    const relayId = dialogForm.value.relay_id || null
    const body = relayId
      ? createRelayPatchRequest(dialogForm.value)
      : createRelayRequest(dialogForm.value)
    await relayStore.saveRelay(relayId, body)
    dialogVisible.value = false
    dialogForm.value = createEmptyRelayForm()
    notifySuccess(t('saved'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function deleteRelay(relay: ManagedRelay): Promise<void> {
  if (!window.confirm(t('relayDeleteConfirm').replace('{name}', relay.name)))
    return
  try {
    await relayStore.removeRelay(relay.relay_id)
    notifySuccess(t('delete'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function reconnectRelay(relay: ManagedRelay): Promise<void> {
  try {
    await relayStore.reconnectRelay(relay.relay_id)
    notifySuccess(t('relayReconnectRequested'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

onMounted(refresh)
</script>

<template>
  <div class="grid min-w-0 max-w-full gap-3">
    <PageIntro :eyebrow="t('bridge')" :title="t('relays')">
      <template #actions>
        <UBadge :label="relaySummary" />
        <UButton size="sm" @click="openCreateDialog">{{
          t('relayNew')
        }}</UButton>
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          :loading="relayStore.loading"
          @click="refresh"
          >{{ t('refresh') }}</UButton
        >
      </template>
    </PageIntro>

    <section class="grid min-w-0 max-w-full gap-3">
      <UTable
        :data="relayStore.relays"
        :columns="relayColumns"
        :loading="relayStore.loading"
        class="min-w-0"
      >
        <template #empty>
          <div class="px-4 py-6 text-sm text-dimmed">
            {{ t('relayEmpty') }}
          </div>
        </template>
        <template #name-cell="{ row }">
          <div class="font-semibold text-highlighted">
            {{ row.original.name }}
          </div>
        </template>
        <template #relay_url-cell="{ row }">
          <div class="whitespace-nowrap text-default">
            {{ row.original.relay_url }}
          </div>
        </template>
        <template #status-cell="{ row }">
          <div class="flex items-center gap-2 whitespace-nowrap">
            <UBadge
              :label="
                row.original.connected
                  ? t('relayStatusConnected')
                  : t('relayStatusDisconnected')
              "
              :color="row.original.connected ? 'success' : 'neutral'"
            />
            <span class="text-xs text-dimmed">{{
              row.original.enabled ? t('active') : t('disabled')
            }}</span>
          </div>
        </template>
        <template #secrets-cell="{ row }">
          <div
            class="flex items-center gap-2 overflow-x-auto overflow-y-hidden whitespace-nowrap pb-px text-xs text-dimmed"
          >
            <span
              >{{ t('relaySecretCaLabel') }}:
              {{ secretStateLabel(row.original.has_relay_ca) }}</span
            >
            <span
              >{{ t('relaySecretCertLabel') }}:
              {{ secretStateLabel(row.original.has_client_cert) }}</span
            >
            <span
              >{{ t('relaySecretKeyLabel') }}:
              {{ secretStateLabel(row.original.has_client_key) }}</span
            >
            <span
              >{{ t('relaySecretBridgeLabel') }}:
              {{ secretStateLabel(row.original.has_bridge_key) }}</span
            >
          </div>
        </template>
        <template #lastError-cell="{ row }">
          <div
            class="flex items-center gap-2 overflow-x-auto overflow-y-hidden whitespace-nowrap pb-px text-xs"
          >
            <span class="text-dimmed">{{
              row.original.last_error || '—'
            }}</span>
            <span class="text-dimmed"
              >{{ t('relaySnapshotVersion') }}
              {{ row.original.last_snapshot_version ?? '—' }}</span
            >
          </div>
        </template>
        <template #actions-cell="{ row }">
          <div class="flex justify-end gap-2 whitespace-nowrap">
            <UButton
              class="shrink-0 whitespace-nowrap"
              size="sm"
              color="neutral"
              variant="ghost"
              :disabled="!row.original.enabled"
              :loading="
                relayStore.reconnectingRelayId === row.original.relay_id
              "
              @click="reconnectRelay(row.original)"
            >
              {{ t('relayReconnect') }}
            </UButton>
            <UButton
              class="shrink-0 whitespace-nowrap"
              size="sm"
              color="neutral"
              variant="ghost"
              @click="openEditDialog(row.original)"
              >{{ t('edit') }}</UButton
            >
            <UButton
              class="shrink-0 whitespace-nowrap"
              size="sm"
              color="error"
              variant="ghost"
              @click="deleteRelay(row.original)"
              >{{ t('delete') }}</UButton
            >
          </div>
        </template>
      </UTable>
    </section>

    <RelayDialog
      v-model:open="dialogVisible"
      v-model:form="dialogForm"
      :bridge-options="bridgeOptions"
      :busy="relayStore.loading"
      :current-relay="currentDialogRelay"
      :header="dialogHeader"
      :secret-action-options="secretActionOptions"
      :t="t"
      :tls-options="tlsOptions"
      @save="saveRelay"
    />
  </div>
</template>
