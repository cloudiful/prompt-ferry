<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import TablePagination from '@/components/shared/TablePagination.vue'
import type { EndpointListItemView } from '@/models/endpoints'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/table-pagination'

const props = defineProps<{
  busy: boolean
  first: number
  items: EndpointListItemView[]
  rows: number
  t: TranslateFn
  total: number
}>()

const columns = computed<TableColumn<EndpointListItemView>[]>(() => [
  { accessorKey: 'name', header: props.t('name') },
  { id: 'status', header: props.t('status') },
  { id: 'test', header: props.t('test') },
  { id: 'actions' },
])

defineEmits<{
  deleteEndpoint: [endpointId: string]
  editEndpoint: [endpointId: string]
  endpointPage: [event: TablePageChange]
  testEndpoint: [endpointId: string]
  toggleEndpointEnabled: [endpointId: string, enabled: boolean]
}>()
</script>

<template>
  <div class="hidden min-w-0 md:block">
    <UTable :data="items" :columns="columns" :loading="busy" class="min-w-0">
      <template #name-cell="{ row }">
        <div class="w-56 max-w-56 min-w-0">
          <div class="truncate font-semibold text-highlighted">
            {{ row.original.name }}
          </div>
          <div class="truncate text-xs text-muted">
            {{ row.original.base_url }}
          </div>
        </div>
      </template>
      <template #status-cell="{ row }">
        <div
          class="flex min-w-0 flex-nowrap items-center gap-1.5 overflow-x-auto overflow-y-hidden whitespace-nowrap pb-px [&>*]:flex-none"
        >
          <label class="inline-flex flex-none items-center whitespace-nowrap">
            <USwitch
              :model-value="row.original.enabled"
              :aria-label="t('status')"
              :disabled="busy || row.original.toggling"
              @update:model-value="
                $emit('toggleEndpointEnabled', row.original.endpoint_id, $event)
              "
            />
          </label>
          <UBadge
            v-if="row.original.owner_label"
            :label="row.original.owner_label"
            color="neutral"
          />
        </div>
      </template>
      <template #test-cell="{ row }">
        <UBadge
          v-if="row.original.test_severity"
          :label="row.original.test_message"
          :color="row.original.test_severity"
          variant="subtle"
        />
        <span v-else class="text-xs text-dimmed">-</span>
      </template>
      <template #actions-cell="{ row }">
        <div class="flex justify-end gap-2">
          <UButton
            size="sm"
            color="neutral"
            variant="ghost"
            :aria-label="t('test')"
            :loading="row.original.testing"
            @click="$emit('testEndpoint', row.original.endpoint_id)"
            ><UIcon name="i-lucide-refresh-cw" class="h-4 w-4"
          /></UButton>
          <UButton
            size="sm"
            color="neutral"
            variant="ghost"
            :aria-label="t('edit')"
            @click="$emit('editEndpoint', row.original.endpoint_id)"
            ><UIcon name="i-lucide-pencil" class="h-4 w-4"
          /></UButton>
          <UButton
            size="sm"
            color="error"
            variant="ghost"
            :aria-label="t('delete')"
            :loading="busy"
            @click="$emit('deleteEndpoint', row.original.endpoint_id)"
            ><UIcon name="i-lucide-trash-2" class="h-4 w-4"
          /></UButton>
        </div>
      </template>
    </UTable>
    <TablePagination
      :first="first"
      :rows="rows"
      :total="total"
      :page-size-options="STANDARD_PAGE_SIZE_OPTIONS"
      @change="$emit('endpointPage', $event)"
    />
  </div>
</template>
