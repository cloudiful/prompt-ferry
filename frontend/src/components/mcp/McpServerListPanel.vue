<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import TablePagination from '@/components/shared/TablePagination.vue'
import TestResultPopover from '@/components/shared/TestResultPopover.vue'
import type { McpServer } from '@/generated/admin-api'
import type { McpServerListItemView } from '@/models'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/table-pagination'

const props = defineProps<{
  busy: boolean
  listItems: McpServerListItemView[]
  mcpFirst: number
  mcpRows: number
  mcpTotal: number
  t: TranslateFn
  testingMcpServerId: string
}>()

defineEmits<{
  deleteMcpServer: [server: McpServer]
  editMcpServer: [server: McpServer]
  mcpPage: [event: TablePageChange]
  testMcpServer: [server: McpServer]
  toggleMcpServer: [server: McpServer]
}>()

const columns = computed<TableColumn<McpServerListItemView>[]>(() => [
  { accessorKey: 'name', header: props.t('name') },
  { id: 'test', header: props.t('test') },
  { id: 'actions' },
])
</script>

<template>
  <section class="grid min-w-0 gap-3">
    <UTable
      :data="listItems"
      :columns="columns"
      :loading="busy"
      class="min-w-0"
    >
      <template #empty>
        <div class="px-4 py-6 text-sm text-dimmed">
          {{ t('noMcpServer') }}
        </div>
      </template>
      <template #name-cell="{ row }">
        <div class="min-w-0">
          <div class="flex min-w-0 items-center gap-1.5">
            <div class="min-w-0 flex-1 truncate font-semibold text-highlighted">
              {{ row.original.name }}
            </div>
            <UBadge
              :label="row.original.scope_label"
              color="neutral"
              class="shrink-0"
            />
            <UBadge
              :label="row.original.transport"
              color="info"
              class="shrink-0"
            />
          </div>
          <div class="flex min-w-0 items-center gap-1.5">
            <UIcon name="i-lucide-server" class="h-3.5 w-3.5 shrink-0" />
            <span class="min-w-0 flex-1 truncate text-xs text-muted">{{
              row.original.endpoint_label
            }}</span>
            <label class="inline-flex shrink-0 items-center">
              <USwitch
                :model-value="row.original.enabled"
                :aria-label="t('status')"
                :disabled="busy"
                @update:model-value="
                  $emit('toggleMcpServer', row.original.server)
                "
              />
            </label>
          </div>
        </div>
      </template>
      <template #test-cell="{ row }">
        <div class="min-w-0">
          <TestResultPopover
            :message="row.original.test_message"
            :severity="
              row.original.test_ok === null
                ? null
                : row.original.test_ok
                  ? 'success'
                  : 'error'
            "
          />
        </div>
      </template>
      <template #actions-cell="{ row }">
        <div class="flex justify-end gap-2">
          <UButton
            size="sm"
            color="neutral"
            variant="ghost"
            :aria-label="t('refreshCatalog')"
            :loading="testingMcpServerId === row.original.server_id"
            @click="$emit('testMcpServer', row.original.server)"
          >
            <UIcon name="i-lucide-refresh-cw" class="h-4 w-4" />
          </UButton>
          <UButton
            size="sm"
            color="neutral"
            variant="ghost"
            :aria-label="t('edit')"
            @click="$emit('editMcpServer', row.original.server)"
          >
            <UIcon name="i-lucide-pencil" class="h-4 w-4" />
          </UButton>
          <UButton
            size="sm"
            color="error"
            variant="ghost"
            :aria-label="t('delete')"
            :loading="busy"
            @click="$emit('deleteMcpServer', row.original.server)"
          >
            <UIcon name="i-lucide-trash-2" class="h-4 w-4" />
          </UButton>
        </div>
      </template>
    </UTable>
    <TablePagination
      :first="mcpFirst"
      :rows="mcpRows"
      :total="mcpTotal"
      :page-size-options="STANDARD_PAGE_SIZE_OPTIONS"
      @change="$emit('mcpPage', $event)"
    />
  </section>
</template>
