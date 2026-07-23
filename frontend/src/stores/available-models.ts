import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { meListModels } from '../generated/admin-api'
import type { AvailableModel } from '../generated/admin-api'
import { expectData, withData } from '../api'

export const useAvailableModelsStore = defineStore('available-models', () => {
  const models = ref<AvailableModel[]>([])
  const loading = ref(false)

  const total = computed(() => models.value.length)

  async function refresh(): Promise<void> {
    loading.value = true
    try {
      const response = expectData(await meListModels<true>(withData()))
      models.value = response.models
    } finally {
      loading.value = false
    }
  }

  return {
    loading,
    models,
    refresh,
    total,
  }
})
