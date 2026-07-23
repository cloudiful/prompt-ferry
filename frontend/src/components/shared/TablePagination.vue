<script setup lang="ts">
import { computed } from 'vue'
import { pageChange } from '@/table-pagination'

const props = defineProps<{
  first: number
  pageSizeOptions: readonly number[]
  rows: number
  total: number
}>()

const emit = defineEmits<{
  change: [event: TablePageChange]
}>()

const page = computed(() => Math.floor(props.first / props.rows) + 1)
const pageSizeItems = computed(() => [...props.pageSizeOptions])

function changePage(nextPage: number): void {
  emit('change', pageChange(nextPage, props.rows))
}

function changeRows(value: unknown): void {
  const rows = Number(value)
  if (Number.isFinite(rows)) emit('change', pageChange(1, rows))
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
      @update:model-value="changeRows"
    />
    <UPagination
      :page="page"
      :items-per-page="rows"
      :total="total"
      @update:page="changePage"
    />
  </div>
</template>
