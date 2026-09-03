<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import type { RawObjectStoreBackend } from '@/generated/admin-api'
import { useNotifier } from '@/composables/useNotifier'
import { useSettingsStore } from '@/stores/settings'
import SettingsCard from './SettingsCard.vue'
import SettingsStorageCredentials from './SettingsStorageCredentials.vue'

const props = defineProps<{ t: TranslateFn }>()
const settingsStore = useSettingsStore()
const { notifyApiError, notifySuccess } = useNotifier()

const draft = reactive({
  backend: 'local' as RawObjectStoreBackend,
  local_dir: '',
  s3_endpoint: '',
  s3_bucket: '',
  s3_region: 'auto',
  s3_prefix: 'prompt-ferry/raw',
  s3_allow_http: false,
})

const s3AccessKeyInput = ref('')
const s3SecretKeyInput = ref('')
const clearAccessKey = ref(false)
const clearSecretKey = ref(false)
const saving = ref(false)
const localError = ref<string | null>(null)

const backendOptions = computed(() => [
  { label: props.t('storageBackendLocal'), value: 'local' as const },
  { label: props.t('storageBackendS3'), value: 's3' as const },
  { label: props.t('storageBackendDisabled'), value: 'disabled' as const },
])

function syncFromStore(): void {
  const src = settingsStore.rawObjectStore
  draft.backend = src.backend
  draft.local_dir = src.local_dir
  draft.s3_endpoint = src.s3_endpoint
  draft.s3_bucket = src.s3_bucket
  draft.s3_region = src.s3_region
  draft.s3_prefix = src.s3_prefix
  draft.s3_allow_http = src.s3_allow_http
  s3AccessKeyInput.value = ''
  s3SecretKeyInput.value = ''
  clearAccessKey.value = false
  clearSecretKey.value = false
  localError.value = null
}

watch(() => settingsStore.rawObjectStore, syncFromStore, {
  deep: true,
  immediate: true,
})

const isS3 = computed(() => draft.backend === 's3')
const isLocal = computed(() => draft.backend === 'local')
const isUnavailable = computed(() => Boolean(settingsStore.rawObjectStoreError))

function buildSecretPatch(input: string, shouldClear: boolean) {
  if (shouldClear) return { mode: 'clear' as const }
  const t = input.trim()
  if (t) return { mode: 'replace' as const, value: t }
  return { mode: 'keep' as const }
}

async function save(): Promise<void> {
  localError.value = null
  if (isS3.value && !draft.s3_bucket.trim()) {
    localError.value = props.t('storageSaveFailed')
    notifyApiError(new Error(`${props.t('storageS3Bucket')} is required`))
    return
  }
  saving.value = true
  try {
    await settingsStore.saveRawObjectStore({
      backend: draft.backend,
      local_dir: draft.local_dir,
      s3_endpoint: draft.s3_endpoint,
      s3_bucket: draft.s3_bucket,
      s3_region: draft.s3_region || 'auto',
      s3_prefix: draft.s3_prefix,
      s3_allow_http: draft.s3_allow_http,
      s3_access_key: buildSecretPatch(
        s3AccessKeyInput.value,
        clearAccessKey.value,
      ),
      s3_secret_key: buildSecretPatch(
        s3SecretKeyInput.value,
        clearSecretKey.value,
      ),
    })
    syncFromStore()
    notifySuccess(props.t('storageSaved'))
  } catch (cause) {
    localError.value = cause instanceof Error ? cause.message : String(cause)
    notifyApiError(cause)
  } finally {
    saving.value = false
  }
}

async function refresh(): Promise<void> {
  try {
    await settingsStore.refreshRawObjectStore()
  } catch (cause) {
    notifyApiError(cause)
  }
}
</script>

<template>
  <section class="grid gap-3">
    <SettingsCard>
      <template #header>
        <h3
          class="m-0 inline-flex items-center gap-1.5 text-[0.82rem] font-semibold text-highlighted"
        >
          <UIcon name="i-lucide-hard-drive" class="h-3.5 w-3.5 text-muted" />
          {{ t('storageTitle') }}
        </h3>
        <div class="flex flex-wrap items-center gap-2">
          <USelect
            v-model="draft.backend"
            size="sm"
            :items="backendOptions"
            label-key="label"
            value-key="value"
            class="min-w-32"
            :disabled="isUnavailable"
          />
          <UButton
            size="sm"
            icon="i-lucide-save"
            :loading="saving || settingsStore.loading"
            :disabled="isUnavailable"
            @click="save"
          >
            {{ t('save') }}
          </UButton>
        </div>
      </template>

      <div
        v-if="isUnavailable"
        class="rounded-md border border-warning/30 bg-warning/10 px-3 py-2"
      >
        <p
          class="m-0 flex items-center gap-1.5 text-xs leading-relaxed text-muted"
        >
          <UIcon
            name="i-lucide-triangle-alert"
            class="h-3.5 w-3.5 shrink-0 text-warning"
          />
          {{ t('storageUnavailable') }}
        </p>
        <p
          v-if="settingsStore.rawObjectStoreError"
          class="m-0 mt-1 text-[11px] text-muted"
        >
          {{ settingsStore.rawObjectStoreError }}
        </p>
        <div class="mt-2">
          <UButton
            size="xs"
            color="neutral"
            variant="soft"
            icon="i-lucide-refresh-cw"
            @click="refresh"
          >
            {{ t('refresh') }}
          </UButton>
        </div>
      </div>

      <div v-else class="grid gap-3">
        <div
          v-if="localError"
          class="rounded-md border border-error/30 bg-error/10 px-3 py-2 text-xs text-error"
        >
          {{ localError }}
        </div>

        <div v-if="isLocal" class="grid gap-1">
          <label class="grid gap-1">
            <span
              class="inline-flex items-center gap-1 text-xs font-medium text-muted"
            >
              {{ t('storageLocalDir') }}
              <UTooltip :text="t('storageLocalDirHelp')">
                <UButton
                  type="button"
                  size="xs"
                  color="neutral"
                  variant="ghost"
                  icon="i-lucide-info"
                  :aria-label="t('storageLocalDirHelp')"
                />
              </UTooltip>
            </span>
            <UInput
              v-model="draft.local_dir"
              size="sm"
              :placeholder="t('storageLocalDirPlaceholder')"
            />
          </label>
          <p class="m-0 text-[11px] text-muted">{{ t('storageLocalHint') }}</p>
        </div>

        <div v-else-if="isS3" class="grid gap-3">
          <div class="grid gap-3 sm:grid-cols-2">
            <label class="grid gap-1">
              <span class="text-xs font-medium text-muted">{{
                t('storageS3Endpoint')
              }}</span>
              <UInput
                v-model="draft.s3_endpoint"
                size="sm"
                :placeholder="t('storageS3EndpointPlaceholder')"
              />
            </label>
            <label class="grid gap-1">
              <span class="text-xs font-medium text-muted"
                >{{ t('storageS3Bucket') }} *</span
              >
              <UInput
                v-model="draft.s3_bucket"
                size="sm"
                :placeholder="t('storageS3BucketPlaceholder')"
              />
            </label>
            <label class="grid gap-1">
              <span class="text-xs font-medium text-muted">{{
                t('storageS3Region')
              }}</span>
              <UInput
                v-model="draft.s3_region"
                size="sm"
                :placeholder="t('storageS3RegionPlaceholder')"
              />
            </label>
            <label class="grid gap-1">
              <span class="text-xs font-medium text-muted">{{
                t('storageS3Prefix')
              }}</span>
              <UInput
                v-model="draft.s3_prefix"
                size="sm"
                :placeholder="t('storageS3PrefixPlaceholder')"
              />
            </label>
          </div>

          <label class="inline-flex items-center gap-2">
            <USwitch v-model="draft.s3_allow_http" />
            <span
              class="inline-flex items-center gap-1 text-xs font-medium text-muted"
            >
              {{ t('storageS3AllowHttp') }}
              <UTooltip :text="t('storageS3AllowHttpHelp')">
                <UButton
                  type="button"
                  size="xs"
                  color="neutral"
                  variant="ghost"
                  icon="i-lucide-info"
                  :aria-label="t('storageS3AllowHttpHelp')"
                />
              </UTooltip>
            </span>
          </label>

          <SettingsStorageCredentials
            v-model:access-key-input="s3AccessKeyInput"
            v-model:secret-key-input="s3SecretKeyInput"
            v-model:clear-access-key="clearAccessKey"
            v-model:clear-secret-key="clearSecretKey"
            :t="t"
            :has-access-key="settingsStore.rawObjectStore.has_s3_access_key"
            :has-secret-key="settingsStore.rawObjectStore.has_s3_secret_key"
          />
          <p class="m-0 text-[11px] text-muted">{{ t('storageSecretHint') }}</p>
          <p class="m-0 text-[11px] text-muted">{{ t('storageS3Hint') }}</p>
        </div>

        <div v-else class="grid gap-1">
          <p class="m-0 text-xs text-muted">{{ t('storageDisabledHint') }}</p>
        </div>
      </div>
    </SettingsCard>
  </section>
</template>
