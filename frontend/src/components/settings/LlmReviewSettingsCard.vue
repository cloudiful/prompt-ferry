<script setup lang="ts">
import { computed } from 'vue'
import type { LlmReviewSettings } from '@/generated/admin-api'

const props = defineProps<{
  busy: boolean
  t: TranslateFn
}>()

const llmReview = defineModel<LlmReviewSettings>('llmReview', {
  required: true,
})
const webhookHeadersText = defineModel<string>('webhookHeadersText', {
  required: true,
})

const failurePolicyOptions = [
  { label: 'failurePolicyFailClosed', value: 'fail_closed' },
  { label: 'failurePolicyFailOpen', value: 'fail_open' },
]

const translatedFailurePolicyOptions = computed(() =>
  failurePolicyOptions.map((item) => ({
    label: props.t(item.label),
    value: item.value,
  })),
)
</script>

<template>
  <div class="grid gap-3 p-3">
    <div class="grid grid-cols-2 gap-3 max-[767px]:grid-cols-1">
      <div class="grid gap-1.5">
        <label class="text-xs text-muted">{{ t('reviewBaseUrl') }}</label>
        <UInput
          v-model="llmReview.review_base_url"
          size="sm"
          :placeholder="t('reviewBaseUrlPlaceholder')"
        />
      </div>
      <div class="grid gap-1.5">
        <label class="text-xs text-muted">{{ t('reviewModel') }}</label>
        <UInput
          v-model="llmReview.review_model"
          size="sm"
          :placeholder="t('reviewModelPlaceholder')"
        />
      </div>
    </div>

    <UCollapsible>
      <template #default="{ open }">
        <UButton
          color="neutral"
          variant="subtle"
          :label="t('reviewExecution')"
          :trailing-icon="
            open ? 'i-lucide-chevron-up' : 'i-lucide-chevron-down'
          "
          block
        />
      </template>
      <template #content>
        <div class="grid gap-3 md:grid-cols-2">
          <div class="grid gap-2">
            <label class="text-xs text-muted">{{ t('apiKey') }}</label>
            <UInput
              v-model="llmReview.review_api_key"
              size="sm"
              type="password"
            />
          </div>
          <div class="grid gap-2">
            <label class="text-xs text-muted">{{ t('failurePolicy') }}</label>
            <USelect
              v-model="llmReview.failure_policy"
              size="sm"
              :items="translatedFailurePolicyOptions"
              label-key="label"
              value-key="value"
            />
          </div>
          <div class="grid gap-2">
            <label class="text-xs text-muted">{{ t('reviewTimeoutMs') }}</label>
            <UInputNumber
              v-model="llmReview.review_timeout_ms"
              size="sm"
              :min="100"
            />
          </div>
          <div class="grid gap-2">
            <label class="text-xs text-muted">{{
              t('pendingTtlSeconds')
            }}</label>
            <UInputNumber
              v-model="llmReview.pending_ttl_seconds"
              size="sm"
              :min="1"
            />
          </div>
        </div>
      </template>
    </UCollapsible>

    <UCollapsible>
      <template #default="{ open }">
        <UButton
          color="neutral"
          variant="subtle"
          :label="t('customPolicyText')"
          :trailing-icon="
            open ? 'i-lucide-chevron-up' : 'i-lucide-chevron-down'
          "
          block
        />
      </template>
      <template #content>
        <div class="grid gap-2">
          <UTextarea
            v-model="llmReview.custom_policy_text"
            autoresize
            :rows="3"
          />
        </div>
      </template>
    </UCollapsible>

    <UCollapsible v-if="llmReview.webhook">
      <template #default="{ open }">
        <div class="flex items-center gap-2">
          <UButton
            color="neutral"
            variant="subtle"
            :label="t('webhook')"
            :trailing-icon="
              open ? 'i-lucide-chevron-up' : 'i-lucide-chevron-down'
            "
            class="flex-1"
          />
          <USwitch v-model="llmReview.webhook.enabled" />
        </div>
      </template>
      <template #content>
        <div class="grid gap-2.5">
          <div class="grid gap-3 md:grid-cols-2">
            <div class="grid gap-2">
              <label class="text-xs text-muted">{{ t('webhookUrl') }}</label>
              <UInput v-model="llmReview.webhook.url" size="sm" />
            </div>
            <div class="grid gap-2">
              <label class="text-xs text-muted">{{
                t('webhookBearerToken')
              }}</label>
              <UInput
                v-model="llmReview.webhook.bearer_token"
                size="sm"
                type="password"
              />
            </div>
          </div>
          <div class="grid gap-2">
            <label class="text-xs text-muted">{{ t('extraHeaders') }}</label>
            <UTextarea
              v-model="webhookHeadersText"
              autoresize
              :rows="3"
              class="font-mono"
              :placeholder="t('extraHeadersPlaceholder')"
            />
            <div class="text-xs text-dimmed">
              {{ t('extraHeadersHint') }}
            </div>
          </div>
        </div>
      </template>
    </UCollapsible>
  </div>
</template>
