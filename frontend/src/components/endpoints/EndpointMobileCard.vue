<script setup lang="ts">
import TestResultPopover from '@/components/shared/TestResultPopover.vue'
import type { EndpointListItemView } from '@/models/endpoints'

defineProps<{
  busy: boolean
  item: EndpointListItemView
  t: TranslateFn
}>()

defineEmits<{
  deleteEndpoint: [endpointId: string]
  editEndpoint: [endpointId: string]
  testEndpoint: [endpointId: string]
  tokenPlanUsage: [endpointId: string]
  toggleEndpointEnabled: [endpointId: string, enabled: boolean]
}>()
</script>

<template>
  <article class="grid gap-3 rounded-xl border border-default bg-default p-3">
    <div class="flex items-start justify-between gap-2">
      <div class="min-w-0">
        <div class="text-[0.88rem] leading-[1.2] font-bold text-highlighted">
          {{ item.name }}
        </div>
        <div class="mt-px break-words text-[0.7rem] leading-[1.35] text-dimmed">
          {{ item.base_url }}
        </div>
      </div>
    </div>

    <div class="grid gap-1.5">
      <div class="grid gap-px">
        <div
          class="text-[0.7rem] font-bold tracking-wide text-dimmed uppercase"
        >
          {{ t('status') }}
        </div>
        <div class="break-words text-[0.76rem] leading-[1.38] text-default">
          {{ item.scope_label }} / {{ item.native_api_label }} /
          {{ item.native_api_source_label }}
        </div>
      </div>
      <div v-if="item.owner_label" class="grid gap-px">
        <div
          class="text-[0.7rem] font-bold tracking-wide text-dimmed uppercase"
        >
          {{ t('user') }}
        </div>
        <div class="break-words text-[0.76rem] leading-[1.38] text-default">
          {{ item.owner_label }}
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
            $emit('toggleEndpointEnabled', item.endpoint_id, $event)
          "
        />
      </label>
    </div>

    <div
      class="grid gap-2 md:grid-cols-2 [&>button]:w-full [&>button]:justify-center"
    >
      <UButton
        v-if="item.provider === 'minimax'"
        size="sm"
        color="neutral"
        variant="outline"
        @click="$emit('tokenPlanUsage', item.endpoint_id)"
      >
        <UIcon name="i-lucide-gauge" class="h-4 w-4" />
        {{ t('tokenPlanUsage') }}
      </UButton>
      <UButton
        size="sm"
        color="neutral"
        variant="outline"
        :loading="item.testing"
        @click="$emit('testEndpoint', item.endpoint_id)"
        >{{ t('test') }}</UButton
      >
      <UButton
        size="sm"
        color="neutral"
        variant="outline"
        @click="$emit('editEndpoint', item.endpoint_id)"
        >{{ t('edit') }}</UButton
      >
      <UButton
        size="sm"
        color="error"
        variant="outline"
        :loading="busy"
        @click="$emit('deleteEndpoint', item.endpoint_id)"
        >{{ t('delete') }}</UButton
      >
    </div>
  </article>
</template>
