<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import type { EndpointForm } from '@/models'

const props = defineProps<{
  t: TranslateFn
}>()
const form = defineModel<EndpointForm>('form', { required: true })

type ApiKeyRow = { key: string; index: number }

const rows = computed<ApiKeyRow[]>(() =>
  (form.value?.api_keys ?? []).map((_, index) => ({
    key: `key-${index}`,
    index,
  })),
)
const columns = computed<TableColumn<ApiKeyRow>[]>(() => [
  { id: 'key', header: props.t('apiKey') },
  { id: 'status', header: props.t('status') },
  { id: 'actions' },
])

function addApiKey(): void {
  if (!form.value) return
  if (!Array.isArray(form.value.api_keys)) form.value.api_keys = []
  form.value.api_keys.push({
    key_label: '',
    api_key: '',
    has_saved_key: false,
    enabled: true,
    key_id: '',
  })
}

function canDeleteApiKey(index: number): boolean {
  const keys = form.value?.api_keys ?? []
  return (
    keys.length > 1 &&
    keys.some((apiKey, apiKeyIndex) => apiKeyIndex !== index && apiKey.enabled)
  )
}

function canDisableApiKey(index: number): boolean {
  const keys = form.value?.api_keys ?? []
  const apiKey = keys[index]
  return (
    !apiKey?.enabled ||
    keys.some(
      (otherApiKey, otherIndex) => otherIndex !== index && otherApiKey.enabled,
    )
  )
}

function removeApiKey(index: number): void {
  if (!form.value || !Array.isArray(form.value.api_keys)) return
  if (!canDeleteApiKey(index)) return
  form.value.api_keys.splice(index, 1)
}

function rowHasSavedKey(index: number): boolean {
  return form.value?.api_keys[index]?.has_saved_key ?? false
}

function rowPlaceholder(index: number): string {
  return rowHasSavedKey(index)
    ? props.t('apiKeyOptionalOnEdit')
    : props.t('apiKey')
}

function apiKeyValue(index: number): string {
  return form.value?.api_keys?.[index]?.api_key ?? ''
}

function setApiKeyValue(index: number, value: string): void {
  const entry = form.value?.api_keys?.[index]
  if (entry) entry.api_key = value
}

function apiKeyEnabled(index: number): boolean {
  return form.value?.api_keys?.[index]?.enabled ?? true
}

function setApiKeyEnabled(index: number, value: boolean): void {
  const entry = form.value?.api_keys?.[index]
  if (entry) entry.enabled = value
}
</script>

<template>
  <div class="grid gap-2">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div class="grid gap-1">
        <div class="flex items-center gap-1">
          <label class="text-xs font-medium text-default">
            {{ t('apiKeys') }}
          </label>
          <UTooltip :text="t('endpointApiKeysHint')">
            <UButton
              type="button"
              size="xs"
              color="neutral"
              variant="ghost"
              icon="i-lucide-info"
              :aria-label="t('endpointApiKeysHint')"
            />
          </UTooltip>
        </div>
      </div>
      <div class="flex flex-wrap items-center justify-end gap-3">
        <label
          class="inline-flex min-h-8 items-center gap-2 text-[0.75rem] text-muted"
        >
          <span>{{ t('endpointKeyLbEnabled') }}</span>
          <USwitch v-model="form.key_lb_enabled" />
        </label>
        <UButton type="button" size="sm" color="neutral" @click="addApiKey">
          {{ t('addApiKey') }}
        </UButton>
      </div>
    </div>
    <UTable :data="rows" :columns="columns" class="min-w-0">
      <template #key-cell="{ row }">
        <div class="flex min-w-0 items-center gap-2">
          <div v-if="rowHasSavedKey(row.original.index)" class="shrink-0">
            <UBadge :label="t('saved')" color="neutral" />
          </div>
          <UInput
            :model-value="apiKeyValue(row.original.index)"
            class="min-w-0 flex-1"
            :placeholder="rowPlaceholder(row.original.index)"
            @update:model-value="setApiKeyValue(row.original.index, $event)"
          />
        </div>
      </template>
      <template #status-cell="{ row }">
        <div class="flex justify-center">
          <USwitch
            :model-value="apiKeyEnabled(row.original.index)"
            :aria-label="t('status')"
            :disabled="!canDisableApiKey(row.original.index)"
            @update:model-value="setApiKeyEnabled(row.original.index, $event)"
          />
        </div>
      </template>
      <template #actions-cell="{ row }">
        <div class="flex justify-end">
          <UButton
            type="button"
            size="sm"
            color="error"
            variant="ghost"
            :aria-label="t('delete')"
            :disabled="!canDeleteApiKey(row.original.index)"
            @click="removeApiKey(row.original.index)"
          >
            <UIcon name="i-lucide-trash-2" class="h-4 w-4" />
          </UButton>
        </div>
      </template>
    </UTable>
  </div>
</template>
