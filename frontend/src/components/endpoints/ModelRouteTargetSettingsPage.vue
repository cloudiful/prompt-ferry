<script setup lang="ts">
import { computed } from 'vue'
import ProxySettingsFields from '@/components/shared/ProxySettingsFields.vue'
import ScheduleWindowsFields from '@/components/shared/ScheduleWindowsFields.vue'
import ServiceTierOverrideField from '@/components/shared/ServiceTierOverrideField.vue'
import SettingsFieldRow from '@/components/shared/SettingsFieldRow.vue'
import type { EndpointProvider } from '@/generated/admin-api'
import type { ModelRouteTargetForm } from '@/models'
import { supportsServiceTierFor } from '@/models/endpoints/service-tier'

const props = defineProps<{
  t: TranslateFn
  endpointProvider?: EndpointProvider | null
}>()

const target = defineModel<ModelRouteTargetForm>('target', { required: true })

const proxyUrl = computed({
  get: () => target.value?.proxy_url_override ?? '',
  set: (value: string) => {
    if (target.value) target.value.proxy_url_override = value
  },
})

const hasSaved = computed({
  get: () => target.value?.has_saved_proxy_url_override ?? false,
  set: (value: boolean) => {
    if (target.value) target.value.has_saved_proxy_url_override = value
  },
})

const windows = computed({
  get: () => target.value?.active_windows ?? [],
  set: (value: Array<{ start: string; end: string }>) => {
    if (target.value) target.value.active_windows = value
  },
})

// Issue #409 Phase 2: per-target port type, default Auto (follow caller).
// Reuses EndpointProviderFields USelect paradigm + endpoint i18n labels.
const nativeApiSelection = computed({
  get(): 'auto' | 'anthropic_messages' | 'chat' | 'responses' | 'realtime' {
    const value = target.value?.native_api ?? 'auto'
    return value === 'anthropic_messages' ||
      value === 'responses' ||
      value === 'chat' ||
      value === 'realtime'
      ? value
      : 'auto'
  },
  set(
    value: 'auto' | 'anthropic_messages' | 'chat' | 'responses' | 'realtime',
  ) {
    if (target.value) target.value.native_api = value
  },
})

// Issue #637: expose the free-form target override only when the selected
// endpoint provider and this target's port type are a documented pair; the
// provider travels from the endpoint picker into this settings view.
const serviceTierEligible = computed(() =>
  supportsServiceTierFor(
    props.endpointProvider ?? null,
    nativeApiSelection.value,
  ),
)

const serviceTier = computed({
  get: () => target.value?.service_tier ?? null,
  set: (value: string | null) => {
    if (target.value) target.value.service_tier = value
  },
})

const touched = computed({
  get: () => target.value?.active_windows_touched ?? false,
  set: (value: boolean) => {
    if (target.value) target.value.active_windows_touched = value
  },
})

// Issue #464: per-target thinking effort override, default inherit
// (follow caller). An explicit value force-replaces the caller value on
// Chat (`reasoning_effort`) and Responses (`reasoning.effort`).
const thinkingEffort = computed({
  get: () => target.value?.thinking_effort_override ?? 'inherit',
  set: (value: string) => {
    if (target.value)
      target.value.thinking_effort_override = value === 'inherit' ? null : value
  },
})

// Issue #502 Task 5: per-target compact mode, default passthrough
// (no new default semantics). self_summarize enables ferry-side handoff
// summarization for non-Responses targets; off rejects compact explicitly.
const compactMode = computed({
  get: () => target.value?.compact_mode ?? 'passthrough',
  set: (value: string) => {
    if (target.value)
      target.value.compact_mode =
        value === 'self_summarize' || value === 'off' ? value : 'passthrough'
  },
})
</script>

<template>
  <div
    v-if="target"
    class="grid gap-3 rounded border border-default bg-muted p-3"
  >
    <SettingsFieldRow
      :label="t('modelRouteNativeApi')"
      :hint="t('modelRouteNativeApiHint')"
    >
      <USelect
        v-model="nativeApiSelection"
        class="w-full"
        :items="[
          { label: t('endpointSourceAuto'), value: 'auto' },
          { label: t('nativeApiChat'), value: 'chat' },
          { label: t('nativeApiResponses'), value: 'responses' },
          {
            label: t('nativeApiAnthropicMessages'),
            value: 'anthropic_messages',
          },
          { label: t('nativeApiRealtime'), value: 'realtime' },
        ]"
        label-key="label"
        value-key="value"
      />
    </SettingsFieldRow>
    <ServiceTierOverrideField
      v-if="serviceTierEligible"
      v-model="serviceTier"
      :t="t"
      input-id="target-service-tier"
    />
    <SettingsFieldRow
      :label="t('proxyUrlOverride')"
      :hint="t('proxyUrlOverrideHint')"
    >
      <ProxySettingsFields
        v-model:proxy-url="proxyUrl"
        v-model:has-saved="hasSaved"
        :t="t"
      />
    </SettingsFieldRow>
    <SettingsFieldRow
      :label="t('scheduleWindows')"
      :hint="t('scheduleWindowsHint')"
    >
      <ScheduleWindowsFields
        v-model:windows="windows"
        v-model:touched="touched"
        :t="t"
      />
    </SettingsFieldRow>
    <SettingsFieldRow
      :label="t('thinkingEffortOverride')"
      :hint="t('thinkingEffortOverrideHint')"
    >
      <USelect
        v-model="thinkingEffort"
        class="w-full"
        :items="[
          { label: t('thinkingEffortInherit'), value: 'inherit' },
          { label: 'none', value: 'none' },
          { label: 'minimal', value: 'minimal' },
          { label: 'low', value: 'low' },
          { label: 'medium', value: 'medium' },
          { label: 'high', value: 'high' },
          { label: 'xhigh', value: 'xhigh' },
          { label: 'max', value: 'max' },
        ]"
        label-key="label"
        value-key="value"
      />
    </SettingsFieldRow>
    <SettingsFieldRow :label="t('compactMode')" :hint="t('compactModeHint')">
      <USelect
        v-model="compactMode"
        class="w-full"
        :items="[
          { label: t('compactModePassthrough'), value: 'passthrough' },
          { label: t('compactModeSelfSummarize'), value: 'self_summarize' },
          { label: t('compactModeOff'), value: 'off' },
        ]"
        label-key="label"
        value-key="value"
      />
    </SettingsFieldRow>
    <SettingsFieldRow :label="t('normalizeLabel')" :hint="t('normalizeHint')">
      <div class="flex md:justify-end">
        <USwitch
          v-model="target.dev_system_normalize"
          :aria-label="t('normalizeLabel')"
        />
      </div>
    </SettingsFieldRow>
    <SettingsFieldRow
      :label="t('thinkingDowngradeLabel')"
      :hint="t('thinkingDowngradeHint')"
    >
      <div class="flex md:justify-end">
        <USwitch
          v-model="target.thinking_downgrade_enabled"
          :aria-label="t('thinkingDowngradeLabel')"
        />
      </div>
    </SettingsFieldRow>
  </div>
</template>
