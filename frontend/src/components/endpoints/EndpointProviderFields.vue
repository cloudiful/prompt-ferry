<script setup lang="ts">
import { computed } from 'vue'
import type { EndpointForm } from '@/models'

const props = defineProps<{
  t: TranslateFn
}>()

const form = defineModel<EndpointForm>('form', { required: true })
const minimaxBaseUrls = {
  cn: {
    openai: 'https://api.minimaxi.com',
    anthropic: 'https://api.minimaxi.com/anthropic',
  },
  global: {
    openai: 'https://api.minimax.io',
    anthropic: 'https://api.minimax.io/anthropic',
  },
} as const
type MinimaxProtocol = 'openai' | 'anthropic'
const hasVersionPath = computed(() =>
  /\/v1\/?$/.test(form.value.base_url.trim()),
)
const providerSelection = computed({
  get: () => form.value.provider,
  set(value: 'generic' | 'minimax') {
    form.value.provider = value
    if (value === 'generic') {
      form.value.provider_region = null
      // Service tier only applies to MiniMax upstreams; reset to the default
      // so a later switch back starts from standard behavior.
      form.value.service_tier = 'standard'
      // MCP exposure is only valid for MiniMax endpoints; backend validation
      // rejects an explicit `mcp_enabled: true` for generic providers, so
      // collapse to false here as well to keep the UI in sync.
      form.value.mcp_enabled = false
      return
    }
    const region = form.value.provider_region ?? 'cn'
    form.value.provider_region = region
    // Preserve an explicit priority selection across provider switches;
    // normalize legacy/unknown values to the standard default.
    form.value.service_tier =
      form.value.service_tier === 'priority' ? 'priority' : 'standard'
    if (!form.value.endpoint_id) {
      form.value.mcp_enabled = true
    }
    if (form.value.protocol_mode === 'auto') {
      form.value.protocol_mode = 'manual'
      form.value.native_api_override = 'anthropic_messages'
    }
    setMinimaxBaseUrl(region, activeMinimaxProtocol())
  },
})
const providerRegionSelection = computed({
  get: () => form.value.provider_region ?? 'cn',
  set(value: 'cn' | 'global') {
    form.value.provider_region = value
    setMinimaxBaseUrl(value, activeMinimaxProtocol())
  },
})
const isMinimax = computed(() => form.value.provider === 'minimax')
const serviceTierSelection = computed({
  get: () => (form.value.service_tier === 'priority' ? 'priority' : 'standard'),
  set(value: 'standard' | 'priority') {
    form.value.service_tier = value
  },
})
const serviceTierOptions = computed(() => [
  { label: props.t('serviceTierStandard'), value: 'standard' },
  { label: props.t('serviceTierPriority'), value: 'priority' },
])
const usesCustomMinimaxBaseUrl = computed(() => {
  if (!isMinimax.value) return false
  const current = form.value.base_url.trim().replace(/\/+$/, '')
  const known = Object.values(minimaxBaseUrls).flatMap((urls) =>
    Object.values(urls),
  )
  return Boolean(current) && !known.includes(current as (typeof known)[number])
})
const protocolSelection = computed({
  get(): 'auto' | 'anthropic_messages' | 'responses' | 'chat' | 'realtime' {
    if (form.value.protocol_mode === 'auto') return 'auto'
    return form.value.native_api_override ?? 'responses'
  },
  set(
    value: 'auto' | 'anthropic_messages' | 'responses' | 'chat' | 'realtime',
  ) {
    if (value === 'auto') {
      form.value.protocol_mode = 'auto'
      form.value.native_api_override = null
      if (isMinimax.value) {
        setMinimaxBaseUrl(form.value.provider_region ?? 'cn', 'openai')
      }
      return
    }
    form.value.protocol_mode = 'manual'
    form.value.native_api_override = value
    if (isMinimax.value) {
      setMinimaxBaseUrl(
        form.value.provider_region ?? 'cn',
        value === 'anthropic_messages' ? 'anthropic' : 'openai',
      )
    }
  },
})
const baseUrlHintText = computed(() => {
  const hints = [props.t('baseUrlHint')]
  if (isMinimax.value && protocolSelection.value === 'anthropic_messages') {
    hints.push(props.t('providerMinimaxAnthropicBaseUrlHint'))
  }
  if (usesCustomMinimaxBaseUrl.value) {
    hints.push(props.t('providerCustomBaseUrlHint'))
  }
  return hints.join(' ')
})

function activeMinimaxProtocol(): MinimaxProtocol {
  return form.value.protocol_mode === 'manual' &&
    form.value.native_api_override === 'anthropic_messages'
    ? 'anthropic'
    : 'openai'
}

function setMinimaxBaseUrl(
  region: 'cn' | 'global',
  protocol: MinimaxProtocol,
): void {
  const current = form.value.base_url.trim().replace(/\/+$/, '')
  const known = Object.values(minimaxBaseUrls).flatMap((urls) =>
    Object.values(urls),
  )
  if (!current || known.includes(current as (typeof known)[number])) {
    form.value.base_url = minimaxBaseUrls[region][protocol]
  }
}
</script>

<template>
  <div class="grid gap-3 md:grid-cols-[8rem_12rem_minmax(0,1fr)_12rem]">
    <USelect v-model="form.scope" class="w-full" :items="['admin', 'user']" />
    <USelect
      v-model="providerSelection"
      class="w-full"
      :items="[
        { label: t('providerGeneric'), value: 'generic' },
        { label: t('providerMinimax'), value: 'minimax' },
      ]"
      label-key="label"
      value-key="value"
    />
    <UInput v-model="form.name" class="w-full" :placeholder="t('name')" />
    <USelect
      v-model="protocolSelection"
      class="w-full"
      :items="[
        { label: t('endpointSourceAuto'), value: 'auto' },
        {
          label: t('nativeApiAnthropicMessages'),
          value: 'anthropic_messages',
        },
        { label: t('nativeApiChat'), value: 'chat' },
        { label: t('nativeApiResponses'), value: 'responses' },
        { label: t('nativeApiRealtime'), value: 'realtime' },
      ]"
      label-key="label"
      value-key="value"
    />
  </div>
  <div v-if="isMinimax" class="grid gap-1 md:grid-cols-[8rem_minmax(0,1fr)]">
    <label class="flex items-center text-xs text-muted">
      {{ t('providerRegion') }}
    </label>
    <USelect
      v-model="providerRegionSelection"
      class="w-full"
      :items="[
        { label: t('providerRegionCn'), value: 'cn' },
        { label: t('providerRegionGlobal'), value: 'global' },
      ]"
      label-key="label"
      value-key="value"
    />
  </div>
  <div v-if="isMinimax" class="grid gap-1 md:grid-cols-[8rem_minmax(0,1fr)]">
    <div class="flex items-center gap-1">
      <label class="text-xs text-muted" for="endpoint-service-tier">
        {{ t('serviceTier') }}
      </label>
      <UTooltip :text="t('serviceTierHint')">
        <UButton
          type="button"
          size="xs"
          color="neutral"
          variant="ghost"
          icon="i-lucide-info"
          :aria-label="t('serviceTierHint')"
        />
      </UTooltip>
    </div>
    <USelect
      id="endpoint-service-tier"
      v-model="serviceTierSelection"
      class="w-full"
      :items="serviceTierOptions"
      label-key="label"
      value-key="value"
    />
    <p class="text-xs leading-snug text-muted md:col-start-2">
      {{ t('serviceTierHint') }}
    </p>
  </div>
  <div class="grid gap-1 md:grid-cols-[9rem_minmax(0,1fr)] md:items-center">
    <div class="flex items-center gap-1">
      <label class="text-xs text-muted" for="endpoint-base-url">
        {{ t('baseUrl') }}
      </label>
      <UTooltip :text="baseUrlHintText">
        <UButton
          type="button"
          size="xs"
          color="neutral"
          variant="ghost"
          icon="i-lucide-info"
          :aria-label="baseUrlHintText"
        />
      </UTooltip>
    </div>
    <UInput
      id="endpoint-base-url"
      v-model="form.base_url"
      class="w-full"
      :placeholder="t('baseUrl')"
    />
    <p
      v-if="hasVersionPath"
      class="text-xs leading-snug text-warning md:col-start-2"
    >
      {{ t('baseUrlVersionWarning') }}
    </p>
  </div>
</template>
