<script setup lang="ts">
import { computed, defineComponent, h, type PropType } from 'vue'
import TestResultPopover from '@/components/shared/TestResultPopover.vue'
import {
  progressColor,
  useTokenPlanBadges,
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

function badgeColorForPercent(percent: number): string {
  return progressColor({
    end_at: null,
    remaining_percent: percent,
  })
}

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
        'inline-flex items-center rounded-full border border-default bg-elevated px-2 py-px text-[0.74rem] font-semibold whitespace-nowrap'
      const shortPill =
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
          : null
      const longPill =
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
          : null
      if (isOpenRouter.value) {
        const label =
          badges.value.openrouterRemaining !== null
            ? `${t('tokenPlanOpenRouterRemaining')} ${badges.value.openrouterRemaining.toFixed(0)}%`
            : t('tokenPlanNoQuota')
        const openrouterColor =
          badges.value.openrouterRemaining !== null
            ? badgeColorForPercent(badges.value.openrouterRemaining)
            : ''
        return h(
          'span',
          { class: 'inline-flex items-center gap-1' },
          h(
            'span',
            {
              class: pillBase,
              style: { color: openrouterColor },
              title: openrouterLabel.value,
            },
            label,
          ),
        )
      }
      if (shortPill || longPill) {
        return h('span', { class: 'flex flex-wrap items-center gap-1' }, [
          shortPill,
          longPill,
        ])
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
