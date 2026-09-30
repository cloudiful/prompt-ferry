<script setup lang="ts">
import type { ConfigImportPreview } from '@/generated/admin-api'

defineProps<{
  preview: ConfigImportPreview
  t: TranslateFn
}>()

function formatNumber(value: number): string {
  return value.toLocaleString()
}

function shortFingerprint(value: string): string {
  return value ? value.slice(0, 16) : '-'
}
</script>

<template>
  <section
    class="grid gap-2 rounded-md border border-default bg-muted/40 p-2.5"
  >
    <div class="flex flex-wrap items-center gap-2 text-[11px] text-muted">
      <span
        >{{ t('configArchiveExportBackend') }}: {{ preview.backend_kind }}</span
      >
      <span>
        {{ t('configArchiveExportFormatVersion') }}:
        {{ preview.format_version }}
      </span>
      <span class="font-mono">
        {{ t('configArchiveExportFingerprint') }}:
        {{ shortFingerprint(preview.payload_fingerprint) }}
      </span>
    </div>

    <ul class="m-0 grid list-none gap-1.5 p-0">
      <li
        v-for="domain in preview.domains"
        :key="domain.name"
        class="grid gap-1 rounded-md border border-default bg-default px-2 py-1.5"
      >
        <div class="flex flex-wrap items-center justify-between gap-2">
          <span class="text-xs font-medium text-highlighted">
            {{ domain.name }}
          </span>
          <span class="text-[11px] text-muted">
            {{ t('configArchivePreviewTargetRecords') }}:
            {{ formatNumber(domain.target_records) }}
          </span>
        </div>
        <div class="flex flex-wrap items-center gap-1.5">
          <UBadge
            size="xs"
            color="success"
            variant="subtle"
            :label="`${t('configArchivePreviewCreates')} ${formatNumber(domain.creates)}`"
          />
          <UBadge
            size="xs"
            color="info"
            variant="subtle"
            :label="`${t('configArchivePreviewUpdates')} ${formatNumber(domain.updates)}`"
          />
          <UBadge
            size="xs"
            color="error"
            variant="subtle"
            :label="`${t('configArchivePreviewDeletes')} ${formatNumber(domain.deletes)}`"
          />
          <UBadge
            v-if="domain.unrecoverable_secrets > 0"
            size="xs"
            color="warning"
            variant="subtle"
            :label="`${t('configArchivePreviewUnrecoverableSecrets')} ${formatNumber(domain.unrecoverable_secrets)}`"
          />
        </div>
      </li>
    </ul>

    <UAlert
      v-if="preview.warnings.length > 0"
      color="warning"
      variant="subtle"
      icon="i-lucide-triangle-alert"
      :description="preview.warnings.join(' · ')"
    />
  </section>
</template>
