<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import type { BillingPriceRuleResponse, ProviderEndpoint } from '@/generated/admin-api'
import { formatBillingRate, formatBillingTime } from '@/models/billing'

const props = defineProps<{
  endpoints: ProviderEndpoint[]
  loading: boolean
  rules: BillingPriceRuleResponse[]
  t: TranslateFn
}>()

const emit = defineEmits<{
  create: []
  toggle: [rule: BillingPriceRuleResponse]
}>()

const columns = computed<TableColumn<BillingPriceRuleResponse>[]>(() => [
  { accessorKey: 'price_side', header: props.t('billingStatus') },
  { id: 'scope', header: props.t('publicModel') },
  { accessorKey: 'input_rate', header: props.t('inputRate') },
  { accessorKey: 'cache_read_rate', header: props.t('cacheReadRate') },
  { accessorKey: 'cache_write_rate', header: props.t('cacheWriteRate') },
  { accessorKey: 'output_rate', header: props.t('outputRate') },
  { accessorKey: 'effective_from', header: props.t('effectiveFrom') },
  { accessorKey: 'enabled', header: props.t('status') },
  { id: 'actions' },
])

function scope(rule: BillingPriceRuleResponse): string {
  if (rule.price_side === 'sale') return rule.public_model || '-'
  const endpoint = props.endpoints.find((item) => item.endpoint_id === rule.endpoint_id)
  return `${endpoint?.name || rule.endpoint_id || '-'} / ${rule.upstream_model || '-'}`
}
</script>

<template>
  <section class="grid gap-3 rounded-lg bg-default p-3">
    <div class="flex flex-wrap items-center gap-2">
      <h2 class="mr-auto text-sm font-semibold text-highlighted">{{ t('billingPriceRules') }}</h2>
      <UButton size="sm" icon="i-lucide-plus" @click="emit('create')">{{ t('newPriceRule') }}</UButton>
    </div>
    <div class="min-w-0 overflow-x-auto">
      <UTable :data="rules" :columns="columns" :loading="loading" class="min-w-[64rem]">
        <template #empty>{{ t('noPriceRules') }}</template>
        <template #price_side-cell="{ row }"><UBadge :label="row.original.price_side === 'sale' ? t('salePrice') : t('costPrice')" variant="subtle" /></template>
        <template #scope-cell="{ row }">{{ scope(row.original) }}</template>
        <template #input_rate-cell="{ row }">{{ formatBillingRate(row.original.input_rate, row.original.currency) }}</template>
        <template #cache_read_rate-cell="{ row }">{{ formatBillingRate(row.original.cache_read_rate, row.original.currency) }}</template>
        <template #cache_write_rate-cell="{ row }">{{ formatBillingRate(row.original.cache_write_rate, row.original.currency) }}</template>
        <template #output_rate-cell="{ row }">{{ formatBillingRate(row.original.output_rate, row.original.currency) }}</template>
        <template #effective_from-cell="{ row }">{{ formatBillingTime(row.original.effective_from) }}</template>
        <template #enabled-cell="{ row }"><UBadge :label="row.original.enabled ? t('active') : t('disabled')" :color="row.original.enabled ? 'success' : 'neutral'" variant="subtle" /></template>
        <template #actions-cell="{ row }">
          <UButton size="sm" color="neutral" variant="ghost" :icon="row.original.enabled ? 'i-lucide-circle-pause' : 'i-lucide-circle-play'" :aria-label="row.original.enabled ? t('disablePriceRule') : t('enablePriceRule')" @click="emit('toggle', row.original)" />
        </template>
      </UTable>
    </div>
    <p class="text-xs text-dimmed">{{ t('billingRuleScopeHint') }}</p>
  </section>
</template>
