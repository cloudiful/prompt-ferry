<script setup lang="ts">
import { computed, ref } from 'vue'
import { useNotifier } from '@/composables/useNotifier'
import { useConfigArchiveStore } from '@/stores/settings/config-archive'
import SettingsCard from './SettingsCard.vue'
import ConfigArchiveImportConfirmDialog from './ConfigArchiveImportConfirmDialog.vue'
import ConfigArchiveImportPreview from './ConfigArchiveImportPreview.vue'

const props = defineProps<{ t: TranslateFn }>()

const store = useConfigArchiveStore()
const { notifyApiError, notifySuccess } = useNotifier()

const fileInput = ref<HTMLInputElement | null>(null)
const confirmOpen = ref(false)

const canPreview = computed(
  () =>
    Boolean(store.importFile) &&
    Boolean(store.importPassphrase) &&
    !store.previewing &&
    !store.applying,
)
const canApply = computed(() => Boolean(store.importPreview) && !store.applying)
const dialogOpen = computed(
  () => confirmOpen.value && Boolean(store.importPreview),
)

function chooseFile(): void {
  fileInput.value?.click()
}

async function onFileChange(event: Event): Promise<void> {
  const input = event.target as HTMLInputElement
  const file = input.files?.[0]
  // Clear so picking the same file again still fires `change`.
  input.value = ''
  if (!file) return
  try {
    await store.selectImportFile(file)
  } catch (cause) {
    notifyApiError(cause)
  }
}

function formatNumber(value: number): string {
  return value.toLocaleString()
}

function shortFingerprint(value: string | null | undefined): string {
  return value ? value.slice(0, 16) : '-'
}

async function previewImport(): Promise<void> {
  try {
    await store.previewImport()
  } catch (cause) {
    notifyApiError(cause)
  }
}

function requestApply(): void {
  if (!store.importPreview) return
  confirmOpen.value = true
}

async function confirmApply(): Promise<void> {
  try {
    await store.applyImport()
    notifySuccess(props.t('configArchiveImportApplied'))
  } catch (cause) {
    notifyApiError(cause)
  } finally {
    confirmOpen.value = false
  }
}

function cancelConfirm(): void {
  confirmOpen.value = false
  store.cancelImport()
}
</script>

<template>
  <SettingsCard class="lg:col-span-full">
    <template #header>
      <h3
        class="m-0 inline-flex items-center gap-1.5 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
      >
        <UIcon name="i-lucide-upload" class="h-3.5 w-3.5 text-muted" />
        {{ t('configArchiveImportTitle') }}
      </h3>
    </template>

    <p class="m-0 text-[11px] leading-relaxed text-muted">
      {{ t('configArchiveImportHelp') }}
    </p>

    <div class="grid gap-3 md:grid-cols-2">
      <div class="grid gap-1.5">
        <span class="text-xs font-medium text-muted">
          {{ t('configArchiveArchiveFile') }}
        </span>
        <input
          ref="fileInput"
          type="file"
          class="hidden"
          accept=".pfce,application/octet-stream"
          @change="onFileChange"
        />
        <div class="flex flex-wrap items-center gap-2">
          <UButton
            type="button"
            size="sm"
            color="neutral"
            variant="outline"
            icon="i-lucide-folder-open"
            @click="chooseFile"
          >
            {{ t('configArchiveChooseFile') }}
          </UButton>
          <span
            v-if="store.importFile"
            class="inline-flex min-w-0 items-center gap-1.5 text-xs text-default"
          >
            <UIcon
              name="i-lucide-file"
              class="h-3.5 w-3.5 shrink-0 text-muted"
            />
            <span class="truncate">{{ store.importFile.name }}</span>
            <span class="text-muted">
              {{ formatNumber(store.importFile.size) }}
            </span>
            <UButton
              type="button"
              size="xs"
              color="neutral"
              variant="ghost"
              icon="i-lucide-x"
              :aria-label="t('configArchiveRemoveFile')"
              @click="store.clearImportFlow()"
            />
          </span>
        </div>
      </div>

      <label class="grid gap-1.5">
        <span class="text-xs font-medium text-muted">
          {{ t('configArchivePassphrase') }}
        </span>
        <UInput
          v-model="store.importPassphrase"
          size="sm"
          type="password"
          autocomplete="new-password"
          :placeholder="t('configArchivePassphrasePlaceholder')"
        />
      </label>
    </div>

    <div class="flex flex-wrap items-center gap-2">
      <UButton
        type="button"
        size="sm"
        color="neutral"
        variant="outline"
        icon="i-lucide-search"
        :disabled="!canPreview"
        :loading="store.previewing"
        @click="previewImport"
      >
        {{ t('configArchivePreviewAction') }}
      </UButton>
      <UButton
        type="button"
        size="sm"
        color="error"
        icon="i-lucide-upload"
        :disabled="!canApply"
        :loading="store.applying"
        @click="requestApply"
      >
        {{ t('configArchiveApplyAction') }}
      </UButton>
    </div>

    <ConfigArchiveImportPreview
      v-if="store.importPreview"
      :preview="store.importPreview"
      :t="t"
    />

    <section
      v-if="store.importApplied"
      class="grid gap-1.5 rounded-md border border-success/30 bg-success/10 p-2.5"
    >
      <p class="m-0 inline-flex items-center gap-1.5 text-xs text-default">
        <UIcon
          name="i-lucide-circle-check"
          class="h-3.5 w-3.5 shrink-0 text-success"
        />
        {{ t('configArchiveImportApplied') }}
      </p>
      <p class="m-0 text-[11px] text-muted">
        {{ t('configArchiveExportFingerprint') }}:
        <span class="font-mono">{{
          shortFingerprint(store.importApplied.payload_fingerprint)
        }}</span>
      </p>
      <p class="m-0 flex flex-wrap gap-2 text-[11px] text-muted">
        <span v-for="domain in store.importApplied.domains" :key="domain.name">
          {{ domain.name }}: {{ formatNumber(domain.records) }}
        </span>
      </p>
    </section>

    <ConfigArchiveImportConfirmDialog
      :open="dialogOpen"
      :preview="store.importPreview"
      :busy="store.applying"
      :t="t"
      @cancel="cancelConfirm"
      @confirm="confirmApply"
    />
  </SettingsCard>
</template>
