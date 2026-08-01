<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import type { BillingChargeResponse } from '@/generated/admin-api'
import {
  formatBillingAmount,
  formatBillingTime,
  formatTokenCount,
} from '@/models/billing'
import { BILLING_PAGE_SIZE_OPTIONS } from '@/models/billing'
import TablePagination from '@/components/shared/TablePagination.vue'

const props = defineProps<{
  charges: BillingChargeResponse[]
  first: number
  isAdmin: boolean
  loading: boolean
  rows: number
  total: number
  t: TranslateFn
}>()

const emit = defineEmits<{
  openDetail: [charge: BillingChargeResponse]
  page: [event: TablePageChange]
}>()

const columns = computed<TableColumn<BillingChargeResponse>[]>(() => [
  { id: 'details' },
  ...(props.isAdmin
    ? [{ accessorKey: 'user_login_name', header: props.t('user') }]
    : []),
  { accessorKey: 'client_key_label', header: props.t('clientKey') },
  { accessorKey: 'requested_model', header: props.t('publicModel') },
  ...(props.isAdmin
    ? [{ accessorKey: 'endpoint_name', header: props.t('endpoint') }]
    : []),
  { id: 'tokens', header: props.t('usageTokens') },
  { accessorKey: 'pricing_status', header: props.t('billingStatus') },
  { accessorKey: 'customer_amount', header: props.t('chargeAmount') },
  { accessorKey: 'created_at', header: props.t('createdAt') },
])

function statusLabel(charge: BillingChargeResponse): string {
  if (charge.pricing_status === 'priced') return props.t('pricingPriced')
  if (charge.usage_status === 'unknown') return props.t('usageUnknown')
  return props.t('pricingUnpriced')
}

function statusColor(
  charge: BillingChargeResponse,
): 'success' | 'warning' | 'neutral' {
  if (charge.pricing_status === 'priced') return 'success'
  return charge.usage_status === 'unknown' ? 'neutral' : 'warning'
}
</script>

<template>
  <section class="grid gap-3 rounded-lg bg-default p-3">
    <h2 class="text-sm font-semibold text-highlighted">
      {{ t('billingCharges') }}
    </h2>
    <div class="min-w-0 overflow-x-auto">
      <UTable
        :data="charges"
        :columns="columns"
        :loading="loading"
        class="min-w-[60rem]"
      >
        <template #empty>{{ t('noBillingCharges') }}</template>
        <template #details-cell="{ row }">
          <UButton
            icon="i-lucide-receipt-text"
            color="neutral"
            variant="ghost"
            :aria-label="t('billingDetails')"
            @click="emit('openDetail', row.original)"
          />
        </template>
        <template #user_login_name-cell="{ row }">{{
          row.original.user_login_name || '-'
        }}</template>
        <template #client_key_label-cell="{ row }">{{
          row.original.client_key_label || '-'
        }}</template>
        <template #requested_model-cell="{ row }">{{
          row.original.requested_model || '-'
        }}</template>
        <template #endpoint_name-cell="{ row }">{{
          row.original.endpoint_name || '-'
        }}</template>
        <template #tokens-cell="{ row }">
          {{ formatTokenCount(row.original.input_tokens) }} /
          {{ formatTokenCount(row.original.cache_read_tokens) }} /
          {{ formatTokenCount(row.original.cache_write_tokens) }} /
          {{ formatTokenCount(row.original.output_tokens) }}
        </template>
        <template #pricing_status-cell="{ row }">
          <UBadge
            :label="statusLabel(row.original)"
            :color="statusColor(row.original)"
            variant="subtle"
          />
        </template>
        <template #customer_amount-cell="{ row }">{{
          formatBillingAmount(
            row.original.customer_amount,
            row.original.currency,
          )
        }}</template>
        <template #created_at-cell="{ row }">{{
          formatBillingTime(row.original.created_at)
        }}</template>
      </UTable>
    </div>
    <TablePagination
      :first="first"
      :rows="rows"
      :total="total"
      :page-size-options="BILLING_PAGE_SIZE_OPTIONS"
      @change="emit('page', $event)"
    />
  </section>
</template>
