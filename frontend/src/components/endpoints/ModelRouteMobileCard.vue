<script setup lang="ts">
import { computed, ref } from 'vue'
import TestResultPopover from '@/components/shared/TestResultPopover.vue'
import type { ModelRouteListItemView } from '@/models/endpoints'
import { modelRouteTestCounts } from '@/models/endpoints/model-route-item'
import ModelRouteTargetsSummary from './ModelRouteTargetsSummary.vue'
import ModelRouteTestResultsDialog from './ModelRouteTestResultsDialog.vue'

const props = withDefaults(
  defineProps<{
    busy: boolean
    item: ModelRouteListItemView
    pendingKeys?: Readonly<Record<string, string>>
    t: TranslateFn
  }>(),
  { pendingKeys: () => ({}) },
)

// Same shared gate as the desktop table: save, delete, and toggle of one route
// hold one key, and only the action holding it spins.
function isPending(): boolean {
  return Object.hasOwn(props.pendingKeys, props.item.rule_id)
}

function pendingAction(): string {
  return props.pendingKeys[props.item.rule_id] ?? ''
}

defineEmits<{
  deleteModelRoute: [ruleId: string]
  editModelRoute: [ruleId: string]
  testModelRoute: [ruleId: string]
  toggleModelRouteEnabled: [ruleId: string, enabled: boolean]
}>()

const detailsOpen = ref(false)

// The card shows the counts next to the icon, so the result is readable
// without relying on the color alone.
const countsLabel = computed(() => {
  const counts = modelRouteTestCounts(props.item.test_results)
  return counts.total ? `${counts.passed}/${counts.total}` : ''
})
</script>

<template>
  <article class="grid gap-3 rounded-xl border border-default bg-default p-3">
    <div class="flex items-start justify-between gap-2">
      <div class="min-w-0">
        <div class="text-[0.88rem] leading-[1.2] font-bold text-highlighted">
          {{ item.model_pattern }}
        </div>
      </div>
    </div>

    <div class="grid gap-1.5">
      <div class="grid gap-px">
        <div
          class="text-[0.7rem] font-bold tracking-wide text-dimmed uppercase"
        >
          {{ t('endpoint') }}
        </div>
        <div class="min-w-0 text-[0.76rem] leading-[1.38] text-default">
          <ModelRouteTargetsSummary :targets="item.targets" :t="t" />
        </div>
      </div>
      <div class="grid gap-px">
        <div
          class="text-[0.7rem] font-bold tracking-wide text-dimmed uppercase"
        >
          {{ t('status') }}
        </div>
        <div class="break-words text-[0.76rem] leading-[1.38] text-default">
          {{ item.scope_label
          }}{{ item.owner_label ? ` / ${item.owner_label}` : '' }}
        </div>
      </div>
      <div class="grid gap-px min-w-0">
        <div
          class="text-[0.7rem] font-bold tracking-wide text-dimmed uppercase"
        >
          {{ t('test') }}
        </div>
        <div class="min-w-0">
          <TestResultPopover
            :message="item.test_message"
            :severity="item.test_severity"
            :detail-label="countsLabel"
            interactive
            @select="detailsOpen = true"
          />
        </div>
      </div>
    </div>

    <div class="grid gap-1">
      <label class="inline-flex flex-none items-center whitespace-nowrap">
        <USwitch
          :model-value="item.enabled"
          :aria-label="t('status')"
          :disabled="busy || item.toggling || isPending()"
          :loading="pendingAction() === 'toggle'"
          @update:model-value="
            $emit('toggleModelRouteEnabled', item.rule_id, $event)
          "
        />
      </label>
    </div>

    <div
      class="grid gap-2 md:grid-cols-2 [&>button]:w-full [&>button]:justify-center"
    >
      <UButton
        size="sm"
        color="neutral"
        variant="outline"
        :loading="item.testing"
        @click="$emit('testModelRoute', item.rule_id)"
        >{{ t('test') }}</UButton
      >
      <UButton
        size="sm"
        color="neutral"
        variant="outline"
        @click="$emit('editModelRoute', item.rule_id)"
        >{{ t('edit') }}</UButton
      >
      <UButton
        size="sm"
        color="error"
        variant="outline"
        :loading="pendingAction() === 'delete'"
        :disabled="isPending()"
        @click="$emit('deleteModelRoute', item.rule_id)"
        >{{ t('delete') }}</UButton
      >
    </div>

    <ModelRouteTestResultsDialog
      v-model:visible="detailsOpen"
      :results="item.test_results"
      :t="t"
    />
  </article>
</template>
