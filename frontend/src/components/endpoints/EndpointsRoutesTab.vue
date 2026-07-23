<script setup lang="ts">
import { computed } from 'vue'
import type { EndpointsWorkspaceView } from '@/models/endpoints'
import WorkspacePagerBar from '@/components/shared/WorkspacePagerBar.vue'
import ModelRouteMobileCard from './ModelRouteMobileCard.vue'
import ModelRoutesTable from './ModelRoutesTable.vue'

const props = defineProps<{
  t: TranslateFn
  workspace: EndpointsWorkspaceView
}>()

const emit = defineEmits<{
  editModelRoute: [ruleId: string]
  deleteModelRoute: [ruleId: string]
  testModelRoute: [ruleId: string]
  modelRoutePage: [event: TablePageChange]
  toggleModelRouteEnabled: [ruleId: string, enabled: boolean]
}>()

const canPrev = computed(() => props.workspace.model_route_first > 0)
const canNext = computed(
  () =>
    props.workspace.model_route_first + props.workspace.model_route_rows <
    props.workspace.model_route_total,
)
const pageStart = computed(() =>
  props.workspace.model_route_total === 0
    ? 0
    : props.workspace.model_route_first + 1,
)
const pageEnd = computed(() =>
  Math.min(
    props.workspace.model_route_first + props.workspace.model_route_rows,
    props.workspace.model_route_total,
  ),
)
function buildPageEvent(nextFirst: number): TablePageChange {
  const rows = props.workspace.model_route_rows
  const total = props.workspace.model_route_total
  return {
    first: nextFirst,
    page: Math.floor(nextFirst / rows),
    pageCount: Math.max(1, Math.ceil(total / rows)),
    rows,
  } as TablePageChange
}

function page(direction: 'prev' | 'next'): void {
  const nextFirst =
    direction === 'prev'
      ? Math.max(
          0,
          props.workspace.model_route_first - props.workspace.model_route_rows,
        )
      : props.workspace.model_route_first + props.workspace.model_route_rows
  emit('modelRoutePage', buildPageEvent(nextFirst))
}

function forwardToggleModelRouteEnabled(
  ruleId: string,
  enabled: boolean,
): void {
  emit('toggleModelRouteEnabled', ruleId, enabled)
}
</script>

<template>
  <div class="grid gap-3">
    <div class="grid gap-3 md:hidden">
      <template v-if="workspace.model_route_items.length">
        <ModelRouteMobileCard
          v-for="item in workspace.model_route_items"
          :key="item.rule_id"
          :busy="workspace.busy"
          :item="item"
          :t="t"
          @delete-model-route="$emit('deleteModelRoute', $event)"
          @edit-model-route="$emit('editModelRoute', $event)"
          @test-model-route="$emit('testModelRoute', $event)"
          @toggle-model-route-enabled="forwardToggleModelRouteEnabled"
        />

        <WorkspacePagerBar
          v-if="workspace.model_route_total > workspace.model_route_rows"
          :can-next="canNext"
          :can-prev="canPrev"
          :end="pageEnd"
          :start="pageStart"
          :total="workspace.model_route_total"
          @next="page('next')"
          @prev="page('prev')"
        />
      </template>
      <div
        v-else
        class="rounded-xl border border-default bg-default px-4 py-5 text-sm text-dimmed"
      >
        {{ t('noModelRoutes') }}
      </div>
    </div>

    <ModelRoutesTable
      :busy="workspace.busy"
      :first="workspace.model_route_first"
      :items="workspace.model_route_items"
      :rows="workspace.model_route_rows"
      :t="t"
      :total="workspace.model_route_total"
      @delete-model-route="$emit('deleteModelRoute', $event)"
      @edit-model-route="$emit('editModelRoute', $event)"
      @model-route-page="$emit('modelRoutePage', $event)"
      @test-model-route="$emit('testModelRoute', $event)"
      @toggle-model-route-enabled="forwardToggleModelRouteEnabled"
    />
  </div>
</template>
