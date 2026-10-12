<script setup lang="ts">
import type { OAuthDeviceStartResponse } from '@/generated/admin-api'

withDefaults(
  defineProps<{
    actionsEnabled: boolean
    device: OAuthDeviceStartResponse | null
    message: string
    polling: boolean
    starting: boolean
    t: TranslateFn
  }>(),
  { actionsEnabled: false },
)

defineEmits<{
  start: []
  copy: [value: string]
}>()
</script>

<template>
  <div class="grid gap-2">
    <div class="flex items-center justify-between gap-2">
      <span class="text-xs font-medium text-default">
        {{ t('endpointOAuthDeviceLogin') }}
      </span>
      <UButton
        type="button"
        size="xs"
        color="primary"
        variant="soft"
        :loading="starting"
        :disabled="!actionsEnabled"
        @click="$emit('start')"
        >{{ t('endpointOAuthDeviceStart') }}</UButton
      >
    </div>
    <p class="text-xs text-dimmed">{{ t('endpointOAuthDeviceHint') }}</p>
    <template v-if="device">
      <div class="flex items-center gap-2">
        <code class="rounded bg-muted px-2 py-1 font-mono text-sm">
          {{ device.user_code }}
        </code>
        <UButton
          type="button"
          size="xs"
          color="neutral"
          variant="ghost"
          icon="i-lucide-copy"
          :aria-label="t('endpointOAuthCopy')"
          @click="$emit('copy', device.user_code)"
        />
      </div>
      <p
        class="flex items-center gap-1 text-xs text-dimmed"
        :aria-busy="polling"
      >
        <UIcon
          v-if="polling"
          name="i-lucide-loader-circle"
          class="h-3 w-3 animate-spin"
        />
        <span>{{ t('endpointOAuthDeviceWaiting') }}</span>
        <a
          class="underline"
          :href="device.verification_uri"
          target="_blank"
          rel="noreferrer"
          >{{ device.verification_uri }}</a
        >
      </p>
    </template>
    <p v-if="message" class="break-words text-error">{{ message }}</p>
  </div>
</template>
