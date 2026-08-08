<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import type { McpBearerTokenForm } from '@/models'

const tokens = defineModel<McpBearerTokenForm[]>('tokens', { required: true })

defineProps<{
  t: TranslateFn
}>()

type TokenRow = { key: string; index: number }

const rows = computed<TokenRow[]>(() =>
  tokens.value.map((_, index) => ({ key: `token-${index}`, index })),
)

const columns: TableColumn<TokenRow>[] = [
  { id: 'index', header: '#' },
  { id: 'token' },
  { id: 'status' },
  { id: 'actions' },
]

function addToken(): void {
  tokens.value.push({ token: '', enabled: true })
}

function hasEnabledNonEmptyToken(otherThan: number): boolean {
  return tokens.value.some(
    (token, tokenIndex) =>
      tokenIndex !== otherThan && token.enabled && token.token.trim() !== '',
  )
}

function canDisableToken(index: number): boolean {
  const token = tokens.value[index]
  if (!token || !token.enabled || token.token.trim() === '') return true
  return hasEnabledNonEmptyToken(index)
}

function canRemoveToken(index: number): boolean {
  const token = tokens.value[index]
  if (!token || token.token.trim() === '') return true
  const otherNonEmpty = tokens.value.some(
    (other, otherIndex) => otherIndex !== index && other.token.trim() !== '',
  )
  return !otherNonEmpty || hasEnabledNonEmptyToken(index)
}

function removeToken(index: number): void {
  if (!canRemoveToken(index)) return
  tokens.value.splice(index, 1)
}
</script>

<template>
  <div class="grid gap-2">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div class="flex items-center gap-1">
        <label class="text-xs font-medium text-default">
          {{ t('bearerToken') }}
        </label>
        <UTooltip :text="t('bearerTokenHint')">
          <UButton
            type="button"
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-info"
            :aria-label="t('bearerTokenHint')"
          />
        </UTooltip>
      </div>
      <UButton type="button" size="sm" color="neutral" @click="addToken">
        {{ t('addBearerToken') }}
      </UButton>
    </div>
    <UTable :data="rows" :columns="columns" class="min-w-0">
      <template #index-header>{{ t('id') }}</template>
      <template #index-cell="{ row }">
        <span class="text-xs font-medium text-default">{{
          row.original.index + 1
        }}</span>
      </template>
      <template #token-header>{{ t('bearerToken') }}</template>
      <template #token-cell="{ row }">
        <UInput
          v-model="tokens[row.original.index].token"
          class="w-full"
          :placeholder="t('bearerTokenPlaceholder')"
        />
      </template>
      <template #status-header>{{ t('status') }}</template>
      <template #status-cell="{ row }">
        <div class="flex justify-center">
          <USwitch
            v-model="tokens[row.original.index].enabled"
            :aria-label="t('status')"
            :disabled="!canDisableToken(row.original.index)"
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
            :disabled="!canRemoveToken(row.original.index)"
            @click="removeToken(row.original.index)"
          >
            <UIcon name="i-lucide-trash-2" class="h-4 w-4" />
          </UButton>
        </div>
      </template>
    </UTable>
  </div>
</template>
