<script setup lang="ts">
import type { MessageKey } from '@/i18n'

type RequestLimitFormLike = {
  daily_max_requests: number | null
  monthly_max_requests: number | null
}

defineProps<{
  dailyLabel: MessageKey
  monthlyLabel: MessageKey
  t: TranslateFn
}>()

const form = defineModel<RequestLimitFormLike>('form', { required: true })
</script>

<template>
  <div class="grid gap-3 md:grid-cols-2">
    <div class="flex min-w-0 items-center gap-3">
      <div class="shrink-0 text-xs text-muted">{{ t(dailyLabel) }}</div>
      <UInputNumber
        v-model="form.daily_max_requests"
        class="min-w-0 flex-1"
        size="sm"
        :min="1"
        :use-grouping="false"
      />
    </div>
    <div class="flex min-w-0 items-center gap-3">
      <div class="shrink-0 text-xs text-muted">
        {{ t(monthlyLabel) }}
      </div>
      <UInputNumber
        v-model="form.monthly_max_requests"
        class="min-w-0 flex-1"
        size="sm"
        :min="1"
        :use-grouping="false"
      />
    </div>
  </div>
</template>
