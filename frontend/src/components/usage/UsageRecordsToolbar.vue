<script setup lang="ts">
import type { RequestRecordCategory } from '@/generated/admin-api'
import type { RequestRecordFilterModel } from '@/models'
import type { UsageFacetOptionsView } from '@/models/usage'
import UsageFilterSelect from './UsageFilterSelect.vue'

defineProps<{
  category: RequestRecordCategory
  facets: UsageFacetOptionsView
  isAdmin: boolean
  t: TranslateFn
}>()

const filters = defineModel<RequestRecordFilterModel>('filters', {
  required: true,
})

const emit = defineEmits<{
  filter: []
  openClearDialog: []
  search: []
}>()

function applyFilter(): void {
  emit('filter')
}
</script>

<template>
  <div class="grid gap-3 border-b border-default pb-3">
    <div class="flex flex-wrap items-center gap-2">
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
      <slot name="headerActions" />
      <UButton
        color="error"
        variant="outline"
        icon="i-lucide-trash-2"
        :label="t('clearHistory')"
        @click="emit('openClearDialog')"
      />
    </div>
    <div class="flex flex-wrap gap-2">
      <UsageFilterSelect
        v-model="filters.request_date.value"
        :options="facets.request_date_options"
        :placeholder="t('allDates')"
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
    </div>
  </div>
</template>
