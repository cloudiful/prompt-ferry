<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  bindCredentialGroup,
  listServerCredentials,
} from '@/generated/admin-api'
import type { McpCredentialView, McpQuotaGroup } from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { useNotifier } from '@/composables/useNotifier'

const props = defineProps<{
  t: TranslateFn
  serverId: string | null
  quotaGroups: McpQuotaGroup[]
}>()

const { notifyApiError } = useNotifier()

const credentials = ref<McpCredentialView[]>([])
const loading = ref(false)
const savingId = ref('')

const groupOptions = computed(() => [
  { label: props.t('quotaGroupUnbound'), value: '' },
  ...props.quotaGroups.map((group) => ({
    label: group.name,
    value: group.group_id,
  })),
])

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
        class="grid grid-cols-[minmax(0,1fr)_10rem] items-center gap-2"
      >
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
    </div>
  </div>
</template>
