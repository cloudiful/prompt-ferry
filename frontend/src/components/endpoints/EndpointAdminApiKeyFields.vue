<script setup lang="ts">
import { computed } from 'vue'
import type { EndpointForm } from '@/models'

const props = defineProps<{
  t: TranslateFn
}>()

const form = defineModel<EndpointForm>('form', { required: true })

const saved = computed(() => form.value?.has_admin_api_key ?? false)
const clearing = computed(() => form.value?.admin_api_key_clear ?? false)

const placeholder = computed(() => {
  if (clearing.value) return props.t('endpointAdminApiKeyClearedOnSave')
  return saved.value
    ? props.t('endpointAdminApiKeyOptionalOnEdit')
    : props.t('endpointAdminApiKey')
})

function onInput(value: string): void {
  if (!form.value) return
  form.value.admin_api_key = value
  // Typing a replacement cancels a pending clear.
  if (value.trim() !== '') form.value.admin_api_key_clear = false
}

function requestClear(): void {
  if (!form.value) return
  form.value.admin_api_key = ''
  form.value.admin_api_key_clear = true
}

function undoClear(): void {
  if (!form.value) return
  form.value.admin_api_key_clear = false
}
</script>

<template>
  <div class="grid gap-2">
    <div class="flex items-center gap-1">
      <label
        class="text-xs font-medium text-default"
        for="endpoint-admin-api-key"
      >
        {{ t('endpointAdminApiKey') }}
      </label>
      <UTooltip :text="t('endpointAdminApiKeyHint')">
        <UButton
          type="button"
          size="xs"
          color="neutral"
          variant="ghost"
          icon="i-lucide-info"
          :aria-label="t('endpointAdminApiKeyHint')"
        />
      </UTooltip>
      <UBadge v-if="saved && !clearing" :label="t('saved')" color="neutral" />
      <UBadge
        v-if="clearing"
        :label="t('endpointAdminApiKeyPendingClear')"
        color="warning"
        variant="subtle"
      />
    </div>
    <div class="flex items-center gap-2">
      <UInput
        id="endpoint-admin-api-key"
        class="min-w-0 flex-1"
        type="password"
        autocomplete="off"
        :model-value="form.admin_api_key"
        :disabled="clearing"
        :placeholder="placeholder"
        @update:model-value="onInput"
      />
      <UButton
        v-if="saved && !clearing"
        type="button"
        size="sm"
        color="neutral"
        variant="ghost"
        @click="requestClear"
        >{{ t('endpointAdminApiKeyClear') }}</UButton
      >
      <UButton
        v-if="clearing"
        type="button"
        size="sm"
        color="neutral"
        variant="ghost"
        @click="undoClear"
        >{{ t('cancel') }}</UButton
      >
    </div>
    <p v-if="saved" class="text-xs leading-snug text-muted">
      {{ t('endpointAdminApiKeyKeepHint') }}
    </p>
  </div>
</template>
