<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import type { BillingBreakdownResponse } from '@/generated/admin-api'
import { formatBillingAmount, formatTokenCount } from '@/models/billing'

const props = defineProps<{
  rows: BillingBreakdownResponse[]
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
  { accessorKey: 'customer_amount', header: props.t('chargeAmount') },
])
</script>

<template>
  <section class="grid gap-3 rounded-lg bg-default p-3">
    <h2 class="text-sm font-semibold text-highlighted">{{ title }}</h2>
    <div class="min-w-0 overflow-x-auto">
      <UTable :data="rows" :columns="columns" class="min-w-[52rem]">
        <template #empty>-</template>
        <template #request_count-cell="{ row }">{{
          formatTokenCount(row.original.request_count)
        }}</template>
        <template #input_tokens-cell="{ row }">{{
          formatTokenCount(row.original.input_tokens)
        }}</template>
        <template #cache_read_tokens-cell="{ row }">{{
          formatTokenCount(row.original.cache_read_tokens)
        }}</template>
        <template #cache_write_tokens-cell="{ row }">{{
          formatTokenCount(row.original.cache_write_tokens)
        }}</template>
        <template #output_tokens-cell="{ row }">{{
          formatTokenCount(row.original.output_tokens)
        }}</template>
        <template #customer_amount-cell="{ row }">{{
          formatBillingAmount(row.original.customer_amount)
        }}</template>
      </UTable>
    </div>
  </section>
</template>
