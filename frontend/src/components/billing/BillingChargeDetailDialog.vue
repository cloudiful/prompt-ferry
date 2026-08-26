<script setup lang="ts">
import type { BillingChargeDetailResponse } from '@/generated/admin-api'
import { formatTokenQuantity } from '@/composables/useUsageFormatting'
import { formatBillingAmount, formatBillingTime } from '@/models/billing'

const props = defineProps<{
  detail: BillingChargeDetailResponse | null
  isAdmin: boolean
  loading: boolean
  t: TranslateFn
}>()

const visible = defineModel<boolean>('visible', { required: true })
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="t('billingDetails')"
    :ui="{ content: 'sm:max-w-4xl', body: 'max-h-[80vh] overflow-y-auto' }"
  >
    <template #body>
      <div v-if="loading" class="py-6 text-sm text-dimmed">
        {{ t('loading') }}
      </div>
      <div v-else-if="detail" class="grid gap-4 text-xs">
        <section class="grid gap-2 rounded-lg border border-default p-3">
          <div class="flex flex-wrap items-center gap-2">
            <UBadge
              :label="
                detail.charge.pricing_status === 'priced'
                  ? t('pricingPriced')
                  : detail.charge.usage_status === 'unknown'
                    ? t('usageUnknown')
                    : t('pricingUnpriced')
              "
              variant="subtle"
            />
            <span class="text-dimmed">{{ detail.charge.request_id }}</span>
          </div>
          <div class="grid gap-2 sm:grid-cols-2">
            <div>
              <span class="text-dimmed">{{ t('usageTokens') }}</span
              ><strong class="mt-1 block"
                >{{ formatTokenQuantity(detail.charge.input_tokens) }} /
                {{ formatTokenQuantity(detail.charge.cache_read_tokens) }} /
                {{ formatTokenQuantity(detail.charge.cache_write_tokens) }} /
                {{ formatTokenQuantity(detail.charge.output_tokens) }}</strong
              >
            </div>
            <div>
              <span class="text-dimmed">{{ t('chargeAmount') }}</span
              ><strong class="mt-1 block">{{
                formatBillingAmount(
                  detail.charge.customer_amount,
                  detail.charge.currency,
                )
              }}</strong>
            </div>
          </div>
          <div v-if="isAdmin" class="grid gap-2 sm:grid-cols-2 text-dimmed">
            <span
              >{{ t('endpoint') }}:
              {{
                detail.charge.endpoint_name || detail.charge.endpoint_id || '-'
              }}</span
            >
            <span
              >{{ t('upstreamKey') }}:
              {{ detail.charge.endpoint_key_id || '-' }}</span
            >
          </div>
          <div class="text-dimmed">
            {{ formatBillingTime(detail.charge.created_at) }}
          </div>
        </section>
        <section class="grid gap-2">
          <h3 class="font-semibold text-highlighted">
            {{ t('billingLines') }}
          </h3>
          <div
            v-for="line in detail.lines"
            :key="line.line_id"
            class="flex flex-wrap items-center justify-between gap-2 border-b border-default py-2"
          >
            <span
              >{{ line.meter }} /
              {{ formatTokenQuantity(line.token_count) }}</span
            >
            <span
              >{{ formatBillingAmount(line.amount) }} ({{
                formatBillingAmount(line.unit_rate)
              }}
              / M)</span
            >
          </div>
        </section>
      </div>
    </template>
  </UModal>
</template>
