<script setup lang="ts">
import { computed } from 'vue'
import { pageChange } from '@/table-pagination'
import { useLocale } from '@/composables/useLocale'

const props = defineProps<{
  first: number
  pageSizeOptions: readonly number[]
  rows: number
  total: number
}>()

const emit = defineEmits<{
  change: [event: TablePageChange]
}>()

const { t } = useLocale()
const page = computed(() => Math.floor(props.first / props.rows) + 1)
const pageSizeItems = computed(() => [...props.pageSizeOptions])

function changePage(nextPage: number): void {
  emit('change', pageChange(nextPage, props.rows))
}

function changeRows(value: unknown): void {
  const rows = Number(value)
  if (Number.isFinite(rows)) emit('change', pageChange(1, rows))
}

function pageLabel(value: number, current: boolean): string {
  return current
    ? t('paginationPageCurrent', { page: value })
    : t('paginationPage', { page: value })
}
</script>

<template>
  <div
    class="flex flex-wrap items-center justify-between gap-3 border-t border-default px-3 py-3"
  >
    <USelect
      :model-value="rows"
      :items="pageSizeItems"
      class="w-24"
      :aria-label="t('paginationPageSize')"
      @update:model-value="changeRows"
    />
    <UPagination
      :page="page"
      :items-per-page="rows"
      :total="total"
      @update:page="changePage"
    >
      <template #first>
        <UButton
          color="neutral"
          variant="outline"
          icon="i-lucide-chevrons-left"
          :aria-label="t('paginationFirst')"
        />
      </template>
      <template #prev>
        <UButton
          color="neutral"
          variant="outline"
          icon="i-lucide-chevron-left"
          :aria-label="t('paginationPrevious')"
        />
      </template>
      <template #next>
        <UButton
          color="neutral"
          variant="outline"
          icon="i-lucide-chevron-right"
          :aria-label="t('paginationNext')"
        />
      </template>
      <template #last>
        <UButton
          color="neutral"
          variant="outline"
          icon="i-lucide-chevrons-right"
          :aria-label="t('paginationLast')"
        />
      </template>
      <template #item="{ item, page: currentPage }">
        <UButton
          :color="currentPage === item.value ? 'primary' : 'neutral'"
          :variant="currentPage === item.value ? 'solid' : 'outline'"
          :label="String(item.value)"
          :aria-label="pageLabel(item.value, currentPage === item.value)"
          square
        />
      </template>
    </UPagination>
  </div>
</template>
