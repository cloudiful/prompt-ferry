<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed, onMounted } from 'vue'
import PageIntro from '@/components/PageIntro.vue'
import { useLocale } from '@/composables/useLocale'
import { useNotifier } from '@/composables/useNotifier'
import type { AvailableModel } from '@/generated/admin-api'
import { useAvailableModelsStore } from '@/stores/available-models'

const { t } = useLocale()
const { notifyApiError } = useNotifier()
const availableModelsStore = useAvailableModelsStore()

const busy = computed(() => availableModelsStore.loading)
const models = computed(() => availableModelsStore.models)
const columns = computed<TableColumn<AvailableModel>[]>(() => [
  { accessorKey: 'id', header: t('id') },
  { accessorKey: 'name', header: t('name') },
])

async function refresh(): Promise<void> {
  try {
    await availableModelsStore.refresh()
  } catch (cause) {
    notifyApiError(cause)
  }
}

onMounted(async () => {
  await refresh()
})
</script>

<template>
  <div class="grid min-w-0 max-w-full gap-3">
    <PageIntro :eyebrow="t('routing')" :title="t('availableModels')">
      <template #actions>
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          :loading="busy"
          :aria-label="t('refresh')"
          @click="refresh"
        >
          <span>{{ t('refresh') }}</span>
        </UButton>
      </template>
    </PageIntro>
    <UTable :data="models" :columns="columns" :loading="busy" class="min-w-0" />
  </div>
</template>
