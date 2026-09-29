<script setup lang="ts">
import { computed, watch } from 'vue'
import type { EndpointProvider } from '@/generated/admin-api'
import { formatMoney } from '@/composables/useTokenPlanWindowEntries'
import { useEndpointOrganizationUsage } from '@/composables/useEndpointOrganizationUsage'
import { isOrganizationUsageEligible } from '@/models/endpoints/quota'

const props = defineProps<{
  endpointId: string
  provider: EndpointProvider
  hasAdminApiKey: boolean
  t: TranslateFn
}>()

const {
  errorMessage,
  loadOrganizationUsage,
  organizationUsage,
  organizationUsageLoading,
  resetOrganizationUsage,
} = useEndpointOrganizationUsage()

const eligible = computed(
  () =>
    Boolean(props.endpointId) &&
    isOrganizationUsageEligible({
      provider: props.provider,
      has_admin_api_key: props.hasAdminApiKey,
    }),
)

function load(): void {
  void loadOrganizationUsage({
    endpointId: props.endpointId,
    provider: props.provider,
    has_admin_api_key: props.hasAdminApiKey,
  })
}

// A different endpoint (dialog reuse) must not keep the previous org's data.
watch(
  () => props.endpointId,
  () => resetOrganizationUsage(),
)

// Issue #589 P2c: the window is UTC month-to-date, so render the instants in
// UTC too instead of the operator's local timezone.
function formatUtc(value: string): string {
  return new Date(value).toLocaleString(undefined, { timeZone: 'UTC' })
}
</script>

<template>
  <div class="grid gap-2 rounded border border-default p-3 text-xs">
    <div class="flex flex-wrap items-start justify-between gap-2">
      <div class="flex items-center gap-2">
        <span class="text-xs font-medium text-default">{{
          t('endpointOrganizationUsage')
        }}</span>
      </div>
      <UButton
        v-if="eligible"
        type="button"
        size="xs"
        color="primary"
        variant="soft"
        icon="i-lucide-chart-column"
        :loading="organizationUsageLoading"
        @click="load"
        >{{
          organizationUsage
            ? t('endpointOrganizationUsageReload')
            : t('endpointOrganizationUsageLoad')
        }}</UButton
      >
    </div>
    <p class="leading-snug text-dimmed">
      {{ t('endpointOrganizationUsageHint') }}
    </p>
    <p v-if="!endpointId" class="text-dimmed">
      {{ t('endpointOrganizationUsageSaveFirst') }}
    </p>
    <p v-else-if="!eligible" class="text-dimmed">
      {{ t('endpointOrganizationUsageRequiredKey') }}
    </p>

    <p v-if="errorMessage" class="break-words text-error">
      {{ errorMessage }}
    </p>

    <div
      v-if="organizationUsage"
      class="grid gap-2 border-t border-default pt-2"
    >
      <p class="text-dimmed">
        {{
          t('endpointOrganizationUsagePeriod', {
            start: formatUtc(organizationUsage.period_start),
            end: formatUtc(organizationUsage.period_end),
          })
        }}
      </p>
      <div class="flex flex-wrap gap-x-4 gap-y-1">
        <span
          >{{ t('endpointOrganizationUsageInputTokens') }}:
          {{ organizationUsage.input_tokens.toLocaleString() }}</span
        >
        <span
          >{{ t('endpointOrganizationUsageOutputTokens') }}:
          {{ organizationUsage.output_tokens.toLocaleString() }}</span
        >
        <span class="font-semibold"
          >{{ t('endpointOrganizationUsageTotalTokens') }}:
          {{ organizationUsage.total_tokens.toLocaleString() }}</span
        >
        <span
          >{{ t('endpointOrganizationUsageCost') }}:
          {{
            formatMoney(organizationUsage.currency, organizationUsage.cost_usd)
          }}</span
        >
      </div>
      <p class="text-dimmed">
        {{ t('endpointOrganizationUsageFetchedAt') }}:
        {{ formatUtc(organizationUsage.fetched_at) }}
      </p>
      <div class="flex flex-wrap gap-2">
        <UBadge
          v-if="organizationUsage.cached"
          :label="t('endpointOrganizationUsageCached')"
          color="neutral"
          variant="subtle"
        />
        <UBadge
          v-if="organizationUsage.truncated"
          :label="t('endpointOrganizationUsageTruncated')"
          color="warning"
          variant="subtle"
        />
      </div>
    </div>
  </div>
</template>
