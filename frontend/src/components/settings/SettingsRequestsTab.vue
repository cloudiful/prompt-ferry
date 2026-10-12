<script setup lang="ts">
import { computed } from 'vue'
import type { MessageKey } from '@/i18n'
import type {
  RequestContentLoggingMode,
  RequestContentLoggingResponse,
  UsageRetentionSettings,
} from '@/generated/admin-api'
import StreamDeltaBatchingFields from '@/components/shared/StreamDeltaBatchingFields.vue'
import type { StreamDeltaBatchingForm } from '@/models'
import type { SettingForm } from '@/stores/settings-actions'
import SettingsCard from './SettingsCard.vue'

const props = defineProps<{
  modelRouteWhitelistPending: boolean
  pendingRetentionForm: SettingForm | null
  streamDeltaBatchingPending: boolean
  t: TranslateFn
}>()

const retentionPending = computed(() => props.pendingRetentionForm !== null)
const requestContentLoggingPending = computed(
  () => props.pendingRetentionForm === 'requestContentLogging',
)
const usageRetentionPending = computed(
  () => props.pendingRetentionForm === 'usageRetention',
)

const requestContentLogging = defineModel<RequestContentLoggingResponse>(
  'requestContentLogging',
  { required: true },
)
const usageRetention = defineModel<UsageRetentionSettings>('usageRetention', {
  required: true,
})
const streamDeltaBatching = defineModel<StreamDeltaBatchingForm>(
  'streamDeltaBatching',
  { required: true },
)
const modelRouteWhitelist = defineModel<{ enabled: boolean }>(
  'modelRouteWhitelist',
  { required: true },
)

const requestContentLoggingModes: Array<{
  value: RequestContentLoggingMode
  label: MessageKey
}> = [
  { value: 'off', label: 'contentLoggingModeOff' },
  { value: 'normalized_only', label: 'contentLoggingModeNormalizedOnly' },
  { value: 'normalized_and_raw', label: 'contentLoggingModeNormalizedAndRaw' },
]

const requestContentLoggingModeOptions = computed(() =>
  requestContentLoggingModes.map((item) => ({
    label: props.t(item.label),
    value: item.value,
  })),
)
</script>

<template>
  <section class="grid gap-3">
    <div class="grid items-start gap-3 lg:grid-cols-2">
      <SettingsCard>
        <template #header>
          <h3
            class="m-0 inline-flex items-center gap-1.5 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
          >
            <UIcon name="i-lucide-file-text" class="h-3.5 w-3.5 text-muted" />
            {{ t('contentLogging') }}
            <span
              v-if="requestContentLoggingPending"
              role="status"
              :aria-label="t('loading')"
              class="inline-flex"
            >
              <UIcon
                name="i-lucide-loader-circle"
                class="h-3.5 w-3.5 animate-spin text-muted"
                aria-hidden="true"
              />
            </span>
          </h3>
          <span class="text-[11px] leading-none text-muted">{{
            t('contentLoggingOff')
          }}</span>
        </template>
        <div class="grid gap-3">
          <label class="grid gap-1" :aria-busy="retentionPending">
            <span class="text-xs text-muted">{{
              t('contentLoggingMode')
            }}</span>
            <USelect
              v-model="requestContentLogging.mode"
              :items="requestContentLoggingModeOptions"
              label-key="label"
              value-key="value"
              size="sm"
              class="w-full min-w-0"
              :disabled="retentionPending"
            />
          </label>
          <div class="grid gap-3 sm:grid-cols-2">
            <label class="grid gap-1" :aria-busy="retentionPending">
              <span class="text-xs text-muted">{{
                t('rawRetentionDays')
              }}</span>
              <UInputNumber
                v-model="requestContentLogging.raw_retention_days"
                size="sm"
                :min="1"
                :max="30"
                :use-grouping="false"
                :disabled="retentionPending"
              />
            </label>
            <label class="grid gap-1" :aria-busy="retentionPending">
              <span class="inline-flex items-center gap-1 text-xs text-muted">
                {{ t('approvalRetentionDays') }}
                <span
                  v-if="usageRetentionPending"
                  role="status"
                  :aria-label="t('loading')"
                  class="inline-flex"
                >
                  <UIcon
                    name="i-lucide-loader-circle"
                    class="h-3.5 w-3.5 animate-spin text-muted"
                    aria-hidden="true"
                  />
                </span>
              </span>
              <UInputNumber
                v-model="usageRetention.approval_retention_days"
                size="sm"
                :min="1"
                :use-grouping="false"
                :disabled="retentionPending"
              />
            </label>
          </div>
        </div>
      </SettingsCard>

      <SettingsCard>
        <template #header>
          <h3
            class="m-0 inline-flex items-center gap-1.5 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
          >
            <UIcon
              name="i-lucide-shield-check"
              class="h-3.5 w-3.5 text-muted"
            />
            {{ t('modelRouteWhitelist') }}
            <span
              v-if="modelRouteWhitelistPending"
              role="status"
              :aria-label="t('loading')"
              class="inline-flex"
            >
              <UIcon
                name="i-lucide-loader-circle"
                class="h-3.5 w-3.5 animate-spin text-muted"
                aria-hidden="true"
              />
            </span>
          </h3>
          <USwitch
            v-model="modelRouteWhitelist.enabled"
            id="settings-model-route-whitelist-enabled"
            :aria-label="t('modelRouteWhitelist')"
            :disabled="modelRouteWhitelistPending"
          />
        </template>
        <p class="m-0 text-xs leading-relaxed text-muted">
          {{ t('modelRouteWhitelistHint') }}
        </p>
      </SettingsCard>

      <SettingsCard class="lg:col-span-full">
        <template #header>
          <h3
            class="m-0 inline-flex items-center gap-1.5 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
          >
            <UIcon name="i-lucide-zap" class="h-3.5 w-3.5 text-muted" />
            {{ t('streamDeltaBatching') }}
            <span
              v-if="streamDeltaBatchingPending"
              role="status"
              :aria-label="t('loading')"
              class="inline-flex"
            >
              <UIcon
                name="i-lucide-loader-circle"
                class="h-3.5 w-3.5 animate-spin text-muted"
                aria-hidden="true"
              />
            </span>
          </h3>
          <USwitch
            v-model="streamDeltaBatching.enabled"
            id="settings-stream-delta-batching-enabled"
            :aria-label="t('streamDeltaBatching')"
            :disabled="streamDeltaBatchingPending"
          />
        </template>
        <fieldset
          :disabled="streamDeltaBatchingPending"
          :aria-busy="streamDeltaBatchingPending"
          class="m-0 min-w-0 border-0 p-0"
        >
          <StreamDeltaBatchingFields
            v-model:form="streamDeltaBatching"
            :t="t"
          />
        </fieldset>
        <p class="m-0 text-[11px] leading-relaxed text-muted">
          {{ t('streamDeltaBatchingSummary') }}
        </p>
      </SettingsCard>
    </div>
  </section>
</template>
