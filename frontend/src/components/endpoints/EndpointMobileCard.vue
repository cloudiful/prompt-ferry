<script setup lang="ts">
import { computed, defineComponent, h, type PropType } from 'vue'
import TestResultPopover from '@/components/shared/TestResultPopover.vue'
import { useTokenPlanBadgeRotation } from '@/composables/useTokenPlanBadgeRotation'
import {
  tokenPlanBadgePills,
  useTokenPlanBadges,
  type TokenPlanKeyBadges,
} from '@/composables/useTokenPlanBadges'
import { prefetchTokenPlanBatch } from '@/composables/useTokenPlanUsageCache'
import type { EndpointListItemView } from '@/models/endpoints'

const props = defineProps<{
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

// Same provider gate as the desktop table — non-quota providers fall
// back to a "—" pill in the usage row.
const QUOTA_PROVIDERS = new Set<EndpointListItemView['provider']>([
  'minimax',
  'command_code',
  'opencode_go',
  'openrouter',
  'glm',
])

const isQuotaProvider = computed(() => QUOTA_PROVIDERS.has(props.item.provider))

// Kick off the prefetch when the card mounts so mobile renders the
// badges within one round-trip of the table mount. The cache layer
// already dedupes against the table's prefetch, so this is free.
prefetchTokenPlanBatch([props.item.endpoint_id], 4)

// Inline usage-badge subcomponent for the mobile card. Compact pill
// pair stacked vertically so the wider card surface can afford the
// verbose "短窗 42% / 长窗 73%" labels; OpenRouter collapses to a
// single balance pill + the "无额度" fallback when no cap is set.
const EndpointMobileUsageBadges = defineComponent({
  name: 'EndpointMobileUsageBadges',
  props: {
    endpointId: { type: String, required: true },
    t: { type: Function as PropType<TranslateFn>, required: true },
  },
  setup(props) {
    // Wrap the prop in a `computed` so the badge composable tracks
    // changes to `props.endpointId` reactively. Within the card's
    // lifetime (same v-for item, swap of `item.endpoint_id` after an
    // edit) the badge must re-evaluate against the new endpoint id
    // without a remount.
    const badges = useTokenPlanBadges(computed(() => props.endpointId))
    const keyBadges = computed(() => badges.value.keys)
    const rotation = useTokenPlanBadgeRotation(keyBadges)
    // Same rule as the desktop table: rotate only when at least two keys
    // report usable numbers; otherwise keep the aggregate pill pair.
    const rotating = computed(
      () => keyBadges.value.filter((key) => key.ok).length > 1,
    )
    const pillBase =
      'inline-flex items-center rounded-full border border-default bg-elevated px-2 py-px text-[0.74rem] font-semibold whitespace-nowrap'

    function pillNodes(source: TokenPlanKeyBadges) {
      return tokenPlanBadgePills(source, props.t).map((pill) =>
        h(
          'span',
          { class: pillBase, style: { color: pill.color }, title: pill.title },
          pill.label,
        ),
      )
    }

    return () => {
      if (rotating.value) {
        // #277 P3: crossfade across keys inside the usage row. Each key
        // keeps its own wrapping pill pair; the active cell is visible,
        // failed keys are skipped by the rotation and stay hidden.
        const cells = keyBadges.value.map((key, index) => {
          const nodes = pillNodes(key)
          return h(
            'span',
            {
              class: [
                'col-start-1 row-start-1 flex flex-wrap items-center gap-1 transition-opacity duration-500',
                index === rotation.index
                  ? 'opacity-100'
                  : 'pointer-events-none opacity-0',
              ],
              'aria-hidden': index !== rotation.index,
              title: key.keyLabel,
            },
            nodes.length > 0
              ? nodes
              : h('span', { class: 'text-[0.74rem] text-muted' }, '—'),
          )
        })
        return h(
          'span',
          {
            class: 'inline-grid items-center',
            onMouseenter: () => rotation.setPaused(true),
            onMouseleave: () => rotation.setPaused(false),
          },
          cells,
        )
      }
      const nodes = pillNodes(badges.value)
      if (nodes.length > 0) {
        return h('span', { class: 'flex flex-wrap items-center gap-1' }, nodes)
      }
      return h('span', { class: 'text-[0.74rem] text-muted' }, '—')
    }
  },
})
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
      <div v-if="isQuotaProvider" class="grid gap-px min-w-0">
        <div
          class="text-[0.7rem] font-bold tracking-wide text-dimmed uppercase"
        >
          {{ t('tokenPlanUsage') }}
        </div>
        <div class="min-w-0">
          <EndpointMobileUsageBadges :endpoint-id="item.endpoint_id" :t="t" />
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
        v-if="isQuotaProvider"
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
