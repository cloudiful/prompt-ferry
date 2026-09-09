<script setup lang="ts">
import { computed } from 'vue'
import { stripVersionSuffix } from './endpointBaseUrls'

// Per-`native_api` official GLM (Zhipu Coding Plan) base URLs.
// Issue #241: the form ships a one-click preset selector that fills
// the base URL field for the three official protocols; a `custom`
// option reveals the raw input for mirrors and third-party gateways.
const GLM_PRESET_BASES = {
  anthropic: 'https://open.bigmodel.cn/api/anthropic',
  chat_cn: 'https://open.bigmodel.cn/api/coding/paas/v4',
  chat_mirror: 'https://api.z.ai/api/coding/paas/v4',
  responses: 'https://open.bigmodel.cn/api/v1',
} as const
type GlmPresetKey = keyof typeof GLM_PRESET_BASES
const GLM_DEFAULT_BASE_URL = GLM_PRESET_BASES.chat_cn

const PRESET_ORDER: readonly GlmPresetKey[] = [
  'anthropic',
  'chat_cn',
  'chat_mirror',
  'responses',
] as const

const props = defineProps<{
  t: TranslateFn
}>()

const baseUrl = defineModel<string>('baseUrl', { required: true })

function matchesPreset(value: string): GlmPresetKey | null {
  const candidate = value.trim().replace(/\/+$/, '')
  if (!candidate) return null
  // Issue #241 P3-3: exact canonical preset URL first. A bare
  // `.../api` root must stay "custom" — it is a versionless base, not
  // the official `.../api/v1` Responses preset — even though the
  // runtime smart-join happens to converge both spellings on
  // `.../api/v1/responses`. Only spellings that still carry a
  // removable version segment (e.g. a chained `.../v1/v1`) fall back
  // to the stripped comparison.
  for (const key of PRESET_ORDER) {
    if (candidate === GLM_PRESET_BASES[key]) {
      return key
    }
  }
  const stripped = stripVersionSuffix(candidate)
  if (stripped === candidate) return null
  for (const key of PRESET_ORDER) {
    if (stripped === stripVersionSuffix(GLM_PRESET_BASES[key])) {
      return key
    }
  }
  return null
}

// Provider-switch hook: when the user picks GLM, the parent calls
// this to reset the base URL only if the current value is empty or
// matches a known upstream base for a different provider. The
// `nonGlmBases` list is supplied by the parent so the child does not
// need to know about MiniMax / CommandCode / OpencodeGo / OpenRouter.
function applyDefaultIfNonGlmBase(nonGlmBases: readonly string[]): void {
  const current = stripVersionSuffix(baseUrl.value)
  if (!current || nonGlmBases.includes(current)) {
    baseUrl.value = GLM_DEFAULT_BASE_URL
  }
}

defineExpose({ applyDefaultIfNonGlmBase })

const selectedPreset = computed<GlmPresetKey | 'custom'>({
  get(): GlmPresetKey | 'custom' {
    return matchesPreset(baseUrl.value) ?? 'custom'
  },
  set(value: GlmPresetKey | 'custom') {
    if (value === 'custom') {
      return
    }
    baseUrl.value = GLM_PRESET_BASES[value]
  },
})

const presetOptions = computed(() => [
  { label: props.t('providerGlmBaseUrlPresetAnthropic'), value: 'anthropic' },
  { label: props.t('providerGlmBaseUrlPresetChatCn'), value: 'chat_cn' },
  {
    label: props.t('providerGlmBaseUrlPresetChatMirror'),
    value: 'chat_mirror',
  },
  { label: props.t('providerGlmBaseUrlPresetResponses'), value: 'responses' },
  { label: props.t('providerGlmBaseUrlPresetCustom'), value: 'custom' },
])
</script>

<template>
  <div class="grid gap-1 md:grid-cols-[9rem_minmax(0,1fr)] md:items-center">
    <label class="text-xs text-muted" for="endpoint-glm-base-preset">
      {{ t('providerGlmBaseUrlPreset') }}
    </label>
    <USelect
      id="endpoint-glm-base-preset"
      v-model="selectedPreset"
      class="w-full"
      :items="presetOptions"
      label-key="label"
      value-key="value"
    />
  </div>
</template>
