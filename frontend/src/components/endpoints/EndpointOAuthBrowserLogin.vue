<script setup lang="ts">
import type { OAuthBrowserStartResponse } from '@/generated/admin-api'

withDefaults(
  defineProps<{
    actionsEnabled: boolean
    browser: OAuthBrowserStartResponse | null
    busy: boolean
    message: string
    redirectUrl: string
    t: TranslateFn
  }>(),
  { actionsEnabled: false },
)

defineEmits<{
  'update:redirectUrl': [value: string]
  start: []
  complete: []
  copy: [value: string]
}>()
</script>

<template>
  <div class="grid gap-2 border-t border-default pt-3">
    <div class="flex items-center justify-between gap-2">
      <span class="text-xs font-medium text-default">
        {{ t('endpointOAuthBrowserLogin') }}
      </span>
      <UButton
        type="button"
        size="xs"
        color="primary"
        variant="soft"
        :loading="busy && !browser"
        :disabled="!actionsEnabled || busy"
        @click="$emit('start')"
        >{{ t('endpointOAuthBrowserStart') }}</UButton
      >
    </div>
    <p class="text-xs text-dimmed">{{ t('endpointOAuthBrowserHint') }}</p>
    <template v-if="browser">
      <div class="grid gap-1">
        <label class="text-xs text-muted" for="endpoint-oauth-authorize">
          {{ t('endpointOAuthAuthorizeUrl') }}
        </label>
        <div class="flex items-center gap-2">
          <UInput
            id="endpoint-oauth-authorize"
            class="w-full"
            :model-value="browser.authorize_url"
            readonly
          />
          <UButton
            type="button"
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-copy"
            :aria-label="t('endpointOAuthCopy')"
            @click="$emit('copy', browser.authorize_url)"
          />
        </div>
      </div>
      <div class="grid gap-1">
        <label class="text-xs text-muted" for="endpoint-oauth-redirect">
          {{ t('endpointOAuthRedirectUrl') }}
        </label>
        <UInput
          id="endpoint-oauth-redirect"
          class="w-full"
          :model-value="redirectUrl"
          :disabled="busy || !actionsEnabled"
          :placeholder="t('endpointOAuthRedirectPlaceholder')"
          @update:model-value="
            $emit('update:redirectUrl', String($event ?? ''))
          "
        />
      </div>
      <div class="flex justify-end">
        <UButton
          type="button"
          size="xs"
          color="primary"
          :loading="busy && Boolean(redirectUrl.trim())"
          :disabled="!actionsEnabled || busy || redirectUrl.trim() === ''"
          @click="$emit('complete')"
          >{{ t('endpointOAuthComplete') }}</UButton
        >
      </div>
    </template>
    <p v-if="message" class="break-words text-error">{{ message }}</p>
  </div>
</template>
