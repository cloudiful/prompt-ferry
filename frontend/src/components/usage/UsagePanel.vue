<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import type { SortingState } from '@tanstack/vue-table'
import { computed } from 'vue'
import type { RequestRecordFilterModel, RequestRecordRowView } from '@/models'
import type { RequestRecordFormatting } from '@/models/request-record-formatting'
import type { UsageWorkspaceView } from '@/models/usage'
import { REQUEST_RECORD_PAGE_SIZE_OPTIONS } from '@/table-pagination'
import TablePagination from '@/components/shared/TablePagination.vue'
import UsageDetailDialog from './detail/UsageDetailDialog.vue'
import UsageRecordsToolbar from './UsageRecordsToolbar.vue'

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
  clearConversationOverride: []
  filter: [event: TableFilterChange]
  loadDetailRequestFull: []
  resetSessionAffinity: []
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
  { accessorKey: 'model_key', header: props.t('model'), enableSorting: true },
  { accessorKey: 'target', header: props.t('upstream'), enableSorting: true },
  {
    accessorKey: 'request_state',
    header: props.t('status'),
    enableSorting: true,
  },
  { id: 'redaction', header: props.t('redaction') },
  {
    accessorKey: 'duration_ms',
    header: props.t('totalLatency'),
    enableSorting: true,
  },
  {
    accessorKey: 'total_tokens',
    header: props.t('tokenCacheSummary'),
    enableSorting: true,
  },
  { id: 'throughput', header: `${props.t('e2eOutputRate')} token/s` },
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
</script>

<template>
  <div class="grid gap-3">
    <UsageRecordsToolbar
      v-model:filters="filters"
      category="ai"
      :facets="workspace.facets"
      :is-admin="isAdmin"
      :t="t"
      @filter="$emit('filter', {})"
      @open-clear-dialog="$emit('openClearDialog')"
      @search="$emit('search')"
    >
      <template #headerActions><slot name="headerActions" /></template>
    </UsageRecordsToolbar>

    <div class="min-w-0 overflow-x-auto overflow-y-hidden">
      <UTable
        v-model:sorting="sorting"
        :data="workspace.records"
        :columns="columns"
        :loading="workspace.records_loading"
        class="min-w-[76rem] overflow-visible"
      >
        <template #empty>{{ t('noRequestRecords') }}</template>
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
        <template #model_key-cell="{ row }">
          <span class="font-semibold text-highlighted">{{
            row.original.model_key
          }}</span>
        </template>
        <template #target-cell="{ row }">
          <div class="flex items-center gap-1 whitespace-nowrap">
            <span class="font-semibold text-highlighted">{{
              row.original.upstream_label
            }}</span>
            <UBadge
              :label="`${t('session')} ${row.original.session_short_id}`"
              variant="subtle"
            />
            <UBadge
              v-if="row.original.conversation_seq"
              :label="`#${row.original.conversation_seq}`"
              variant="subtle"
            />
            <UBadge
              v-if="row.original.is_first_turn"
              :label="t('firstTurn')"
              variant="subtle"
            />
          </div>
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
        <template #redaction-cell="{ row }">
          <div
            v-if="row.original.redaction.applied"
            class="flex items-center gap-1"
          >
            <UBadge :label="t('redactionHit')" color="info" variant="subtle" />
            <UBadge
              :label="String(row.original.redaction.replacements_count)"
              color="neutral"
              variant="subtle"
            />
          </div>
          <span v-else>-</span>
        </template>
        <template #duration_ms-cell="{ row }">{{
          formatting.formatMs(row.original.duration_ms)
        }}</template>
        <template #total_tokens-cell="{ row }">
          {{ formatting.formatTokenQuantity(row.original.total_tokens) }} /
          {{ formatting.formatTokenQuantity(row.original.cached_tokens) }} /
          {{ formatting.formatPercent(row.original.cache_rate) }}
        </template>
        <template #throughput-cell="{ row }">
          <UTooltip
            v-if="formatting.hasOutputRate(row.original)"
            :text="t('e2eOutputRate')"
          >
            <span>{{
              formatting.formatOutputTokensPerSecond(row.original)
            }}</span>
          </UTooltip>
          <span v-else>-</span>
        </template>
        <template #error_message-cell="{ row }">
          <UButton
            v-if="row.original.error_message"
            :color="
              row.original.request_state === 'aborted' ? 'neutral' : 'error'
            "
            variant="link"
            class="max-w-56 truncate"
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

    <UsageDetailDialog
      v-model:visible="detailVisible"
      :detail="workspace.detail"
      :formatting="formatting"
      :t="t"
      @clear-conversation-override="$emit('clearConversationOverride')"
      @load-request-full="$emit('loadDetailRequestFull')"
      @reset-session-affinity="$emit('resetSessionAffinity')"
      @save-conversation-override="$emit('saveConversationOverride', $event)"
    />
  </div>
</template>
