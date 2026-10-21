<script setup lang="ts">
import type { ModelRouteTestTargetResult } from '@/generated/admin-api'

const props = defineProps<{
  results: ModelRouteTestTargetResult[]
  t: TranslateFn
}>()

const visible = defineModel<boolean>('visible', { required: true })
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="t('modelRouteTestDetails')"
    :ui="{ content: 'sm:max-w-2xl' }"
  >
    <template #body>
      <ul class="grid gap-2 text-xs">
        <li
          v-for="result in props.results"
          :key="result.target_id"
          class="grid gap-1 rounded border border-default p-2"
        >
          <div class="flex flex-wrap items-center gap-2">
            <UIcon
              :name="result.ok ? 'i-lucide-check' : 'i-lucide-x'"
              :class="result.ok ? 'h-4 w-4 text-success' : 'h-4 w-4 text-error'"
            />
            <span class="font-medium text-highlighted">
              {{ result.endpoint_name }}
            </span>
            <span
              v-if="result.preferred"
              class="rounded-full border border-default px-1.5 py-px text-[0.68rem] text-muted"
            >
              {{ t('modelRouteTestPreferred') }}
            </span>
          </div>
          <div class="break-words text-muted">
            {{ result.model ?? '-' }}
          </div>
          <div class="text-muted">
            HTTP {{ result.status ?? '-' }} / {{ result.duration_ms }}ms
          </div>
          <div class="break-words whitespace-pre-wrap text-default">
            {{ result.message }}
          </div>
        </li>
      </ul>
    </template>
    <template #footer>
      <div class="flex justify-end">
        <UButton
          type="button"
          size="sm"
          color="neutral"
          @click="visible = false"
          >{{ t('modelRouteTestClose') }}</UButton
        >
      </div>
    </template>
  </UModal>
</template>
