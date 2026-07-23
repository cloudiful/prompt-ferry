<script setup lang="ts">
import { computed } from 'vue'
import type { MessageKey } from '@/i18n'
import type {
  RequestContentLoggingMode,
  RequestContentLoggingResponse,
} from '@/generated/admin-api'
import StreamDeltaBatchingFields from '@/components/shared/StreamDeltaBatchingFields.vue'
import type { StreamDeltaBatchingForm } from '@/models'
import SettingsCard from './SettingsCard.vue'

const props = defineProps<{
  t: TranslateFn
}>()

const requestContentLogging = defineModel<RequestContentLoggingResponse>(
  'requestContentLogging',
  { required: true },
)
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
  <section class="grid gap-4">
    <div class="grid items-start gap-3 lg:grid-cols-2">
      <SettingsCard>
        <template #header>
          <div class="min-w-0">
            <h3
              class="m-0 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
            >
              {{ t('contentLogging') }}
            </h3>
          </div>
        </template>
        <div class="grid items-end gap-3 md:grid-cols-[12rem_auto]">
          <label class="grid gap-1.5">
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
            />
          </label>

          <label class="grid gap-1.5">
            <span class="text-xs text-muted">{{ t('rawRetentionDays') }}</span>
            <UInputNumber
              v-model="requestContentLogging.raw_retention_days"
              size="sm"
              :min="1"
              :max="30"
              :use-grouping="false"
            />
          </label>
        </div>
      </SettingsCard>

      <SettingsCard>
        <template #header>
          <div class="grid gap-2">
            <h3
              class="m-0 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
            >
              {{ t('modelRouteWhitelist') }}
            </h3>
            <label
              for="settings-model-route-whitelist-enabled"
              class="inline-flex cursor-pointer items-center gap-2 select-none"
            >
              <USwitch
                v-model="modelRouteWhitelist.enabled"
                id="settings-model-route-whitelist-enabled"
              />
              <span class="text-xs text-dimmed">{{
                modelRouteWhitelist.enabled ? t('active') : t('disabled')
              }}</span>
            </label>
          </div>
        </template>
      </SettingsCard>

      <SettingsCard class="lg:col-span-full">
        <template #header>
          <div class="grid gap-2">
            <h3
              class="m-0 text-[0.82rem] leading-[1.3] font-semibold text-highlighted"
            >
              {{ t('streamDeltaBatching') }}
            </h3>
            <label
              for="settings-stream-delta-batching-enabled"
              class="inline-flex cursor-pointer items-center gap-2 select-none"
            >
              <USwitch
                v-model="streamDeltaBatching.enabled"
                id="settings-stream-delta-batching-enabled"
              />
              <span class="text-xs text-dimmed">{{
                streamDeltaBatching.enabled ? t('active') : t('disabled')
              }}</span>
            </label>
          </div>
        </template>
        <StreamDeltaBatchingFields v-model:form="streamDeltaBatching" :t="t" />
      </SettingsCard>
    </div>
  </section>
</template>
