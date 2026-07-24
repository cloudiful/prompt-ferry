import { defineStore } from 'pinia'
import { ref } from 'vue'
import { meListModels } from '../generated/admin-api'
import type { AvailableModel } from '../generated/admin-api'
import { expectData, withData } from '../api'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'

export const useAvailableModelsStore = defineStore('available-models', () => {
  const models = ref<AvailableModel[]>([])
  const loading = ref(false)
  const first = ref(0)
  const rows = useStoredPageSize(
    'available-models',
    10,
    STANDARD_PAGE_SIZE_OPTIONS,
  )
  const total = ref(0)

  async function refresh(
    nextFirst = first.value,
    nextRows = rows.value,
  ): Promise<void> {
    first.value = nextFirst
    rows.value = nextRows
    loading.value = true
    try {
      const response = expectData(
        await meListModels<true>(
          withData({ query: { first: first.value, rows: rows.value } }),
        ),
      )
      models.value = response.models
      total.value = response.total
      first.value = response.first
      rows.value = response.rows
      if (
        models.value.length === 0 &&
        total.value > 0 &&
        first.value >= total.value
      ) {
        const previousFirst = Math.floor((total.value - 1) / rows.value) * rows.value
        await refresh(previousFirst, rows.value)
      }
    } finally {
      loading.value = false
    }
  }

  return {
    loading,
    first,
    models,
    refresh,
    rows,
    total,
  }
})
