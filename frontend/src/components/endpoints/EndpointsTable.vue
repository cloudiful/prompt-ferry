<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import {
  computed,
  defineComponent,
  h,
  onMounted,
  watch,
  type PropType,
} from 'vue'
import EndpointNameCell from '@/components/endpoints/EndpointNameCell.vue'
import TablePagination from '@/components/shared/TablePagination.vue'
import TestResultPopover from '@/components/shared/TestResultPopover.vue'
import {
  tokenPlanBadgePills,
  useTokenPlanBadges,
} from '@/composables/useTokenPlanBadges'
import { prefetchTokenPlanBatch } from '@/composables/useTokenPlanUsageCache'
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

// Quota-bearing providers — mirrors the guard around the existing
// `tokenPlanUsage` action button and the backend admin guard. Non-quota
// providers carry no plan windows, so the badge column collapses to a
// dash instead of a misleading row of empty pills.
const QUOTA_PROVIDERS = new Set<EndpointListItemView['provider']>([
  'minimax',
  'command_code',
  'opencode_go',
  'openrouter',
  'glm',
  'deepseek',
])

function isQuotaProvider(provider: EndpointListItemView['provider']): boolean {
  return QUOTA_PROVIDERS.has(provider)
}

const showUsageColumn = computed(() =>
  props.items.some((item) => isQuotaProvider(item.provider)),
)

const columns = computed<TableColumn<EndpointListItemView>[]>(() => {
  const cols: TableColumn<EndpointListItemView>[] = [
    { accessorKey: 'name', header: props.t('name') },
    { id: 'status', header: props.t('status') },
  ]
  if (showUsageColumn.value) {
    cols.push({
      id: 'usage',
      header: props.t('tokenPlanUsageColumn'),
    })
  }
  cols.push({ id: 'test', header: props.t('test') })
  cols.push({ id: 'actions' })
  return cols
})

defineEmits<{
  deleteEndpoint: [endpointId: string]
  editEndpoint: [endpointId: string]
  endpointPage: [event: TablePageChange]
  testEndpoint: [endpointId: string]
  tokenPlanUsage: [endpointId: string]
  toggleEndpointEnabled: [endpointId: string, enabled: boolean]
}>()

// Lazy prefetch: kick off token-plan fetches for the visible page once
// the table mounts and every time the items list swaps (page change,
// refresh, delete). The cache layer caps in-flight at 4 concurrent
// requests and coalesces duplicate ids.
async function prefetchVisible(): Promise<void> {
  const ids = props.items
    .filter((item) => isQuotaProvider(item.provider))
    .map((item) => item.endpoint_id)
  await prefetchTokenPlanBatch(ids, 4)
}

onMounted(() => {
  void prefetchVisible()
})

// Re-prefetch whenever the visible endpoint set changes. We compare on
// the joined id list so order or unrelated field updates don't trigger
// redundant fetches.
watch(
  () => props.items.map((item) => item.endpoint_id).join('|'),
  () => {
    void prefetchVisible()
  },
)

// Inline usage-badge subcomponent. Lives in the same SFC so we don't
// have to introduce a brand-new file: the table is the only consumer
// on desktop. The mobile card renders its own compact variant inline.
const EndpointUsageBadges = defineComponent({
  name: 'EndpointUsageBadges',
  props: {
    endpointId: { type: String, required: true },
    t: { type: Function as PropType<TranslateFn>, required: true },
  },
  setup(props) {
    // Wrap the prop in a `computed` so the badge composable tracks
    // changes to `props.endpointId` reactively. UTable also gets a
    // stable `getRowId` below, but that only forces unmount/remount on
    // row key change — within a single row lifetime the prop can still
    // shift (e.g. after an inline edit) and the badge must re-evaluate
    // against the new endpoint id without a remount.
    const badges = useTokenPlanBadges(computed(() => props.endpointId))
    const pillBase =
      'inline-flex items-center rounded-full border border-default bg-elevated px-1.5 py-px text-[0.7rem] font-semibold whitespace-nowrap'

    return () => {
      const nodes = tokenPlanBadgePills(badges.value, props.t).map((pill) =>
        h(
          'span',
          { class: pillBase, style: { color: pill.color }, title: pill.title },
          pill.label,
        ),
      )
      if (nodes.length > 0) {
        return h('span', { class: 'inline-flex items-center gap-1' }, nodes)
      }
      // Cache not yet populated: render a dash so the row height stays
      // stable while the lazy prefetch resolves.
      return h('span', { class: 'text-xs text-muted' }, '—')
    }
  },
})
</script>

<template>
  <div class="hidden min-w-0 md:block">
    <UTable
      :data="items"
      :columns="columns"
      :loading="busy"
      :get-row-id="(row: EndpointListItemView) => row.endpoint_id"
      class="min-w-0"
    >
      <template #name-cell="{ row }">
        <EndpointNameCell
          :name="row.original.name"
          :base-url="row.original.base_url"
          :provider="row.original.provider"
        />
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
          <UBadge
            v-if="row.original.mcp_enabled"
            :label="t('minimaxMcp')"
            color="success"
          />
        </div>
      </template>
      <template v-if="showUsageColumn" #usage-cell="{ row }">
        <EndpointUsageBadges
          v-if="isQuotaProvider(row.original.provider)"
          :endpoint-id="row.original.endpoint_id"
          :t="t"
        />
        <span v-else class="text-xs text-muted">—</span>
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
          <UTooltip
            v-if="isQuotaProvider(row.original.provider)"
            :text="t('tokenPlanUsage')"
          >
            <UButton
              size="sm"
              color="neutral"
              variant="ghost"
              :aria-label="t('tokenPlanUsage')"
              @click="$emit('tokenPlanUsage', row.original.endpoint_id)"
            >
              <UIcon name="i-lucide-gauge" class="h-4 w-4" />
            </UButton>
          </UTooltip>
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
