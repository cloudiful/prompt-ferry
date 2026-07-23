<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import type { EndpointForm } from '@/models'

const props = defineProps<{
  t: TranslateFn
}>()
const form = defineModel<EndpointForm>('form', { required: true })

type ApiKeyRow =
  | { key: 'primary'; kind: 'primary' }
  | { key: `extra-${number}`; kind: 'extra'; index: number }

const rows = computed<ApiKeyRow[]>(() => [
  { key: 'primary', kind: 'primary' },
  ...form.value.api_keys.map((_, index) => ({
    key: `extra-${index}` as const,
    kind: 'extra' as const,
    index,
  })),
])
const columns = computed<TableColumn<ApiKeyRow>[]>(() => [
  { id: 'kind', header: props.t('type') },
  { id: 'key', header: props.t('apiKey') },
  { id: 'status', header: props.t('status') },
  { id: 'actions' },
])

function addApiKey(): void {
  form.value.api_keys.push({
    key_label: '',
    api_key: '',
    has_saved_key: false,
    enabled: true,
  })
}

function removeApiKey(index: number): void {
  form.value.api_keys.splice(index, 1)
}

function rowHasSavedKey(row: ApiKeyRow): boolean {
  if (row.kind === 'primary') {
    return form.value.primary_api_key_saved
  }
  return form.value.api_keys[row.index]?.has_saved_key ?? false
}

function rowPlaceholder(row: ApiKeyRow): string {
  return rowHasSavedKey(row)
    ? props.t('apiKeyOptionalOnEdit')
    : props.t('apiKey')
}
</script>

<template>
  <div class="grid gap-2">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div class="grid gap-1">
        <label class="text-xs font-medium text-default">
          {{ t('apiKeys') }}
        </label>
        <div class="text-xs leading-snug text-dimmed">
          {{ t('endpointApiKeysHint') }}
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
      <template #kind-cell="{ row }">
        <span class="text-xs font-medium text-default">
          {{
            row.original.kind === 'primary'
              ? t('primaryApiKey')
              : `#${row.original.index + 2}`
          }}
        </span>
      </template>
      <template #key-cell="{ row }">
        <div class="grid gap-1.5">
          <div
            v-if="rowHasSavedKey(row.original)"
            class="flex items-center gap-2"
          >
            <UBadge :label="t('saved')" color="neutral" />
          </div>
          <UInput
            v-if="row.original.kind === 'primary'"
            v-model="form.api_key"
            class="w-full"
            :placeholder="rowPlaceholder(row.original)"
          />
          <UInput
            v-else
            v-model="form.api_keys[row.original.index].api_key"
            class="w-full"
            :placeholder="rowPlaceholder(row.original)"
          />
        </div>
      </template>
      <template #status-cell="{ row }">
        <div v-if="row.original.kind === 'primary'" class="text-xs text-dimmed">
          {{ t('active') }}
        </div>
        <div v-else class="flex justify-center">
          <USwitch v-model="form.api_keys[row.original.index].enabled" />
        </div>
      </template>
      <template #actions-cell="{ row }">
        <div class="flex justify-end">
          <UButton
            v-if="row.original.kind === 'extra'"
            type="button"
            size="sm"
            color="error"
            variant="ghost"
            :aria-label="t('delete')"
            @click="removeApiKey(row.original.index)"
          >
            <UIcon name="i-lucide-trash-2" class="h-4 w-4" />
          </UButton>
        </div>
      </template>
    </UTable>
  </div>
</template>
