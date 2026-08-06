<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import type { SortingState } from '@tanstack/vue-table'
import { computed } from 'vue'
import type { RequestRecordFilterModel, RequestRecordRowView } from '@/models'
import type { RequestRecordFormatting } from '@/models/request-record-formatting'
import type { UsageWorkspaceView } from '@/models/usage'
import { REQUEST_RECORD_PAGE_SIZE_OPTIONS } from '@/table-pagination'
import TablePagination from '@/components/shared/TablePagination.vue'
import UsageFilterSelect from '@/components/usage/UsageFilterSelect.vue'
import McpUsageDetailDialog from './McpUsageDetailDialog.vue'

const props = defineProps<{
  formatting: RequestRecordFormatting
  isAdmin: boolean
  t: TranslateFn
  workspace: UsageWorkspaceView
}>()

const filters = defineModel<RequestRecordFilterModel>('filters', {
  required: true,
})
const detailVisible = defineModel<boolean>('detailVisible', { required: true })
const emit = defineEmits<{
  filter: [event: TableFilterChange]
  openClearDialog: []
  openDetail: [record: RequestRecordRowView]
  page: [event: TablePageChange]
  search: []
  sort: [event: TableSortChange]
}>()

const columns = computed<TableColumn<RequestRecordRowView>[]>(() => [
  { id: 'details' },
  { accessorKey: 'request_date', header: props.t('date'), enableSorting: true },
  { accessorKey: 'created_at', header: props.t('time'), enableSorting: true },
  ...(props.isAdmin
    ? [
        {
          accessorKey: 'user_key',
          header: props.t('user'),
          enableSorting: true,
        },
      ]
    : []),
  {
    accessorKey: 'client_key_label',
    header: props.t('clientKey'),
    enableSorting: true,
  },
  { accessorKey: 'target', header: props.t('mcpServer'), enableSorting: true },
  {
    accessorKey: 'mcp_protocol_method',
    header: props.t('mcpMethod'),
    enableSorting: true,
  },
  {
    accessorKey: 'mcp_operation_name',
    header: props.t('mcpOperation'),
    enableSorting: true,
  },
  {
    accessorKey: 'request_state',
    header: props.t('status'),
    enableSorting: true,
  },
  {
    accessorKey: 'duration_ms',
    header: props.t('totalLatency'),
    enableSorting: true,
  },
  { accessorKey: 'error_message', header: props.t('error') },
])

const sorting = computed<SortingState>({
  get: () =>
    props.workspace.sort_order === 0
      ? []
      : [
          {
            id: props.workspace.sort_field,
            desc: props.workspace.sort_order === -1,
          },
        ],
  set: (value) => {
    const sort = value[0]
    emit('sort', {
      sortField: sort?.id ?? 'created_at',
      sortOrder: sort ? (sort.desc ? -1 : 1) : 0,
    })
  },
})

function applyFilter(): void {
  emit('filter', {})
}
</script>

<template>
  <div class="grid gap-3">
    <div class="grid gap-3 border-b border-default pb-3">
      <div class="flex flex-wrap items-center gap-2">
        <h2 class="mr-auto text-sm font-semibold text-highlighted">
          {{ t('mcpCalls') }}
        </h2>
        <UInput
          :model-value="filters.global.value ?? undefined"
          class="min-w-64 flex-1"
          icon="i-lucide-search"
          :placeholder="t('search')"
          @update:model-value="filters.global.value = String($event ?? '')"
          @keydown.enter="$emit('search')"
        />
        <slot name="headerActions" />
        <UButton
          color="error"
          variant="outline"
          icon="i-lucide-trash-2"
          :label="t('clearHistory')"
          @click="$emit('openClearDialog')"
        />
      </div>
      <div class="flex flex-wrap gap-2">
        <UsageFilterSelect
          v-model="filters.request_date.value"
          :options="workspace.facets.request_date_options"
          :placeholder="t('allDates')"
          @change="applyFilter"
        />
        <UsageFilterSelect
          v-if="isAdmin"
          v-model="filters.user_key.value"
          :options="workspace.facets.request_user_options"
          :placeholder="t('allUsers')"
          @change="applyFilter"
        />
        <UsageFilterSelect
          v-model="filters.model_key.value"
          :options="workspace.facets.request_model_options"
          :placeholder="t('mcpServer')"
          @change="applyFilter"
        />
        <UsageFilterSelect
          v-model="filters.request_state.value"
          :options="workspace.facets.request_state_options"
          :placeholder="t('allStatus')"
          @change="applyFilter"
        />
      </div>
    </div>

    <div class="min-w-0 overflow-x-auto overflow-y-hidden">
      <UTable
        v-model:sorting="sorting"
        :data="workspace.records"
        :columns="columns"
        :loading="workspace.records_loading"
        class="min-w-[72rem] overflow-visible"
      >
        <template #empty>{{ t('noMcpCallRecords') }}</template>
        <template #details-cell="{ row }">
          <UButton
            icon="i-lucide-chart-no-axes-column"
            color="neutral"
            variant="ghost"
            :aria-label="t('viewDetails')"
            @click="$emit('openDetail', row.original)"
          />
        </template>
        <template #created_at-cell="{ row }">{{
          formatting.formatTime(row.original.created_at)
        }}</template>
        <template #client_key_label-cell="{ row }">{{
          row.original.client_key_label || '-'
        }}</template>
        <template #target-cell="{ row }">
          <span class="font-semibold text-highlighted">{{
            row.original.mcp_server_name || row.original.upstream_label
          }}</span>
        </template>
        <template #mcp_protocol_method-cell="{ row }">
          <span class="font-mono">{{
            row.original.mcp_protocol_method || '-'
          }}</span>
        </template>
        <template #mcp_operation_name-cell="{ row }">
          <span class="font-mono">{{
            row.original.mcp_operation_name || '-'
          }}</span>
        </template>
        <template #request_state-cell="{ row }">
          <div class="flex items-center gap-1 whitespace-nowrap">
            <UBadge
              :label="
                formatting.formatRequestStateLabel(row.original.request_state)
              "
              :color="
                formatting.requestStateSeverity(row.original.request_state)
              "
            />
            <UBadge
              :label="`HTTP ${row.original.status ?? '-'}`"
              color="neutral"
              variant="subtle"
            />
          </div>
        </template>
        <template #duration_ms-cell="{ row }">{{
          formatting.formatMs(row.original.duration_ms)
        }}</template>
        <template #error_message-cell="{ row }">
          <UButton
            v-if="row.original.error_message"
            color="error"
            variant="link"
            class="max-w-72 truncate"
            :label="`${row.original.error_code}: ${row.original.error_message}`"
            @click="$emit('openDetail', row.original)"
          />
          <span v-else>-</span>
        </template>
      </UTable>
    </div>

    <TablePagination
      :first="workspace.first"
      :rows="workspace.rows_per_page"
      :total="workspace.total"
      :page-size-options="REQUEST_RECORD_PAGE_SIZE_OPTIONS"
      @change="$emit('page', $event)"
    />

    <McpUsageDetailDialog
      v-model:visible="detailVisible"
      :detail-loading="workspace.detail.detail_loading"
      :event="workspace.detail.record"
      :formatting="formatting"
      :t="t"
    />
  </div>
</template>
