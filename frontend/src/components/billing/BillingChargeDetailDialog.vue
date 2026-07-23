<script setup lang="ts">
import type { BillingChargeDetailResponse } from '@/generated/admin-api'
import { formatBillingAmount, formatBillingTime, formatTokenCount } from '@/models/billing'

const props = defineProps<{
  detail: BillingChargeDetailResponse | null
  isAdmin: boolean
  loading: boolean
  t: TranslateFn
}>()

const visible = defineModel<boolean>('visible', { required: true })
const adjustmentAmount = defineModel<string>('adjustmentAmount', { required: true })
const adjustmentReason = defineModel<string>('adjustmentReason', { required: true })

defineEmits<{
  addAdjustment: []
}>()
</script>

<template>
  <UModal v-model:open="visible" :title="t('billingDetails')" :ui="{ content: 'sm:max-w-4xl', body: 'max-h-[80vh] overflow-y-auto' }">
    <template #body>
      <div v-if="loading" class="py-6 text-sm text-dimmed">{{ t('loading') }}</div>
      <div v-else-if="detail" class="grid gap-4 text-xs">
        <section class="grid gap-2 rounded-lg border border-default p-3">
          <div class="flex flex-wrap items-center gap-2">
            <UBadge :label="detail.charge.pricing_status === 'adjusted' ? t('pricingAdjusted') : detail.charge.pricing_status === 'priced' ? t('pricingPriced') : detail.charge.usage_status === 'unknown' ? t('usageUnknown') : t('pricingUnpriced')" variant="subtle" />
            <span class="text-dimmed">{{ detail.charge.request_id }}</span>
          </div>
          <div class="grid gap-2 sm:grid-cols-3">
            <div><span class="text-dimmed">{{ t('usageTokens') }}</span><strong class="mt-1 block">{{ formatTokenCount(detail.charge.input_tokens) }} / {{ formatTokenCount(detail.charge.cache_read_tokens) }} / {{ formatTokenCount(detail.charge.cache_write_tokens) }} / {{ formatTokenCount(detail.charge.output_tokens) }}</strong></div>
            <div><span class="text-dimmed">{{ t('chargeAmount') }}</span><strong class="mt-1 block">{{ formatBillingAmount(detail.charge.adjusted_amount, detail.charge.currency) }}</strong></div>
            <div v-if="isAdmin"><span class="text-dimmed">{{ t('grossMargin') }}</span><strong class="mt-1 block">{{ formatBillingAmount(detail.charge.gross_margin, detail.charge.currency) }}</strong></div>
          </div>
          <div v-if="isAdmin" class="grid gap-2 sm:grid-cols-2 text-dimmed">
            <span>{{ t('endpoint') }}: {{ detail.charge.endpoint_name || detail.charge.endpoint_id || '-' }}</span>
            <span>{{ t('upstreamKey') }}: {{ detail.charge.endpoint_key_id || '-' }}</span>
          </div>
          <div class="text-dimmed">{{ formatBillingTime(detail.charge.created_at) }}</div>
        </section>
        <section class="grid gap-2">
          <h3 class="font-semibold text-highlighted">{{ t('billingLines') }}</h3>
          <div v-for="line in detail.lines" :key="line.line_id" class="flex flex-wrap items-center justify-between gap-2 border-b border-default py-2">
            <span>{{ line.price_side }} / {{ line.meter }} / {{ formatTokenCount(line.token_count) }}</span>
            <span>{{ formatBillingAmount(line.amount) }} ({{ formatBillingAmount(line.unit_rate) }} / M)</span>
          </div>
        </section>
        <section v-if="detail.adjustments.length" class="grid gap-2">
          <h3 class="font-semibold text-highlighted">{{ t('billingAdjustments') }}</h3>
          <div v-for="adjustment in detail.adjustments" :key="adjustment.adjustment_id" class="grid gap-1 border-b border-default py-2">
            <div class="flex justify-between gap-2"><span>{{ adjustment.reason }}</span><strong>{{ formatBillingAmount(adjustment.amount) }}</strong></div>
            <span class="text-dimmed">{{ formatBillingTime(adjustment.created_at) }}</span>
          </div>
        </section>
        <form v-if="isAdmin" class="grid gap-2 border-t border-default pt-3" @submit.prevent="$emit('addAdjustment')">
          <h3 class="font-semibold text-highlighted">{{ t('addAdjustment') }}</h3>
          <div class="grid gap-2 sm:grid-cols-2">
            <UInput v-model="adjustmentAmount" :placeholder="t('adjustmentAmount')" inputmode="decimal" required />
            <UInput v-model="adjustmentReason" :placeholder="t('adjustmentReason')" required />
          </div>
          <div><UButton type="submit" icon="i-lucide-plus">{{ t('addAdjustment') }}</UButton></div>
        </form>
      </div>
    </template>
  </UModal>
</template>
