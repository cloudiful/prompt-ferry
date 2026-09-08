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
const COMMAND_CODE_BASE_URL = 'https://api.commandcode.ai/provider' as const
const OPENCODE_GO_BASE_URL = 'https://opencode.ai/zen/go' as const
const OPENROUTER_BASE_URL = 'https://openrouter.ai/api' as const
function stripVersionSuffix(value: string): string {
  let normalized = value.trim()
  for (;;) {
    const withoutSlash = normalized.replace(/\/+$/, '')
    if (withoutSlash.endsWith('/v1')) {
      normalized = withoutSlash.slice(0, -3)
      continue
    }
    normalized = withoutSlash
    break
  }
  return normalized
}
function sanitizeBaseUrlField(): void {
  const sanitized = stripVersionSuffix(form.value.base_url)
  if (sanitized !== form.value.base_url) {
    form.value.base_url = sanitized
  }
}
const hasVersionPath = computed(() =>
  /\/v1\/?$/.test(form.value.base_url.trim()),
)
const providerSelection = computed({
  get: () => form.value.provider,
  set(
    value:
      'generic' | 'minimax' | 'command_code' | 'opencode_go' | 'openrouter',
  ) {
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
    if (value === 'command_code') {
      // CommandCode carries no region (NULL); hide the region control and
      // keep service tier/MCP at generic defaults. Inference accepts both
      // Anthropic Messages and Chat on the same provider-compatible base.
      form.value.provider_region = null
      form.value.service_tier = 'standard'
      form.value.mcp_enabled = false
      setCommandCodeBaseUrl()
      return
    }
    if (value === 'opencode_go') {
      // OpencodeGo carries no region (NULL); hide the region control and
      // keep service tier/MCP at generic defaults. Inference goes through
      // the official Zen /v1 base.
      form.value.provider_region = null
      form.value.service_tier = 'standard'
      form.value.mcp_enabled = false
      setOpencodeGoBaseUrl()
      return
    }
    if (value === 'openrouter') {
      // OpenRouter carries no region (NULL); hide the region control and
      // keep service tier/MCP at generic defaults. Inference goes through
      // the official /api base (stored without the /v1 suffix).
      form.value.provider_region = null
      form.value.service_tier = 'standard'
      form.value.mcp_enabled = false
      setOpenRouterBaseUrl()
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
const isCommandCode = computed(() => form.value.provider === 'command_code')
const isOpencodeGo = computed(() => form.value.provider === 'opencode_go')
const isOpenRouter = computed(() => form.value.provider === 'openrouter')
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
  const current = stripVersionSuffix(form.value.base_url)
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
  if (isCommandCode.value) {
    hints.push(props.t('providerCommandCodeBaseUrlHint'))
  }
  if (isOpencodeGo.value) {
    hints.push(props.t('providerOpencodeGoBaseUrlHint'))
  }
  if (isOpenRouter.value) {
    hints.push(props.t('providerOpenRouterBaseUrlHint'))
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
  const current = stripVersionSuffix(form.value.base_url)
  const known = Object.values(minimaxBaseUrls).flatMap((urls) =>
    Object.values(urls),
  )
  if (!current || known.includes(current as (typeof known)[number])) {
    form.value.base_url = minimaxBaseUrls[region][protocol]
  }
}

function setCommandCodeBaseUrl(): void {
  const current = stripVersionSuffix(form.value.base_url)
  if (!current) {
    form.value.base_url = COMMAND_CODE_BASE_URL
    return
  }
  const knownMinimax = Object.values(minimaxBaseUrls).flatMap((urls) =>
    Object.values(urls).map((url) => stripVersionSuffix(url)),
  )
  const commandCode = stripVersionSuffix(COMMAND_CODE_BASE_URL)
  if (knownMinimax.includes(current) || current === commandCode) {
    form.value.base_url = COMMAND_CODE_BASE_URL
  }
}

function setOpencodeGoBaseUrl(): void {
  const current = stripVersionSuffix(form.value.base_url)
  if (!current) {
    form.value.base_url = OPENCODE_GO_BASE_URL
    return
  }
  const knownMinimax = Object.values(minimaxBaseUrls).flatMap((urls) =>
    Object.values(urls).map((url) => stripVersionSuffix(url)),
  )
  const commandCode = stripVersionSuffix(COMMAND_CODE_BASE_URL)
  const opencodeGo = stripVersionSuffix(OPENCODE_GO_BASE_URL)
  if (
    knownMinimax.includes(current) ||
    current === commandCode ||
    current === opencodeGo
  ) {
    form.value.base_url = OPENCODE_GO_BASE_URL
  }
}

function setOpenRouterBaseUrl(): void {
  const current = stripVersionSuffix(form.value.base_url)
  if (!current) {
    form.value.base_url = OPENROUTER_BASE_URL
    return
  }
  const knownMinimax = Object.values(minimaxBaseUrls).flatMap((urls) =>
    Object.values(urls).map((url) => stripVersionSuffix(url)),
  )
  const commandCode = stripVersionSuffix(COMMAND_CODE_BASE_URL)
  const opencodeGo = stripVersionSuffix(OPENCODE_GO_BASE_URL)
  const openRouter = stripVersionSuffix(OPENROUTER_BASE_URL)
  if (
    knownMinimax.includes(current) ||
    current === commandCode ||
    current === opencodeGo ||
    current === openRouter
  ) {
    form.value.base_url = OPENROUTER_BASE_URL
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
        { label: t('providerCommandCode'), value: 'command_code' },
        { label: t('providerOpencodeGo'), value: 'opencode_go' },
        { label: t('providerOpenRouter'), value: 'openrouter' },
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
  <div v-if="isMinimax" class="grid gap-3 md:grid-cols-2">
    <div class="grid gap-1 md:grid-cols-[8rem_minmax(0,1fr)] md:items-center">
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
    <div class="grid gap-1 md:grid-cols-[8rem_minmax(0,1fr)] md:items-center">
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
    </div>
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
      @blur="sanitizeBaseUrlField"
    />
    <p
      v-if="hasVersionPath"
      class="text-xs leading-snug text-warning md:col-start-2"
    >
      {{ t('baseUrlVersionWarning') }}
    </p>
  </div>
</template>
