<script setup lang="ts">
import type { LlmReviewSettings } from '@/generated/admin-api'
import LlmReviewSettingsCard from './LlmReviewSettingsCard.vue'
import SettingsCard from './SettingsCard.vue'

defineProps<{
  busy: boolean
  t: TranslateFn
}>()

const llmReview = defineModel<LlmReviewSettings>('llmReview', {
  required: true,
})
const llmReviewWebhookHeadersText = defineModel<string>(
  'llmReviewWebhookHeadersText',
  { required: true },
)

defineEmits<{
  saveLlmReview: []
}>()
</script>

<template>
  <section class="grid gap-3">
    <SettingsCard body-class="p-0">
      <template #header>
        <h3
          class="m-0 inline-flex items-center gap-1.5 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
        >
          <UIcon name="i-lucide-sparkles" class="h-3.5 w-3.5 text-muted" />
          {{ t('llmReview') }}
        </h3>
        <div class="flex flex-wrap items-center gap-2">
          <USwitch
            v-model="llmReview.enabled"
            id="settings-llm-review-enabled"
            :aria-label="t('llmReview')"
          />
          <UButton
            size="sm"
            icon="i-lucide-save"
            :loading="busy"
            @click="$emit('saveLlmReview')"
            >{{ t('save') }}</UButton
          >
        </div>
      </template>
      <LlmReviewSettingsCard
        v-model:llm-review="llmReview"
        v-model:webhook-headers-text="llmReviewWebhookHeadersText"
        :busy="busy"
        :t="t"
      />
    </SettingsCard>
    <p class="m-0 px-1 text-[11px] leading-relaxed text-muted">
      {{ t('llmReviewHint') }}
    </p>
  </section>
</template>
