<script setup lang="ts">
import { computed } from 'vue'
import type { EndpointsWorkspaceView } from '@/models/endpoints'
import WorkspacePagerBar from '@/components/shared/WorkspacePagerBar.vue'
import EndpointMobileCard from './EndpointMobileCard.vue'
import EndpointsTable from './EndpointsTable.vue'

const props = defineProps<{
  t: TranslateFn
  workspace: EndpointsWorkspaceView
}>()

const emit = defineEmits<{
  editEndpoint: [endpointId: string]
  deleteEndpoint: [endpointId: string]
  testEndpoint: [endpointId: string]
  endpointPage: [event: TablePageChange]
  toggleEndpointEnabled: [endpointId: string, enabled: boolean]
}>()

const canPrev = computed(() => props.workspace.endpoint_first > 0)
const canNext = computed(
  () =>
    props.workspace.endpoint_first + props.workspace.endpoint_rows <
    props.workspace.endpoint_total,
)
const pageStart = computed(() =>
  props.workspace.endpoint_total === 0 ? 0 : props.workspace.endpoint_first + 1,
)
const pageEnd = computed(() =>
  Math.min(
    props.workspace.endpoint_first + props.workspace.endpoint_rows,
    props.workspace.endpoint_total,
  ),
)
function buildPageEvent(nextFirst: number): TablePageChange {
  const rows = props.workspace.endpoint_rows
  const total = props.workspace.endpoint_total
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
          props.workspace.endpoint_first - props.workspace.endpoint_rows,
        )
      : props.workspace.endpoint_first + props.workspace.endpoint_rows
  emit('endpointPage', buildPageEvent(nextFirst))
}

function forwardToggleEndpointEnabled(
  endpointId: string,
  enabled: boolean,
): void {
  emit('toggleEndpointEnabled', endpointId, enabled)
}
</script>

<template>
  <div class="grid gap-3">
    <div class="grid gap-3 md:hidden">
      <EndpointMobileCard
        v-for="item in workspace.endpoint_items"
        :key="item.endpoint_id"
        :busy="workspace.busy"
        :item="item"
        :t="t"
        @delete-endpoint="$emit('deleteEndpoint', $event)"
        @edit-endpoint="$emit('editEndpoint', $event)"
        @test-endpoint="$emit('testEndpoint', $event)"
        @toggle-endpoint-enabled="forwardToggleEndpointEnabled"
      />

      <WorkspacePagerBar
        v-if="workspace.endpoint_total > workspace.endpoint_rows"
        :can-next="canNext"
        :can-prev="canPrev"
        :end="pageEnd"
        :start="pageStart"
        :total="workspace.endpoint_total"
        @next="page('next')"
        @prev="page('prev')"
      />
    </div>

    <EndpointsTable
      :busy="workspace.busy"
      :first="workspace.endpoint_first"
      :items="workspace.endpoint_items"
      :rows="workspace.endpoint_rows"
      :t="t"
      :total="workspace.endpoint_total"
      @delete-endpoint="$emit('deleteEndpoint', $event)"
      @edit-endpoint="$emit('editEndpoint', $event)"
      @endpoint-page="$emit('endpointPage', $event)"
      @test-endpoint="$emit('testEndpoint', $event)"
      @toggle-endpoint-enabled="forwardToggleEndpointEnabled"
    />
  </div>
</template>
