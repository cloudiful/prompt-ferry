<script setup lang="ts">
import { computed } from 'vue'
import type { ConfigImportPreview } from '@/generated/admin-api'

const props = defineProps<{
  open: boolean
  preview: ConfigImportPreview | null
  busy: boolean
  t: TranslateFn
}>()

const emit = defineEmits<{
  cancel: []
  confirm: []
}>()

const totals = computed(() => {
  return (props.preview?.domains ?? []).reduce(
    (acc, domain) => ({
      creates: acc.creates + domain.creates,
      deletes: acc.deletes + domain.deletes,
      unrecoverable: acc.unrecoverable + domain.unrecoverable_secrets,
      updates: acc.updates + domain.updates,
    }),
    { creates: 0, deletes: 0, unrecoverable: 0, updates: 0 },
  )
})

const hasWarnings = computed(() => (props.preview?.warnings.length ?? 0) > 0)

function onOpenChange(value: boolean): void {
  if (!value) emit('cancel')
}
</script>

<template>
  <UModal
    :open="open"
    :title="t('configArchiveImportConfirmTitle')"
    :ui="{ content: 'sm:max-w-md' }"
    @update:open="onOpenChange"
  >
    <template #body>
      <div class="grid gap-3">
        <p class="m-0 text-sm text-default">
          {{ t('configArchiveImportConfirmBody') }}
        </p>
        <div class="flex flex-wrap gap-1.5">
          <UBadge
            size="sm"
            color="success"
            variant="subtle"
            :label="`${t('configArchivePreviewCreates')} ${totals.creates}`"
          />
          <UBadge
            size="sm"
            color="info"
            variant="subtle"
            :label="`${t('configArchivePreviewUpdates')} ${totals.updates}`"
          />
          <UBadge
            size="sm"
            color="error"
            variant="subtle"
            :label="`${t('configArchivePreviewDeletes')} ${totals.deletes}`"
          />
          <UBadge
            v-if="totals.unrecoverable > 0"
            size="sm"
            color="warning"
            variant="subtle"
            :label="`${t('configArchivePreviewUnrecoverableSecrets')} ${totals.unrecoverable}`"
          />
        </div>
        <UAlert
          v-if="hasWarnings"
          color="warning"
          variant="subtle"
          icon="i-lucide-triangle-alert"
          :description="preview?.warnings.join(' · ')"
        />
        <div class="flex justify-end gap-2">
          <UButton
            type="button"
            size="sm"
            color="neutral"
            variant="outline"
            :disabled="busy"
            @click="emit('cancel')"
          >
            {{ t('cancel') }}
          </UButton>
          <UButton
            type="button"
            size="sm"
            color="error"
            :loading="busy"
            @click="emit('confirm')"
          >
            <UIcon name="i-lucide-upload" class="h-4 w-4" />
            {{ t('configArchiveImportConfirmSubmit') }}
          </UButton>
        </div>
      </div>
    </template>
  </UModal>
</template>
