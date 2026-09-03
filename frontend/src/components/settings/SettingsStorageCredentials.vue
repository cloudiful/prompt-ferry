<script setup lang="ts">
defineProps<{
  t: TranslateFn
  hasAccessKey: boolean
  hasSecretKey: boolean
}>()

const accessKeyInput = defineModel<string>('accessKeyInput', { required: true })
const secretKeyInput = defineModel<string>('secretKeyInput', { required: true })
const clearAccessKey = defineModel<boolean>('clearAccessKey', {
  required: true,
})
const clearSecretKey = defineModel<boolean>('clearSecretKey', {
  required: true,
})
</script>

<template>
  <div class="grid gap-3 sm:grid-cols-2">
    <div class="grid gap-1">
      <span
        class="inline-flex items-center gap-1.5 text-xs font-medium text-muted"
      >
        {{ t('storageS3AccessKey') }}
        <UBadge
          :color="hasAccessKey ? 'success' : 'neutral'"
          variant="subtle"
          size="xs"
          :label="
            hasAccessKey ? t('storageSecretPresent') : t('storageSecretMissing')
          "
        />
      </span>
      <UInput
        v-model="accessKeyInput"
        size="sm"
        type="password"
        :placeholder="t('storageSecretPlaceholder')"
        :disabled="clearAccessKey"
      />
      <label class="inline-flex items-center gap-1.5 text-xs text-muted">
        <UCheckbox v-model="clearAccessKey" size="xs" />
        {{ t('storageClear') }}
      </label>
    </div>
    <div class="grid gap-1">
      <span
        class="inline-flex items-center gap-1.5 text-xs font-medium text-muted"
      >
        {{ t('storageS3SecretKey') }}
        <UBadge
          :color="hasSecretKey ? 'success' : 'neutral'"
          variant="subtle"
          size="xs"
          :label="
            hasSecretKey ? t('storageSecretPresent') : t('storageSecretMissing')
          "
        />
      </span>
      <UInput
        v-model="secretKeyInput"
        size="sm"
        type="password"
        :placeholder="t('storageSecretPlaceholder')"
        :disabled="clearSecretKey"
      />
      <label class="inline-flex items-center gap-1.5 text-xs text-muted">
        <UCheckbox v-model="clearSecretKey" size="xs" />
        {{ t('storageClear') }}
      </label>
    </div>
  </div>
</template>
