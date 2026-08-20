<script setup lang="ts">
import { defineAsyncComponent } from 'vue'
import type {
  RequestRecordCategory,
  RequestRecordOverviewResponse,
} from '@/generated/admin-api'
import type { RequestRecordFilterModel, RequestRecordRowView } from '@/models'
import type { RequestRecordFormatting } from '@/models/request-record-formatting'
import type { UsageWorkspaceView } from '@/models/usage'
import type {
  RequestOverviewDrilldown,
  RequestOverviewMode,
} from '@/request-overview'
import UsageCategoryWorkspace from './UsageCategoryWorkspace.vue'

defineProps<{
  activeMode: RequestOverviewMode
  category: RequestRecordCategory
  formatting: RequestRecordFormatting
  isAdmin: boolean
  overview: RequestRecordOverviewResponse | null
  overviewLoading: boolean
  t: TranslateFn
  workspace: UsageWorkspaceView
}>()

const filtersModel = defineModel<RequestRecordFilterModel>('filters', {
  required: true,
})
const detailVisibleModel = defineModel<boolean>('detailVisible', {
  required: true,
})

defineEmits<{
  'update:detailVisible': [value: boolean]
  'update:filters': [value: RequestRecordFilterModel]
  clearConversationOverride: []
  resetSessionAffinity: []
  drilldown: [filter: RequestOverviewDrilldown]
  filter: [event: TableFilterChange]
  loadDetailRequestFull: []
  openClearDialog: []
  openDetail: [record: RequestRecordRowView]
  page: [event: TablePageChange]
  saveConversationOverride: [
    selection: {
      endpointId: string
      endpointKeyId: string | null
    },
  ]
  search: []
  sort: [event: TableSortChange]
}>()

const UsagePanel = defineAsyncComponent(() => import('./UsagePanel.vue'))
const McpUsagePanel = defineAsyncComponent(
  () => import('../mcp/McpUsagePanel.vue'),
)
</script>

<template>
  <UsageCategoryWorkspace
    :active-mode="activeMode"
    :overview="overview"
    :loading="overviewLoading"
    :category="category"
    :formatting="formatting"
    :t="t"
    @drilldown="$emit('drilldown', $event)"
  >
    <template #records>
      <UsagePanel
        v-if="category === 'ai'"
        v-model:filters="filtersModel"
        v-model:detail-visible="detailVisibleModel"
        :formatting="formatting"
        :workspace="workspace"
        :is-admin="isAdmin"
        :t="t"
        @clear-conversation-override="$emit('clearConversationOverride')"
        @filter="$emit('filter', $event)"
        @load-detail-request-full="$emit('loadDetailRequestFull')"
        @open-clear-dialog="$emit('openClearDialog')"
        @open-detail="$emit('openDetail', $event)"
        @page="$emit('page', $event)"
        @reset-session-affinity="$emit('resetSessionAffinity')"
        @save-conversation-override="$emit('saveConversationOverride', $event)"
        @search="$emit('search')"
        @sort="$emit('sort', $event)"
      >
        <template v-if="$slots.recordsToolbar" #headerActions>
          <slot name="recordsToolbar" />
        </template>
      </UsagePanel>
      <McpUsagePanel
        v-else
        v-model:filters="filtersModel"
        v-model:detail-visible="detailVisibleModel"
        :formatting="formatting"
        :workspace="workspace"
        :is-admin="isAdmin"
        :t="t"
        @filter="$emit('filter', $event)"
        @open-clear-dialog="$emit('openClearDialog')"
        @open-detail="$emit('openDetail', $event)"
        @page="$emit('page', $event)"
        @search="$emit('search')"
        @sort="$emit('sort', $event)"
      >
        <template v-if="$slots.recordsToolbar" #headerActions>
          <slot name="recordsToolbar" />
        </template>
      </McpUsagePanel>
    </template>
  </UsageCategoryWorkspace>
</template>
