<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import type { BillingPriceRuleResponse } from '@/generated/admin-api'
import { formatBillingRate, formatBillingTime } from '@/models/billing'
import TablePagination from '@/components/shared/TablePagination.vue'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/table-pagination'

const props = defineProps<{
  first: number
  loading: boolean
  rules: BillingPriceRuleResponse[]
  rows: number
  total: number
  t: TranslateFn
}>()

const emit = defineEmits<{
  create: []
  edit: [rule: BillingPriceRuleResponse]
  delete: [rule: BillingPriceRuleResponse]
  toggle: [rule: BillingPriceRuleResponse]
  page: [event: TablePageChange]
}>()

const columns = computed<TableColumn<BillingPriceRuleResponse>[]>(() => [
  { accessorKey: 'public_model', header: props.t('publicModel') },
  { accessorKey: 'input_rate', header: props.t('inputRate') },
  { accessorKey: 'cache_read_rate', header: props.t('cacheReadRate') },
  { accessorKey: 'cache_write_rate', header: props.t('cacheWriteRate') },
  { accessorKey: 'output_rate', header: props.t('outputRate') },
  { accessorKey: 'effective_from', header: props.t('effectiveFrom') },
  { accessorKey: 'enabled', header: props.t('status') },
  { id: 'actions' },
])
</script>

<template>
  <section class="grid gap-3 rounded-lg bg-default p-3">
    <div class="flex flex-wrap items-center gap-2">
      <div class="mr-auto flex items-center gap-1">
        <h2 class="text-sm font-semibold text-highlighted">
          {{ t('billingPriceRules') }}
        </h2>
        <UTooltip :text="t('billingRuleScopeHint')">
          <UButton
            type="button"
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-info"
            :aria-label="t('billingRuleScopeHint')"
          />
        </UTooltip>
      </div>
      <UButton size="sm" icon="i-lucide-plus" @click="emit('create')">{{
        t('newPriceRule')
      }}</UButton>
    </div>
    <div class="min-w-0 overflow-x-auto">
      <UTable
        :data="rules"
        :columns="columns"
        :loading="loading"
        class="min-w-[64rem]"
      >
        <template #empty>{{ t('noPriceRules') }}</template>
        <template #input_rate-cell="{ row }">{{
          formatBillingRate(row.original.input_rate, row.original.currency)
        }}</template>
        <template #cache_read_rate-cell="{ row }">{{
          formatBillingRate(row.original.cache_read_rate, row.original.currency)
        }}</template>
        <template #cache_write_rate-cell="{ row }">{{
          formatBillingRate(
            row.original.cache_write_rate,
            row.original.currency,
          )
        }}</template>
        <template #output_rate-cell="{ row }">{{
          formatBillingRate(row.original.output_rate, row.original.currency)
        }}</template>
        <template #effective_from-cell="{ row }">{{
          formatBillingTime(row.original.effective_from)
        }}</template>
        <template #enabled-cell="{ row }"
          ><UBadge
            :label="row.original.enabled ? t('active') : t('disabled')"
            :color="row.original.enabled ? 'success' : 'neutral'"
            variant="subtle"
        /></template>
        <template #actions-cell="{ row }">
          <div class="flex justify-end gap-1">
            <UButton
              size="sm"
              color="neutral"
              variant="ghost"
              icon="i-lucide-pencil"
              :aria-label="t('editPriceRule')"
              @click="emit('edit', row.original)"
            />
            <UButton
              size="sm"
              color="error"
              variant="ghost"
              icon="i-lucide-trash-2"
              :aria-label="t('deletePriceRule')"
              @click="emit('delete', row.original)"
            />
            <UButton
              size="sm"
              color="neutral"
              variant="ghost"
              :icon="
                row.original.enabled
                  ? 'i-lucide-circle-pause'
                  : 'i-lucide-circle-play'
              "
              :aria-label="
                row.original.enabled
                  ? t('disablePriceRule')
                  : t('enablePriceRule')
              "
              @click="emit('toggle', row.original)"
            />
          </div>
        </template>
      </UTable>
    </div>
    <TablePagination
      :first="props.first"
      :rows="props.rows"
      :total="props.total"
      :page-size-options="STANDARD_PAGE_SIZE_OPTIONS"
      @change="$emit('page', $event)"
    />
  </section>
</template>
