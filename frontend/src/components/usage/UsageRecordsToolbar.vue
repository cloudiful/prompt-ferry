<script setup lang="ts">
import { computed } from 'vue'
import type { RequestRecordCategory } from '@/generated/admin-api'
import type { RequestRecordFilterModel } from '@/models'
import type {
  RequestRecordMcpScope,
  UsageFacetOptionsView,
} from '@/models/usage'
import UsageFilterSelect from './UsageFilterSelect.vue'

const props = defineProps<{
  category: RequestRecordCategory
  facets: UsageFacetOptionsView
  isAdmin: boolean
  mcpScope?: RequestRecordMcpScope
  t: TranslateFn
}>()

const filters = defineModel<RequestRecordFilterModel>('filters', {
  required: true,
})

const emit = defineEmits<{
  filter: []
  openClearDialog: []
  scopeChange: [scope: RequestRecordMcpScope]
  search: []
}>()

const scopeOptions = computed(() => [
  { label: props.t('mcpScopeService'), value: 'service' },
  { label: props.t('mcpScopeGateway'), value: 'gateway' },
  { label: props.t('mcpScopeAll'), value: 'all' },
])

function applyFilter(): void {
  emit('filter')
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
    <USelect
      v-if="category === 'mcp'"
      :model-value="mcpScope"
      class="w-36"
      size="sm"
      :aria-label="t('mcpScope')"
      :items="scopeOptions"
      label-key="label"
      value-key="value"
      @update:model-value="emit('scopeChange', $event as RequestRecordMcpScope)"
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
