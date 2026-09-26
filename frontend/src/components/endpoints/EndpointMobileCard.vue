<script setup lang="ts">
import { computed, defineComponent, h, watch, type PropType } from 'vue'
import ProviderIcon from '@/components/providers/ProviderIcon.vue'
import TestResultPopover from '@/components/shared/TestResultPopover.vue'
import {
  tokenPlanBadgePills,
  useTokenPlanBadges,
} from '@/composables/useTokenPlanBadges'
import { prefetchTokenPlanBatch } from '@/composables/useTokenPlanUsageCache'
import type { EndpointListItemView } from '@/models/endpoints'
import { isQuotaEligible } from '@/models/endpoints/quota'

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

// Same shared eligibility gate as the desktop table: non-quota rows keep
// the "—" fallback, and an OpenAI endpoint without a stored subscription
// token never renders or fetches subscription quota.
const quotaEligible = computed(() => isQuotaEligible(props.item))

// Kick off the prefetch when the card mounts or the row becomes eligible
// (OAuth login) so mobile renders the badges within one round-trip of the
// table mount. The cache layer already dedupes against the table's
// prefetch, so this is free.
watch(
  quotaEligible,
  (eligible) => {
    if (eligible) void prefetchTokenPlanBatch([props.item.endpoint_id], 4)
  },
  { immediate: true },
)

// Inline usage-badge subcomponent for the mobile card. Compact pill
// pair wrapping across the row so the wider card surface can afford the
// verbose "短窗 42% / 长窗 73%" labels; balance providers pair their
// balance with a today-usage pill.
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
    const pillBase =
      'inline-flex items-center rounded-full border border-default bg-elevated px-2 py-px text-[0.74rem] font-semibold whitespace-nowrap'

    return () => {
      const nodes = tokenPlanBadgePills(badges.value, props.t).map((pill) =>
        h(
          'span',
          { class: pillBase, style: { color: pill.color }, title: pill.title },
          pill.label,
        ),
      )
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
      <div class="flex min-w-0 items-center gap-1.5">
        <ProviderIcon :provider="item.provider" size="md" />
        <div class="min-w-0">
          <div class="text-[0.88rem] leading-[1.2] font-bold text-highlighted">
            {{ item.name }}
          </div>
          <div
            class="mt-px break-words text-[0.7rem] leading-[1.35] text-dimmed"
          >
            {{ item.base_url }}
          </div>
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
      <div v-if="quotaEligible" class="grid gap-px min-w-0">
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
        v-if="quotaEligible"
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
