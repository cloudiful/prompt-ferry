<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import type { BillingBreakdownResponse } from '@/generated/admin-api'
import { formatBillingAmount, formatTokenCount } from '@/models/billing'

const props = defineProps<{
  rows: BillingBreakdownResponse[]
  isAdmin: boolean
  title: string
  t: TranslateFn
}>()

const columns = computed<TableColumn<BillingBreakdownResponse>[]>(() => [
  { accessorKey: 'grouping_key', header: props.t('model') },
  { accessorKey: 'request_count', header: props.t('billingRequests') },
  { accessorKey: 'input_tokens', header: props.t('inputRate') },
  { accessorKey: 'cache_read_tokens', header: props.t('cacheReadRate') },
  { accessorKey: 'cache_write_tokens', header: props.t('cacheWriteRate') },
  { accessorKey: 'output_tokens', header: props.t('outputRate') },
  { accessorKey: 'adjusted_amount', header: props.t('chargeAmount') },
  ...(props.isAdmin
    ? [{ accessorKey: 'provider_cost', header: props.t('chargeCost') }]
    : []),
])
</script>

<template>
  <section class="grid gap-3 rounded-lg bg-default p-3">
    <h2 class="text-sm font-semibold text-highlighted">{{ title }}</h2>
    <UTable :data="rows" :columns="columns" class="min-w-[52rem]">
      <template #empty>-</template>
      <template #request_count-cell="{ row }">{{ formatTokenCount(row.original.request_count) }}</template>
      <template #input_tokens-cell="{ row }">{{ formatTokenCount(row.original.input_tokens) }}</template>
      <template #cache_read_tokens-cell="{ row }">{{ formatTokenCount(row.original.cache_read_tokens) }}</template>
      <template #cache_write_tokens-cell="{ row }">{{ formatTokenCount(row.original.cache_write_tokens) }}</template>
      <template #output_tokens-cell="{ row }">{{ formatTokenCount(row.original.output_tokens) }}</template>
      <template #adjusted_amount-cell="{ row }">{{ formatBillingAmount(row.original.adjusted_amount) }}</template>
      <template #provider_cost-cell="{ row }">{{ formatBillingAmount(row.original.provider_cost) }}</template>
    </UTable>
  </section>
</template>
