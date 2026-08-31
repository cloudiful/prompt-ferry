<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import TablePagination from '@/components/shared/TablePagination.vue'
import TestResultPopover from '@/components/shared/TestResultPopover.vue'
import type { ModelRouteListItemView } from '@/models/endpoints'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/table-pagination'

const props = defineProps<{
  busy: boolean
  first: number
  items: ModelRouteListItemView[]
  rows: number
  t: TranslateFn
  total: number
}>()

const columns = computed<TableColumn<ModelRouteListItemView>[]>(() => [
  { accessorKey: 'model_pattern', header: props.t('modelPattern') },
  { id: 'targets', header: props.t('endpoint') },
  { id: 'status', header: props.t('status') },
  { id: 'test', header: props.t('test') },
  { id: 'actions' },
])

defineEmits<{
  deleteModelRoute: [ruleId: string]
  editModelRoute: [ruleId: string]
  modelRoutePage: [event: TablePageChange]
  testModelRoute: [ruleId: string]
  toggleModelRouteEnabled: [ruleId: string, enabled: boolean]
}>()
</script>

<template>
  <div class="hidden min-w-0 md:block">
    <UTable :data="items" :columns="columns" :loading="busy" class="min-w-0">
      <template #empty>
        <div class="px-4 py-6 text-sm text-dimmed">
          {{ t('noModelRoutes') }}
        </div>
      </template>
      <template #model_pattern-cell="{ row }">
        <div class="min-w-0 truncate font-semibold text-highlighted">
          {{ row.original.model_pattern }}
        </div>
      </template>
      <template #targets-cell="{ row }">
        <div class="grid min-w-0 gap-1.5">
          <div
            v-if="row.original.targets.length"
            class="flex min-w-0 flex-nowrap items-center gap-1.5 overflow-x-auto overflow-y-hidden whitespace-nowrap pb-px [&>*]:flex-none"
          >
            <span
              v-for="target in row.original.targets"
              :key="target.target_id"
              :class="
                target.endpoint_enabled
                  ? 'inline-flex min-w-0 max-w-full items-center gap-1 rounded-full border border-default bg-default px-2 py-px text-[0.73rem] leading-[1.35] text-default'
                  : 'inline-flex min-w-0 max-w-full items-center gap-1 rounded-full border border-default bg-muted px-2 py-px text-[0.73rem] leading-[1.35] text-muted opacity-72'
              "
            >
              {{ target.endpoint_label }}
              <span
                v-if="target.upstream_model"
                class="text-[0.68rem] text-muted"
              >
                / {{ target.upstream_model }}
              </span>
            </span>
            <UBadge
              :label="row.original.owner_label"
              color="neutral"
              variant="subtle"
            />
          </div>
          <div v-else class="text-xs text-dimmed">-</div>
        </div>
      </template>
      <template #status-cell="{ row }">
        <div
          class="flex min-w-0 flex-nowrap items-center gap-1.5 overflow-x-auto overflow-y-hidden whitespace-nowrap pb-px [&>*]:flex-none"
        >
          <UBadge
            :label="row.original.scope_label"
            color="neutral"
            variant="subtle"
          />
          <label class="inline-flex flex-none items-center whitespace-nowrap">
            <USwitch
              :model-value="row.original.enabled"
              :aria-label="t('status')"
              :disabled="busy || row.original.toggling"
              @update:model-value="
                $emit('toggleModelRouteEnabled', row.original.rule_id, $event)
              "
            />
          </label>
          <UBadge
            :label="row.original.routing_strategy_label"
            color="neutral"
            variant="subtle"
          />
        </div>
      </template>
      <template #test-cell="{ row }">
        <div class="min-w-0">
          <TestResultPopover
            :message="row.original.test_message"
            :severity="row.original.test_severity"
          />
        </div>
      </template>
      <template #actions-cell="{ row }">
        <div class="flex justify-end gap-2">
          <UButton
            size="sm"
            color="neutral"
            variant="ghost"
            :aria-label="t('test')"
            :loading="row.original.testing"
            @click="$emit('testModelRoute', row.original.rule_id)"
            ><UIcon name="i-lucide-refresh-cw" class="h-4 w-4"
          /></UButton>
          <UButton
            size="sm"
            color="neutral"
            variant="ghost"
            :aria-label="t('edit')"
            @click="$emit('editModelRoute', row.original.rule_id)"
            ><UIcon name="i-lucide-pencil" class="h-4 w-4"
          /></UButton>
          <UButton
            size="sm"
            color="error"
            variant="ghost"
            :aria-label="t('delete')"
            :loading="busy"
            @click="$emit('deleteModelRoute', row.original.rule_id)"
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
      @change="$emit('modelRoutePage', $event)"
    />
  </div>
</template>
