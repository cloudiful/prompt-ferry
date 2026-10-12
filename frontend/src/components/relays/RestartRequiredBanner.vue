<script setup lang="ts">
import { useLocale } from '@/composables/useLocale'

defineProps<{
  busy: boolean
  pending: boolean
}>()

defineEmits<{ restart: [] }>()

const { t } = useLocale()
</script>

<template>
  <UAlert
    color="warning"
    variant="subtle"
    icon="i-lucide-triangle-alert"
    :title="t('relayRestartRequired')"
    :description="t('relayRestartRequiredDesc')"
  >
    <template #actions>
      <UButton
        size="sm"
        color="warning"
        :loading="busy"
        :disabled="pending"
        @click="$emit('restart')"
      >
        {{ t('relayRestartNow') }}
      </UButton>
    </template>
  </UAlert>
</template>
