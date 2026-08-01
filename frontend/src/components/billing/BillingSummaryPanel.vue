<script setup lang="ts">
import type { BillingSummaryResponse } from '@/generated/admin-api'
import { formatBillingAmount, formatTokenCount } from '@/models/billing'

const props = defineProps<{
  summary: BillingSummaryResponse | null
  t: TranslateFn
}>()

function amount(value: string | null | undefined): string {
  return formatBillingAmount(value, props.summary?.currency ?? 'CNY')
}
</script>

<template>
  <section class="grid gap-3">
    <div class="grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
      <div class="rounded-lg border border-default bg-default p-3">
        <div class="text-xs text-muted">{{ t('billingAmount') }}</div>
        <strong class="mt-1 block text-xl text-highlighted">{{
          amount(summary?.customer_amount)
        }}</strong>
      </div>
      <div class="rounded-lg border border-default bg-default p-3">
        <div class="text-xs text-muted">{{ t('billingRequests') }}</div>
        <strong class="mt-1 block text-xl text-highlighted">{{
          formatTokenCount(summary?.request_count ?? 0)
        }}</strong>
      </div>
      <div class="rounded-lg border border-default bg-default p-3">
        <div class="text-xs text-muted">{{ t('billingKnown') }}</div>
        <strong class="mt-1 block text-xl text-highlighted">{{
          formatTokenCount(summary?.known_count ?? 0)
        }}</strong>
      </div>
    </div>
    <div class="flex flex-wrap items-center gap-2 text-xs text-muted">
      <UBadge
        :label="`${t('billingPriced')}: ${summary?.priced_count ?? 0}`"
        color="success"
        variant="subtle"
      />
      <UBadge
        :label="`${t('billingUnpriced')}: ${summary?.unpriced_count ?? 0}`"
        color="warning"
        variant="subtle"
      />
      <UBadge
        :label="`${t('billingUnknown')}: ${summary?.unknown_count ?? 0}`"
        color="neutral"
        variant="subtle"
      />
    </div>
  </section>
</template>
