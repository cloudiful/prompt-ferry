<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  bindCredentialGroup,
  listServerCredentials,
  refreshServerCredentialBalance,
} from '@/generated/admin-api'
import type {
  McpCredentialView,
  McpProviderDescriptor,
  McpQuotaGroup,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { useNotifier } from '@/composables/useNotifier'

const props = defineProps<{
  t: TranslateFn
  serverId: string | null
  quotaGroups: McpQuotaGroup[]
  providers: McpProviderDescriptor[]
}>()

const { notifyApiError } = useNotifier()

const credentials = ref<McpCredentialView[]>([])
const loading = ref(false)
const savingId = ref('')
const refreshingId = ref('')

const STALE_AFTER_MS = 24 * 60 * 60 * 1000

const groupOptions = computed(() => [
  { label: props.t('quotaGroupUnbound'), value: '' },
  ...props.quotaGroups.map((group) => ({
    label: group.name,
    value: group.group_id,
  })),
])

function providerFor(
  credential: McpCredentialView,
): McpProviderDescriptor | undefined {
  const kind = credential.provider_kind
  if (!kind) return undefined
  return props.providers.find((provider) => provider.id === kind)
}

function providerLabel(credential: McpCredentialView): string {
  return providerFor(credential)?.display_name ?? props.t('providerGeneric')
}

function balanceSupported(credential: McpCredentialView): boolean {
  return providerFor(credential)?.provider_balance_supported === true
}

function unitLabel(credential: McpCredentialView): string {
  const unit = providerFor(credential)?.unit
  return unit === 'credits'
    ? props.t('quotaUnitCredits')
    : props.t('quotaUnitRequests')
}

function isStale(credential: McpCredentialView): boolean {
  if (!balanceSupported(credential)) return false
  const syncedAt = credential.provider_synced_at
  if (!syncedAt) return true
  const synced = Date.parse(syncedAt)
  if (Number.isNaN(synced)) return true
  const lastErrorAt = credential.last_error_at
    ? Date.parse(credential.last_error_at)
    : Number.NaN
  if (!Number.isNaN(lastErrorAt) && lastErrorAt > synced) return true
  return Date.now() - synced > STALE_AFTER_MS
}

// A last_error older than the most recent successful snapshot is historical;
// only surface errors that are still current.
function hasCurrentError(credential: McpCredentialView): boolean {
  if (!credential.last_error) return false
  const synced = credential.provider_synced_at
    ? Date.parse(credential.provider_synced_at)
    : Number.NaN
  if (Number.isNaN(synced)) return true
  const lastErrorAt = credential.last_error_at
    ? Date.parse(credential.last_error_at)
    : Number.NaN
  if (Number.isNaN(lastErrorAt)) return true
  return lastErrorAt > synced
}

function formatBalance(credential: McpCredentialView): string {
  if (credential.provider_remaining == null) {
    return props.t('providerBalanceNotSynced')
  }
  return `${credential.provider_remaining} ${unitLabel(credential)}`
}

function formatTimestamp(value: string | null | undefined): string {
  if (!value) return '-'
  return new Date(value).toLocaleString()
}

async function load(): Promise<void> {
  if (!props.serverId) {
    credentials.value = []
    return
  }
  loading.value = true
  try {
    const response = expectData(
      await listServerCredentials<true>(
        withData({ path: { server_id: props.serverId } }),
      ),
    )
    credentials.value = response.credentials
  } catch (cause) {
    notifyApiError(cause)
  } finally {
    loading.value = false
  }
}

watch(
  () => props.serverId,
  () => {
    void load()
  },
  { immediate: true },
)

async function bind(
  credential: McpCredentialView,
  groupId: string,
): Promise<void> {
  savingId.value = credential.credential_id
  try {
    await bindCredentialGroup<true>({
      body: { quota_group_id: groupId || null },
      path: {
        server_id: credential.server_id,
        credential_id: credential.credential_id,
      },
    })
    credential.quota_group_id = groupId || null
  } catch (cause) {
    notifyApiError(cause)
    await load()
  } finally {
    savingId.value = ''
  }
}

async function refresh(credential: McpCredentialView): Promise<void> {
  refreshingId.value = credential.credential_id
  try {
    const updated = expectData(
      await refreshServerCredentialBalance<true>({
        path: {
          server_id: credential.server_id,
          credential_id: credential.credential_id,
        },
      }),
    )
    const index = credentials.value.findIndex(
      (item) => item.credential_id === updated.credential_id,
    )
    if (index >= 0) credentials.value[index] = updated
  } catch (cause) {
    notifyApiError(cause)
    await load()
  } finally {
    refreshingId.value = ''
  }
}

function secretPreview(credential: McpCredentialView): string {
  return credential.secret_preview ?? '••••••••'
}

defineExpose({ reload: load })
</script>

<template>
  <div class="grid gap-2">
    <div class="flex items-center gap-1 text-muted">
      <span>{{ t('quotaGroupBind') }}</span>
      <UTooltip :text="t('quotaBindingHint')">
        <UButton
          type="button"
          size="xs"
          color="neutral"
          variant="ghost"
          icon="i-lucide-info"
          :aria-label="t('quotaBindingHint')"
        />
      </UTooltip>
    </div>
    <div v-if="serverId && loading" class="text-xs text-dimmed">
      {{ t('loadingTools') }}
    </div>
    <div
      v-else-if="serverId && credentials.length === 0"
      class="text-xs text-dimmed"
    >
      {{ t('quotaNotConfigured') }}
    </div>
    <div v-else-if="serverId" class="grid gap-2">
      <div
        v-for="credential in credentials"
        :key="credential.credential_id"
        class="grid gap-2 rounded border border-default p-2"
      >
        <div class="grid grid-cols-[minmax(0,1fr)_10rem] items-center gap-2">
          <div class="min-w-0">
            <div class="truncate text-xs font-medium text-highlighted">
              {{ credential.credential_label }}
            </div>
            <div class="truncate font-mono text-xs text-muted">
              {{ secretPreview(credential) }}
            </div>
          </div>
          <USelect
            :model-value="credential.quota_group_id ?? ''"
            :items="groupOptions"
            label-key="label"
            value-key="value"
            :loading="savingId === credential.credential_id"
            :disabled="groupOptions.length <= 1"
            @update:model-value="bind(credential, String($event))"
          />
        </div>
        <div class="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
          <UBadge :label="providerLabel(credential)" color="neutral" />
          <template v-if="balanceSupported(credential)">
            <span class="text-muted">{{ t('providerBalance') }}:</span>
            <span class="font-medium text-highlighted">{{
              formatBalance(credential)
            }}</span>
            <UBadge
              v-if="isStale(credential)"
              :label="t('providerBalanceStale')"
              color="warning"
              size="sm"
            />
            <span v-if="credential.provider_synced_at" class="text-dimmed">
              {{ t('providerBalanceSyncedAt') }}
              {{ formatTimestamp(credential.provider_synced_at) }}
            </span>
            <span v-if="credential.provider_reset_at" class="text-dimmed">
              {{ t('providerBalanceResetsAt') }}
              {{ formatTimestamp(credential.provider_reset_at) }}
            </span>
            <UButton
              size="xs"
              color="neutral"
              variant="outline"
              icon="i-lucide-refresh-cw"
              :loading="refreshingId === credential.credential_id"
              :disabled="refreshingId === credential.credential_id"
              :aria-label="t('providerBalanceRefresh')"
              @click="refresh(credential)"
            />
          </template>
          <span v-else class="text-dimmed">
            {{ t('providerBalanceUnsupported') }}
          </span>
          <span
            v-if="hasCurrentError(credential)"
            class="truncate text-error"
            :title="credential.last_error ?? ''"
          >
            {{ credential.last_error }}
          </span>
        </div>
      </div>
    </div>
  </div>
</template>
