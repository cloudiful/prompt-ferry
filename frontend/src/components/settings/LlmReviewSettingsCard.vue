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
    <div class="grid gap-3 sm:grid-cols-2">
      <label class="grid gap-1">
        <span class="text-xs font-medium text-muted">{{
          t('reviewBaseUrl')
        }}</span>
        <UInput
          v-model="llmReview.review_base_url"
          size="sm"
          :placeholder="t('reviewBaseUrlPlaceholder')"
        />
      </label>
      <label class="grid gap-1">
        <span class="text-xs font-medium text-muted">{{
          t('reviewModel')
        }}</span>
        <UInput
          v-model="llmReview.review_model"
          size="sm"
          :placeholder="t('reviewModelPlaceholder')"
        />
      </label>
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
        <div class="grid gap-3 sm:grid-cols-2">
          <label class="grid gap-1">
            <span class="text-xs font-medium text-muted">{{
              t('apiKey')
            }}</span>
            <UInput
              v-model="llmReview.review_api_key"
              size="sm"
              type="password"
            />
          </label>
          <label class="grid gap-1">
            <span class="text-xs font-medium text-muted">{{
              t('failurePolicy')
            }}</span>
            <USelect
              v-model="llmReview.failure_policy"
              size="sm"
              :items="translatedFailurePolicyOptions"
              label-key="label"
              value-key="value"
            />
          </label>
          <label class="grid gap-1">
            <span class="text-xs font-medium text-muted">{{
              t('reviewTimeoutMs')
            }}</span>
            <UInputNumber
              v-model="llmReview.review_timeout_ms"
              size="sm"
              :min="100"
            />
          </label>
          <label class="grid gap-1">
            <span class="text-xs font-medium text-muted">{{
              t('pendingTtlSeconds')
            }}</span>
            <UInputNumber
              v-model="llmReview.pending_ttl_seconds"
              size="sm"
              :min="1"
            />
          </label>
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
        <div class="grid gap-1">
          <span class="text-xs font-medium text-muted">{{
            t('customPolicyText')
          }}</span>
          <UTextarea
            v-model="llmReview.custom_policy_text"
            autoresize
            :rows="3"
            size="sm"
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
            <div class="flex items-center gap-1">
              <label class="text-xs text-muted">{{ t('extraHeaders') }}</label>
              <UTooltip :text="t('extraHeadersHint')">
                <UButton
                  type="button"
                  size="xs"
                  color="neutral"
                  variant="ghost"
                  icon="i-lucide-info"
                  :aria-label="t('extraHeadersHint')"
                />
              </UTooltip>
            </div>
            <UTextarea
              v-model="webhookHeadersText"
              autoresize
              :rows="3"
              class="font-mono"
              :placeholder="t('extraHeadersPlaceholder')"
            />
          </div>
        </div>
      </template>
    </UCollapsible>
  </div>
</template>
