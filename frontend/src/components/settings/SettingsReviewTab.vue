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
  <section class="grid gap-4">
    <SettingsCard body-class="p-0">
      <template #header>
        <div>
          <h3
            class="m-0 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
          >
            {{ t('llmReview') }}
          </h3>
        </div>
        <div class="flex flex-wrap items-center justify-end gap-2">
          <USwitch
            v-model="llmReview.enabled"
            id="settings-llm-review-enabled"
            :aria-label="t('llmReview')"
          />
          <UButton size="sm" :loading="busy" @click="$emit('saveLlmReview')">{{
            t('save')
          }}</UButton>
        </div>
      </template>
      <LlmReviewSettingsCard
        v-model:llm-review="llmReview"
        v-model:webhook-headers-text="llmReviewWebhookHeadersText"
        :busy="busy"
        :t="t"
      />
    </SettingsCard>
  </section>
</template>
