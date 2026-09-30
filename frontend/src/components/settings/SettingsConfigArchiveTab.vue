<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted } from 'vue'
import type { TableColumn } from '@nuxt/ui'
import type { ConfigAuditEntry } from '@/generated/admin-api'
import { useNotifier } from '@/composables/useNotifier'
import { useConfigArchiveStore } from '@/stores/settings/config-archive'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/table-pagination'
import TablePagination from '@/components/shared/TablePagination.vue'
import SettingsCard from './SettingsCard.vue'
import ConfigArchiveImportCard from './ConfigArchiveImportCard.vue'

const props = defineProps<{ t: TranslateFn }>()

const store = useConfigArchiveStore()
const { notifyApiError, notifySuccess } = useNotifier()

const columns = computed<TableColumn<ConfigAuditEntry>[]>(() => [
  { id: 'action', header: props.t('configArchiveAuditAction') },
  {
    accessorKey: 'actor_login_name',
    header: props.t('configArchiveAuditActor'),
  },
  {
    accessorKey: 'backend_kind',
    header: props.t('configArchiveExportBackend'),
  },
  { id: 'archive_bytes', header: props.t('configArchiveAuditBytes') },
  {
    accessorKey: 'payload_fingerprint',
    header: props.t('configArchiveExportFingerprint'),
  },
  { id: 'records', header: props.t('configArchiveAuditRecords') },
  { id: 'result', header: props.t('configArchiveAuditResult') },
  { id: 'error_code', header: props.t('error') },
  { accessorKey: 'created_at', header: props.t('configArchiveAuditCreatedAt') },
])

function formatTime(value: string): string {
  return new Date(value).toLocaleString()
}

function formatNumber(value: number | null | undefined): string {
  return value == null ? '-' : value.toLocaleString()
}

function shortFingerprint(value: string | null | undefined): string {
  return value ? value.slice(0, 16) : '-'
}

function domainRecords(entry: ConfigAuditEntry): number {
  return entry.domains.reduce((total, domain) => total + domain.records, 0)
}

async function refreshAudit(
  first = store.auditFirst,
  rows = store.auditRows,
): Promise<void> {
  try {
    await store.refreshConfigAudit(first, rows)
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function onPageChange(event: TablePageChange): Promise<void> {
  await refreshAudit(event.first, event.rows)
}

async function exportArchive(): Promise<void> {
  try {
    await store.exportArchive()
    notifySuccess(props.t('configArchiveExportDone'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

onMounted(() => {
  void refreshAudit()
})

onBeforeUnmount(() => {
  store.resetConfigArchiveSecrets()
})
</script>

<template>
  <section class="grid min-w-0 gap-3">
    <SettingsCard class="lg:col-span-full">
      <template #header>
        <h3
          class="m-0 inline-flex items-center gap-1.5 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
        >
          <UIcon name="i-lucide-database" class="h-3.5 w-3.5 text-muted" />
          {{ t('configArchiveExportTitle') }}
        </h3>
      </template>

      <p class="m-0 text-[11px] leading-relaxed text-muted">
        {{ t('configArchiveExportHelp') }}
      </p>

      <div class="grid items-end gap-3 md:grid-cols-2">
        <label class="grid gap-1.5">
          <span class="text-xs font-medium text-muted">
            {{ t('configArchivePassphrase') }}
          </span>
          <UInput
            v-model="store.exportPassphrase"
            size="sm"
            type="password"
            autocomplete="new-password"
            :placeholder="t('configArchivePassphrasePlaceholder')"
          />
        </label>
        <UButton
          type="button"
          size="sm"
          icon="i-lucide-arrow-down"
          class="justify-self-start"
          :disabled="!store.exportPassphrase"
          :loading="store.exporting"
          @click="exportArchive"
        >
          {{ t('configArchiveExportAction') }}
        </UButton>
      </div>

      <div
        v-if="store.exportSummary"
        class="flex flex-wrap items-center gap-2 text-[11px] text-muted"
      >
        <span>
          {{ t('configArchiveExportBackend') }}:
          {{ store.exportSummary.backendKind }}
        </span>
        <span>
          {{ t('configArchiveExportFormatVersion') }}:
          {{ store.exportSummary.formatVersion ?? '-' }}
        </span>
        <span>
          {{ t('configArchiveExportSize') }}:
          {{ formatNumber(store.exportSummary.size) }}
        </span>
        <span class="font-mono">
          {{ t('configArchiveExportFingerprint') }}:
          {{ shortFingerprint(store.exportSummary.payloadFingerprint) }}
        </span>
      </div>

      <p class="m-0 text-[11px] leading-relaxed text-muted">
        {{ t('configArchiveExportFootnote') }}
      </p>
    </SettingsCard>

    <ConfigArchiveImportCard :t="t" />

    <SettingsCard class="lg:col-span-full" body-class="p-0">
      <template #header>
        <h3
          class="m-0 inline-flex items-center gap-1.5 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
        >
          <UIcon name="i-lucide-clock" class="h-3.5 w-3.5 text-muted" />
          {{ t('configArchiveAuditTitle') }}
        </h3>
        <UButton
          size="xs"
          color="neutral"
          variant="soft"
          icon="i-lucide-refresh-cw"
          :loading="store.auditLoading"
          @click="refreshAudit()"
        >
          {{ t('refresh') }}
        </UButton>
      </template>

      <UTable
        :data="store.auditEntries"
        :columns="columns"
        :loading="store.auditLoading"
        class="min-w-0"
        :ui="{ th: 'whitespace-nowrap' }"
      >
        <template #action-cell="{ row }">
          <UBadge
            size="xs"
            variant="subtle"
            :color="row.original.action === 'export' ? 'info' : 'warning'"
            :label="
              row.original.action === 'export'
                ? t('configArchiveAuditActionExport')
                : t('configArchiveAuditActionImport')
            "
          />
        </template>
        <template #actor_login_name-cell="{ row }">
          {{ row.original.actor_login_name || '-' }}
        </template>
        <template #archive_bytes-cell="{ row }">
          {{ formatNumber(row.original.archive_bytes) }}
        </template>
        <template #payload_fingerprint-cell="{ row }">
          <span class="font-mono">{{
            shortFingerprint(row.original.payload_fingerprint)
          }}</span>
        </template>
        <template #records-cell="{ row }">
          {{ formatNumber(domainRecords(row.original)) }}
        </template>
        <template #result-cell="{ row }">
          <UBadge
            size="xs"
            variant="subtle"
            :color="row.original.success ? 'success' : 'error'"
            :label="row.original.success ? t('success') : t('failed')"
          />
        </template>
        <template #error_code-cell="{ row }">
          <div class="grid gap-px">
            <span>{{ row.original.error_code || '-' }}</span>
            <span
              v-if="row.original.error_message"
              class="block max-w-[16rem] truncate text-[11px] text-muted"
              :title="row.original.error_message"
            >
              {{ row.original.error_message }}
            </span>
          </div>
        </template>
        <template #created_at-cell="{ row }">
          {{ formatTime(row.original.created_at) }}
        </template>
        <template #empty>
          <p class="m-0 p-3 text-xs text-muted">
            {{ t('configArchiveAuditEmpty') }}
          </p>
        </template>
      </UTable>

      <TablePagination
        :first="store.auditFirst"
        :rows="store.auditRows"
        :total="store.auditTotal"
        :page-size-options="STANDARD_PAGE_SIZE_OPTIONS"
        @change="onPageChange"
      />
    </SettingsCard>
  </section>
</template>
