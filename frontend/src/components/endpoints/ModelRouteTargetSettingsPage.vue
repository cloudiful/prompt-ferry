<script setup lang="ts">
import { computed } from 'vue'
import ProxySettingsFields from '@/components/shared/ProxySettingsFields.vue'
import ScheduleWindowsFields from '@/components/shared/ScheduleWindowsFields.vue'
import type { ModelRouteTargetForm } from '@/models'

const props = defineProps<{
  t: TranslateFn
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
    <div class="grid gap-2">
      <div class="flex items-center gap-1">
        <span class="text-xs font-medium text-default">{{
          t('modelRouteNativeApi')
        }}</span>
        <UTooltip :text="t('modelRouteNativeApiHint')">
          <UButton
            type="button"
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-info"
            :aria-label="t('modelRouteNativeApiHint')"
          />
        </UTooltip>
      </div>
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
    </div>
    <div class="grid gap-2 border-t border-default pt-3">
      <div class="flex items-center gap-1">
        <span class="text-xs font-medium text-default">{{
          t('proxyUrlOverride')
        }}</span>
        <UTooltip :text="t('proxyUrlOverrideHint')">
          <UButton
            type="button"
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-info"
            :aria-label="t('proxyUrlOverrideHint')"
          />
        </UTooltip>
      </div>
      <ProxySettingsFields
        v-model:proxy-url="proxyUrl"
        v-model:has-saved="hasSaved"
        :t="t"
      />
    </div>
    <div class="grid gap-2 border-t border-default pt-3">
      <div class="flex items-center gap-1">
        <span class="text-xs font-medium text-default">{{
          t('scheduleWindows')
        }}</span>
        <UTooltip :text="t('scheduleWindowsHint')">
          <UButton
            type="button"
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-info"
            :aria-label="t('scheduleWindowsHint')"
          />
        </UTooltip>
      </div>
      <ScheduleWindowsFields
        v-model:windows="windows"
        v-model:touched="touched"
        :t="t"
      />
    </div>
    <div class="grid gap-2 border-t border-default pt-3">
      <div class="flex items-center gap-1">
        <span class="text-xs font-medium text-default">{{
          t('thinkingEffortOverride')
        }}</span>
        <UTooltip :text="t('thinkingEffortOverrideHint')">
          <UButton
            type="button"
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-info"
            :aria-label="t('thinkingEffortOverrideHint')"
          />
        </UTooltip>
      </div>
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
    </div>
    <div class="grid gap-2 border-t border-default pt-3">
      <div class="flex items-center gap-1">
        <span class="text-xs font-medium text-default">{{
          t('compactMode')
        }}</span>
        <UTooltip :text="t('compactModeHint')">
          <UButton
            type="button"
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-info"
            :aria-label="t('compactModeHint')"
          />
        </UTooltip>
      </div>
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
    </div>
    <div
      class="flex items-center justify-between gap-2 border-t border-default pt-3"
    >
      <div class="flex min-w-0 items-center gap-1">
        <span class="font-medium text-default">{{ t('normalizeLabel') }}</span>
        <UTooltip :text="t('normalizeHint')">
          <UButton
            type="button"
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-info"
            :aria-label="t('normalizeHint')"
          />
        </UTooltip>
      </div>
      <USwitch
        v-model="target.dev_system_normalize"
        :aria-label="t('normalizeLabel')"
      />
    </div>
    <div
      class="flex items-center justify-between gap-2 border-t border-default pt-3"
    >
      <div class="flex min-w-0 items-center gap-1">
        <span class="font-medium text-default">{{
          t('thinkingDowngradeLabel')
        }}</span>
        <UTooltip :text="t('thinkingDowngradeHint')">
          <UButton
            type="button"
            size="xs"
            color="neutral"
            variant="ghost"
            icon="i-lucide-info"
            :aria-label="t('thinkingDowngradeHint')"
          />
        </UTooltip>
      </div>
      <USwitch
        v-model="target.thinking_downgrade_enabled"
        :aria-label="t('thinkingDowngradeLabel')"
      />
    </div>
  </div>
</template>
