<script setup lang="ts">
import { computed } from 'vue'
import TestResultPopover from '@/components/shared/TestResultPopover.vue'
import type { ModelRouteListItemView } from '@/models/endpoints'

const props = defineProps<{
  busy: boolean
  item: ModelRouteListItemView
  t: TranslateFn
}>()

defineEmits<{
  deleteModelRoute: [ruleId: string]
  editModelRoute: [ruleId: string]
  testModelRoute: [ruleId: string]
  toggleModelRouteEnabled: [ruleId: string, enabled: boolean]
}>()

const targetsLabel = computed(
  () =>
    props.item.targets
      .map((target) =>
        target.upstream_model
          ? `${target.endpoint_label} / ${target.upstream_model}`
          : target.endpoint_label,
      )
      .join(', ') || '-',
)
</script>

<template>
  <article class="grid gap-3 rounded-xl border border-default bg-default p-3">
    <div class="flex items-start justify-between gap-2">
      <div class="min-w-0">
        <div class="text-[0.88rem] leading-[1.2] font-bold text-highlighted">
          {{ item.model_pattern }}
        </div>
        <div class="mt-px break-words text-[0.7rem] leading-[1.35] text-dimmed">
          {{ item.routing_strategy_label }}
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
        <div class="break-words text-[0.76rem] leading-[1.38] text-default">
          {{ targetsLabel }}
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
          />
        </div>
      </div>
    </div>

    <div class="grid gap-1">
      <label class="inline-flex flex-none items-center whitespace-nowrap">
        <USwitch
          :model-value="item.enabled"
          :aria-label="t('status')"
          :disabled="busy || item.toggling"
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
        :loading="busy"
        @click="$emit('deleteModelRoute', item.rule_id)"
        >{{ t('delete') }}</UButton
      >
    </div>
  </article>
</template>
