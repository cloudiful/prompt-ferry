<script setup lang="ts">
import { computed } from 'vue'
import type { ModelRouteTargetItemView } from '@/models/endpoints'
import {
  enabledRouteTargets,
  hiddenRouteTargetCount,
  routeTargetOverride,
  routeTargetPairLabel,
  routeTargetPreview,
} from '@/models/endpoints/model-route-item'

const props = defineProps<{
  targets: ModelRouteTargetItemView[]
  t: TranslateFn
}>()

type TargetPair = {
  target_id: string
  endpoint: string
  override: string | null
  label: string
}

function toPair(target: ModelRouteTargetItemView): TargetPair {
  return {
    target_id: target.target_id,
    endpoint: target.endpoint_label,
    override: routeTargetOverride(target),
    label: routeTargetPairLabel(target),
  }
}

// The configured order is kept, and a target on a disabled or deleted
// endpoint never reaches the summary.
const pairs = computed<TargetPair[]>(() =>
  enabledRouteTargets(props.targets).map(toPair),
)

const preview = computed<TargetPair[]>(() =>
  routeTargetPreview(props.targets).map(toPair),
)
const hidden = computed(() => hiddenRouteTargetCount(props.targets))
</script>

<template>
  <div class="min-w-0">
    <div v-if="!targets.length" class="text-xs text-dimmed">-</div>
    <div v-else-if="!pairs.length" class="text-xs text-dimmed">
      {{ t('modelRouteNoEnabledTargets') }}
    </div>
    <div
      v-else
      class="grid min-w-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-x-1.5 gap-y-1 md:gap-1.5"
      :class="
        preview.length > 1
          ? 'md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]'
          : 'md:grid-cols-[minmax(0,1fr)_auto]'
      "
    >
      <div class="row-span-2 grid min-w-0 gap-1 md:contents">
        <span
          v-for="pair in preview"
          :key="pair.target_id"
          class="inline-grid min-w-0 max-w-full w-fit items-center gap-1 overflow-hidden rounded-full border border-default bg-default px-2 py-px text-[0.73rem] leading-[1.35] text-default"
          :class="
            pair.override
              ? 'grid-cols-[minmax(0,auto)_auto_minmax(2rem,1fr)]'
              : 'grid-cols-[minmax(0,1fr)]'
          "
          :title="pair.label"
        >
          <span class="min-w-0 truncate">{{ pair.endpoint }}</span>
          <template v-if="pair.override">
            <span aria-hidden="true" class="text-muted">·</span>
            <span class="min-w-0 truncate text-muted">{{ pair.override }}</span>
          </template>
        </span>
      </div>
      <UPopover
        v-if="hidden"
        mode="click"
        :content="{
          side: 'bottom',
          align: 'start',
          sideOffset: 6,
          collisionPadding: 8,
        }"
      >
        <UButton
          type="button"
          size="xs"
          color="neutral"
          variant="subtle"
          class="col-start-2 row-start-2 w-fit shrink-0 md:col-start-auto md:row-start-auto"
          :aria-label="t('modelRouteMoreTargets', { count: hidden })"
          :title="t('modelRouteMoreTargetsHint')"
        >
          +{{ hidden }}
        </UButton>
        <template #content>
          <div
            class="max-h-[min(60vh,18rem)] w-[min(18rem,calc(100vw-2rem))] overflow-y-auto overflow-x-hidden p-1 text-sm"
          >
            <ul class="grid gap-2">
              <li
                v-for="pair in pairs"
                :key="pair.target_id"
                class="grid gap-0.5"
              >
                <span class="font-medium [overflow-wrap:anywhere]">{{
                  pair.endpoint
                }}</span>
                <span
                  v-if="pair.override"
                  class="text-xs text-muted [overflow-wrap:anywhere]"
                >
                  {{ pair.override }}
                </span>
              </li>
            </ul>
          </div>
        </template>
      </UPopover>
    </div>
  </div>
</template>
