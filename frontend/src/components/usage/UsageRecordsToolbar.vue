<script setup lang="ts">
import type { RequestRecordOverviewRange } from '@/generated/admin-api'
import type { RequestRecordCategory } from '@/generated/admin-api'
import type { RequestRecordFilterModel } from '@/models'
import type { UsageFacetOptionsView } from '@/models/usage'
import UsageFilterSelect from './UsageFilterSelect.vue'
import UsageRangePicker from './UsageRangePicker.vue'

defineProps<{
  category: RequestRecordCategory
  end: string
  facets: UsageFacetOptionsView
  isAdmin: boolean
  range: RequestRecordOverviewRange
  start: string
  t: TranslateFn
}>()

const filters = defineModel<RequestRecordFilterModel>('filters', {
  required: true,
})

const emit = defineEmits<{
  filter: []
  openClearDialog: []
  range: [
    input: { range: RequestRecordOverviewRange; start?: string; end?: string },
  ]
  search: []
}>()

function applyFilter(): void {
  emit('filter')
}

function applyRange(input: {
  range: RequestRecordOverviewRange
  start?: string
  end?: string
}): void {
  emit('range', input)
}
</script>

<template>
  <div class="flex flex-wrap items-center gap-2 border-b border-default pb-3">
    <h2 class="mr-auto text-sm font-semibold text-highlighted">
      {{ category === 'ai' ? t('recentRequests') : t('mcpCalls') }}
    </h2>
    <UInput
      :model-value="filters.global.value ?? undefined"
      class="min-w-64 flex-1"
      icon="i-lucide-search"
      :placeholder="t('search')"
      @update:model-value="filters.global.value = String($event ?? '')"
      @keydown.enter="emit('search')"
    />
    <UsageFilterSelect
      v-model="filters.client_key_id.value"
      :options="facets.request_client_key_options"
      :placeholder="t('allClientKeys')"
      @change="applyFilter"
    />
    <UsageFilterSelect
      v-if="isAdmin"
      v-model="filters.user_key.value"
      :options="facets.request_user_options"
      :placeholder="t('allUsers')"
      @change="applyFilter"
    />
    <UsageFilterSelect
      v-model="filters.model_key.value"
      :options="facets.request_model_options"
      :placeholder="category === 'ai' ? t('allModels') : t('mcpServer')"
      @change="applyFilter"
    />
    <UsageFilterSelect
      v-model="filters.request_state.value"
      :options="facets.request_state_options"
      :placeholder="t('allStatus')"
      @change="applyFilter"
    />
    <UsageFilterSelect
      v-if="category === 'ai'"
      v-model="filters.redaction_applied.value"
      :options="facets.request_redaction_options"
      :placeholder="t('allRedactionStates')"
      @change="applyFilter"
    />
    <UsageRangePicker
      :end="end"
      :start="start"
      :t="t"
      :value="range"
      @apply="applyRange"
    />
    <slot name="headerActions" />
    <UButton
      color="error"
      variant="outline"
      icon="i-lucide-trash-2"
      :label="t('clearHistory')"
      @click="emit('openClearDialog')"
    />
  </div>
</template>
