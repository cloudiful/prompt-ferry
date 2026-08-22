<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed, ref } from 'vue'
import type { McpEnvironmentVariableForm } from '@/models'

const props = defineProps<{
  t: TranslateFn
}>()

const variables = defineModel<McpEnvironmentVariableForm[]>('variables', {
  required: true,
})

const visibleValues = ref<Record<number, boolean>>({})

type EnvironmentRow = { key: string; index: number }

const rows = computed<EnvironmentRow[]>(() =>
  variables.value.map((_, index) => ({ key: `env-${index}`, index })),
)

const columns = computed<TableColumn<EnvironmentRow>[]>(() => [
  { id: 'name', header: props.t('environmentVariableName') },
  { id: 'source', header: props.t('environmentVariableSource') },
  { id: 'value', header: props.t('environmentVariableValue') },
  { id: 'actions' },
])

function addVariable(): void {
  variables.value.push({
    name: '',
    source: 'worker',
    value: '',
    has_saved_value: false,
  })
}

function removeVariable(index: number): void {
  variables.value.splice(index, 1)
  const nextVisibleValues: Record<number, boolean> = {}
  for (const [rawIndex, visible] of Object.entries(visibleValues.value)) {
    const currentIndex = Number(rawIndex)
    if (currentIndex === index) continue
    nextVisibleValues[currentIndex > index ? currentIndex - 1 : currentIndex] =
      visible
  }
  visibleValues.value = nextVisibleValues
}

function isValueVisible(index: number): boolean {
  return visibleValues.value[index] === true
}

function toggleValueVisibility(index: number): void {
  visibleValues.value[index] = !isValueVisible(index)
}

function setSource(index: number, source: 'worker' | 'value'): void {
  const variable = variables.value[index]
  if (!variable || variable.source === source) return
  variable.source = source
  variable.value = ''
  variable.has_saved_value = false
}
</script>

<template>
  <div class="grid gap-2">
    <div class="flex flex-wrap items-start justify-between gap-3">
      <div class="flex items-center gap-1">
        <label class="text-xs font-medium text-default">
          {{ t('stdioEnv') }}
        </label>
        <UTooltip :text="t('stdioEnvHint')">
          <UButton
            type="button"
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-info"
            :aria-label="t('stdioEnvHint')"
          />
        </UTooltip>
      </div>
      <UButton type="button" size="sm" color="neutral" @click="addVariable">
        {{ t('addEnvironmentVariable') }}
      </UButton>
    </div>
    <UTable :data="rows" :columns="columns" class="min-w-0">
      <template #name-cell="{ row }">
        <UInput
          v-model="variables[row.original.index].name"
          class="w-full"
          :placeholder="t('environmentVariableNamePlaceholder')"
        />
      </template>
      <template #source-cell="{ row }">
        <USelect
          :model-value="variables[row.original.index].source"
          class="w-full min-w-40"
          :items="[
            { label: t('environmentSourceWorker'), value: 'worker' },
            { label: t('environmentSourceValue'), value: 'value' },
          ]"
          label-key="label"
          value-key="value"
          @update:model-value="setSource(row.original.index, $event)"
        />
      </template>
      <template #value-cell="{ row }">
        <div class="flex min-w-0 items-center gap-1">
          <UInput
            v-model="variables[row.original.index].value"
            class="min-w-0 flex-1"
            :type="
              variables[row.original.index].source === 'value' &&
              !isValueVisible(row.original.index)
                ? 'password'
                : 'text'
            "
            :placeholder="
              variables[row.original.index].source === 'worker'
                ? t('environmentWorkerNamePlaceholder')
                : variables[row.original.index].has_saved_value
                  ? t('savedSecret')
                  : t('environmentValuePlaceholder')
            "
          />
          <UButton
            v-if="variables[row.original.index].source === 'value'"
            type="button"
            size="sm"
            color="neutral"
            variant="ghost"
            :icon="
              isValueVisible(row.original.index)
                ? 'i-lucide-eye-off'
                : 'i-lucide-eye'
            "
            :aria-label="
              isValueVisible(row.original.index)
                ? t('hideSecret')
                : t('showSecret')
            "
            @click="toggleValueVisibility(row.original.index)"
          />
        </div>
      </template>
      <template #actions-cell="{ row }">
        <div class="flex justify-end">
          <UButton
            type="button"
            size="sm"
            color="error"
            variant="ghost"
            :aria-label="t('delete')"
            @click="removeVariable(row.original.index)"
          >
            <UIcon name="i-lucide-trash-2" class="h-4 w-4" />
          </UButton>
        </div>
      </template>
    </UTable>
  </div>
</template>
