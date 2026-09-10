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
import TablePagination from '@/components/shared/TablePagination.vue'
import TestResultPopover from '@/components/shared/TestResultPopover.vue'
import {
  progressColor,
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

// `progressColor` expects a TokenPlanWindowUsage; the badge already
// holds the remaining percent, so we synthesize one with
// `remaining_percent = percent` and let progressColor fold used =
// 100 - remaining internally to drive the hue ramp.
function badgeColorForPercent(percent: number): string {
  return progressColor({
    end_at: null,
    remaining_percent: percent,
  })
}

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
    const isOpenRouter = computed(
      () =>
        badges.value.openrouterLimit !== null ||
        badges.value.openrouterLimitRemaining !== null,
    )
    const openrouterLabel = computed(() => {
      const lim = badges.value.openrouterLimitRemaining
      const cap = badges.value.openrouterLimit
      if (lim === null || cap === null) return ''
      return `${lim.toFixed(2)} / ${cap.toFixed(2)}`
    })
    return () => {
      const t = props.t
      const pillBase =
        'inline-flex items-center rounded-full border border-default bg-elevated px-1.5 py-px text-[0.7rem] font-semibold whitespace-nowrap'
      // OpenRouter path: balance pill with percent, falls back to "—"
      // when the key has no finite cap (free tier / missing numbers).
      if (isOpenRouter.value) {
        const label =
          badges.value.openrouterRemaining !== null
            ? `${t('tokenPlanOpenRouterRemaining')} ${badges.value.openrouterRemaining.toFixed(0)}%`
            : t('tokenPlanNoQuota')
        return h(
          'span',
          { class: 'inline-flex items-center gap-1' },
          h(
            'span',
            {
              class: pillBase,
              style: {
                color:
                  badges.value.openrouterRemaining !== null
                    ? badgeColorForPercent(badges.value.openrouterRemaining)
                    : '',
              },
              title: openrouterLabel.value,
            },
            label,
          ),
        )
      }
      // Quota-bearing providers: dual pill row (short + long); each
      // slot degrades to empty so a provider reporting only one window
      // still renders the row.
      const children = [
        badges.value.short !== null
          ? h(
              'span',
              {
                class: pillBase,
                style: { color: badgeColorForPercent(badges.value.short) },
                title: t('tokenPlanShortBadgeHint'),
              },
              `${t('tokenPlanShortBadge')} ${badges.value.short.toFixed(0)}%`,
            )
          : null,
        badges.value.long !== null
          ? h(
              'span',
              {
                class: pillBase,
                style: { color: badgeColorForPercent(badges.value.long) },
                title: t('tokenPlanLongBadgeHint'),
              },
              `${t('tokenPlanLongBadge')} ${badges.value.long.toFixed(0)}%`,
            )
          : null,
      ]
      if (children[0] || children[1]) {
        return h('span', { class: 'inline-flex items-center gap-1' }, children)
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
        <div class="min-w-0 max-w-40">
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
