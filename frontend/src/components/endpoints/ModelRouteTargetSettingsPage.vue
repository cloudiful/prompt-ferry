<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import ProxySettingsFields from '@/components/shared/ProxySettingsFields.vue'
import ScheduleWindowsFields from '@/components/shared/ScheduleWindowsFields.vue'
import ServiceTierOverrideField from '@/components/shared/ServiceTierOverrideField.vue'
import SettingsFieldRow from '@/components/shared/SettingsFieldRow.vue'
import { parseEmbeddingDimensionsInput } from '@/admin-mappers'
import type { ModelRouteTargetForm } from '@/models'
import type { NativeApi } from '@/generated/admin-api'
import {
  effectiveTargetProtocol,
  normalizeNativeApiAxis,
  supportsChatAdaptationFor,
  supportsEmbeddingDimensionOverrideFor,
  supportsServiceTierFor,
} from '@/models/endpoints/service-tier'

const props = defineProps<{
  t: TranslateFn
  // Issue #1032: the endpoint's own protocol. A target with `auto` inherits it,
  // so the settings page can resolve the effective protocol without guessing
  // from the model name.
  endpointProtocol?: NativeApi | null
}>()

const target = defineModel<ModelRouteTargetForm>('target', { required: true })

const endpointProtocolAxis = computed<NativeApi>(() =>
  normalizeNativeApiAxis(props.endpointProtocol),
)

const effectiveProtocol = computed(() =>
  effectiveTargetProtocol(target.value?.native_api, endpointProtocolAxis.value),
)

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
  get():
    | 'auto'
    | 'anthropic_messages'
    | 'chat'
    | 'embeddings'
    | 'responses'
    | 'realtime' {
    const value = target.value?.native_api ?? 'auto'
    return value === 'anthropic_messages' ||
      value === 'responses' ||
      value === 'chat' ||
      value === 'embeddings' ||
      value === 'realtime'
      ? value
      : 'auto'
  },
  set(
    value:
      | 'auto'
      | 'anthropic_messages'
      | 'chat'
      | 'embeddings'
      | 'responses'
      | 'realtime',
  ) {
    if (target.value) target.value.native_api = value
  },
})

// Issue #1032: an embeddings target carries only the dimension override; the
// chat-only controls (service tier, thinking effort, thinking downgrade,
// compact mode, developer->system normalize) have no meaning there and stay
// hidden. `auto` keeps them because the runtime resolves the operation per
// request and the UI never guesses.
const chatAdaptationEligible = computed(() =>
  supportsChatAdaptationFor(effectiveProtocol.value),
)
const serviceTierEligible = computed(() =>
  supportsServiceTierFor(effectiveProtocol.value),
)
const dimensionEligible = computed(() =>
  supportsEmbeddingDimensionOverrideFor(effectiveProtocol.value),
)

// Issue #1032: the dimension control. `null` inherits the caller's dimensions;
// a positive integer forces the target's dimension count. The raw text is kept
// locally so a malformed entry stays visible (and blocks save) instead of
// being silently turned into inherit.
const dimensionText = ref('')
watch(
  () => target.value,
  (value) => {
    dimensionText.value =
      value?.embedding_dimensions_override == null
        ? ''
        : String(value.embedding_dimensions_override)
    if (value) value.embedding_dimensions_invalid = false
  },
  { immediate: true },
)

const dimensionInvalid = computed(
  () => target.value?.embedding_dimensions_invalid === true,
)

function onDimensionInput(raw: string): void {
  dimensionText.value = raw
  if (!target.value) return
  const parsed = parseEmbeddingDimensionsInput(raw)
  target.value.embedding_dimensions_invalid = parsed.invalid
  if (!parsed.invalid) target.value.embedding_dimensions_override = parsed.value
}

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
          { label: t('nativeApiEmbeddings'), value: 'embeddings' },
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
    <SettingsFieldRow
      v-if="dimensionEligible"
      :label="t('embeddingDimensionsOverride')"
      :hint="t('embeddingDimensionsOverrideHint')"
      for-id="target-embedding-dimensions"
    >
      <div class="grid gap-1">
        <UInput
          id="target-embedding-dimensions"
          :model-value="dimensionText"
          class="w-full"
          inputmode="numeric"
          :placeholder="t('embeddingDimensionsInherit')"
          :aria-invalid="dimensionInvalid"
          :aria-describedby="
            dimensionInvalid ? 'target-embedding-dimensions-error' : undefined
          "
          @update:model-value="onDimensionInput"
        />
        <p
          v-if="dimensionInvalid"
          id="target-embedding-dimensions-error"
          class="text-xs text-error"
        >
          {{ t('embeddingDimensionsInvalid') }}
        </p>
      </div>
    </SettingsFieldRow>
    <ServiceTierOverrideField
      v-if="chatAdaptationEligible && serviceTierEligible"
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
      v-if="chatAdaptationEligible"
      :label="t('thinkingEffortOverride')"
      :hint="t('thinkingEffortOverrideHint')"
    >
      <USelect
        v-model="thinkingEffort"
        class="w-full"
        :ui="{
          // Issue #1004 P1: the reasoning list scrolls only when the
          // available viewport space is insufficient. The content follows the
          // height Reka measured for this instance and stays inside the
          // viewport.
          content:
            'max-h-[min(var(--reka-select-content-available-height,calc(100vh_-_4rem)),calc(100vh_-_4rem))]',
        }"
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
    <SettingsFieldRow
      v-if="chatAdaptationEligible"
      :label="t('compactMode')"
      :hint="t('compactModeHint')"
    >
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
    <SettingsFieldRow
      v-if="chatAdaptationEligible"
      :label="t('normalizeLabel')"
      :hint="t('normalizeHint')"
    >
      <div class="flex md:justify-end">
        <USwitch
          v-model="target.dev_system_normalize"
          :aria-label="t('normalizeLabel')"
        />
      </div>
    </SettingsFieldRow>
    <SettingsFieldRow
      v-if="chatAdaptationEligible"
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
