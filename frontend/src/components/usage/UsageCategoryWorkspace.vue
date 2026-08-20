<script setup lang="ts">
import { defineAsyncComponent } from 'vue'
import type {
  RequestRecordCategory,
  RequestRecordOverviewResponse,
} from '@/generated/admin-api'
import type { RequestRecordFormatting } from '@/models/request-record-formatting'
import type {
  RequestOverviewMode,
  RequestOverviewDrilldown,
} from '@/request-overview'

defineProps<{
  activeMode: RequestOverviewMode
  category: RequestRecordCategory
  formatting: RequestRecordFormatting
  loading: boolean
  overview: RequestRecordOverviewResponse | null
  t: TranslateFn
}>()

defineEmits<{
  drilldown: [filter: RequestOverviewDrilldown]
}>()

const RequestOverviewPanel = defineAsyncComponent(
  () => import('../RequestOverviewPanel.vue'),
)
</script>

<template>
  <section class="grid gap-4">
    <RequestOverviewPanel
      v-if="activeMode === 'overview'"
      :overview="overview"
      :loading="loading"
      :category="category"
      :formatting="formatting"
      :t="t"
      @drilldown="$emit('drilldown', $event)"
    />
    <slot v-else name="records" />
  </section>
</template>
