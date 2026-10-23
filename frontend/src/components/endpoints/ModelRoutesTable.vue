<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed, ref } from 'vue'
import TablePagination from '@/components/shared/TablePagination.vue'
import TestResultPopover from '@/components/shared/TestResultPopover.vue'
import type { ModelRouteListItemView } from '@/models/endpoints'
import { modelRouteTestCounts } from '@/models/endpoints/model-route-item'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/table-pagination'
import ModelRouteTargetsSummary from './ModelRouteTargetsSummary.vue'
import ModelRouteTestResultsDialog from './ModelRouteTestResultsDialog.vue'

const props = withDefaults(
  defineProps<{
    busy: boolean
    first: number
    items: ModelRouteListItemView[]
    pendingKeys?: Readonly<Record<string, string>>
    rows: number
    t: TranslateFn
    total: number
  }>(),
  { pendingKeys: () => ({}) },
)

// Save, delete, and toggle of one route share a key, so a record being written
// is gated as a whole while only the action holding it spins.
function isPending(ruleId: string): boolean {
  return Object.hasOwn(props.pendingKeys, ruleId)
}

function pendingAction(ruleId: string): string {
  return props.pendingKeys[ruleId] ?? ''
}

const detailsRuleId = ref('')
const detailsOpen = ref(false)
const detailsResults = computed(() => {
  const item = props.items.find(
    (entry) => entry.rule_id === detailsRuleId.value,
  )
  return item?.test_results ?? []
})

function openDetails(ruleId: string): void {
  detailsRuleId.value = ruleId
  detailsOpen.value = true
}

// The row shows the counts next to the icon, so the result is readable
// without relying on the color alone.
function countsLabel(item: ModelRouteListItemView): string {
  const counts = modelRouteTestCounts(item.test_results)
  return counts.total ? `${counts.passed}/${counts.total}` : ''
}

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
    <UTable
      :data="items"
      :columns="columns"
      :loading="busy"
      class="min-w-0"
      :ui="{ th: 'whitespace-nowrap' }"
    >
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
          <ModelRouteTargetsSummary :targets="row.original.targets" :t="t" />
          <UBadge
            :label="row.original.owner_label"
            color="neutral"
            variant="subtle"
            class="w-fit"
          />
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
              :disabled="
                busy || row.original.toggling || isPending(row.original.rule_id)
              "
              :loading="pendingAction(row.original.rule_id) === 'toggle'"
              @update:model-value="
                $emit('toggleModelRouteEnabled', row.original.rule_id, $event)
              "
            />
          </label>
        </div>
      </template>
      <template #test-cell="{ row }">
        <div class="min-w-0">
          <TestResultPopover
            :message="row.original.test_message"
            :severity="row.original.test_severity"
            :detail-label="countsLabel(row.original)"
            interactive
            @select="openDetails(row.original.rule_id)"
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
            :loading="pendingAction(row.original.rule_id) === 'delete'"
            :disabled="isPending(row.original.rule_id)"
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
    <ModelRouteTestResultsDialog
      v-model:visible="detailsOpen"
      :results="detailsResults"
      :t="t"
    />
  </div>
</template>
