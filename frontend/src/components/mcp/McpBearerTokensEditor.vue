<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'

const tokens = defineModel<string[]>('tokens', { required: true })

defineProps<{
  t: TranslateFn
}>()

const columns: TableColumn<string>[] = [
  { id: 'index', header: '#' },
  { id: 'token' },
  { id: 'actions' },
]

function addToken(): void {
  tokens.value.push('')
}

function removeToken(index: number): void {
  tokens.value.splice(index, 1)
}
</script>

<template>
  <div class="grid gap-2">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <label class="text-xs font-medium text-default">
        {{ t('bearerToken') }}
      </label>
      <UButton type="button" size="sm" color="neutral" @click="addToken">
        {{ t('addBearerToken') }}
      </UButton>
    </div>
    <UTable :data="tokens" :columns="columns" class="min-w-0">
      <template #index-header>{{ t('id') }}</template>
      <template #index-cell="{ row }">
        <span class="text-xs font-medium text-default">{{
          row.index + 1
        }}</span>
      </template>
      <template #token-header>{{ t('bearerToken') }}</template>
      <template #token-cell="{ row }">
        <UInput
          :model-value="row.original ?? ''"
          class="w-full"
          :placeholder="t('bearerTokenPlaceholder')"
          @update:model-value="
            tokens[row.index] = typeof $event === 'string' ? $event : ''
          "
        />
      </template>
      <template #actions-cell="{ row }">
        <div class="flex justify-end">
          <UButton
            type="button"
            size="sm"
            color="error"
            variant="ghost"
            :aria-label="t('delete')"
            @click="removeToken(row.index)"
          >
            <UIcon name="i-lucide-trash-2" class="h-4 w-4" />
          </UButton>
        </div>
      </template>
    </UTable>
  </div>
</template>
