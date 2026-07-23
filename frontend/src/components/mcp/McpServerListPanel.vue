<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import TablePagination from '@/components/shared/TablePagination.vue'
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

const visibleItems = computed(() =>
  props.listItems.slice(props.mcpFirst, props.mcpFirst + props.mcpRows),
)
const columns = computed<TableColumn<McpServerListItemView>[]>(() => [
  { accessorKey: 'name', header: props.t('name') },
  { id: 'status', header: props.t('status') },
  { id: 'test', header: props.t('test') },
  { accessorKey: 'timeout_label', header: props.t('timeout') },
  { id: 'actions' },
])
</script>

<template>
  <section class="grid min-w-0 gap-3">
    <UTable
      :data="visibleItems"
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
          <div class="truncate font-semibold text-highlighted">
            {{ row.original.name }}
          </div>
          <div
            class="flex min-w-0 items-center gap-1.5 truncate text-xs text-muted"
          >
            <UIcon name="i-lucide-server" class="h-3.5 w-3.5 shrink-0" />
            <span class="truncate">{{ row.original.endpoint_label }}</span>
          </div>
        </div>
      </template>
      <template #status-cell="{ row }">
        <div
          class="flex min-w-0 flex-nowrap items-center gap-1.5 overflow-x-auto overflow-y-hidden whitespace-nowrap pb-px [&>*]:flex-none"
        >
          <UBadge :label="row.original.scope_label" color="neutral" />
          <UBadge :label="row.original.transport" color="info" />
          <UBadge :label="row.original.naming_mode_label" color="neutral" />
          <label
            class="inline-flex flex-none items-center gap-2 whitespace-nowrap"
          >
            <USwitch
              :model-value="row.original.enabled"
              :disabled="busy"
              @update:model-value="
                $emit('toggleMcpServer', row.original.server)
              "
            />
            <span class="text-xs text-muted">{{
              row.original.enabled_label
            }}</span>
          </label>
        </div>
      </template>
      <template #test-cell="{ row }">
        <UBadge
          v-if="row.original.test_ok !== null"
          :label="row.original.test_message"
          :color="row.original.test_ok ? 'success' : 'error'"
          variant="subtle"
        />
        <span v-else class="text-xs text-dimmed">-</span>
      </template>
      <template #timeout_label-cell="{ row }">
        <UBadge :label="row.original.timeout_label" />
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
