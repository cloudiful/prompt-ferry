<script setup lang="ts">
import { computed } from 'vue'
import type { EndpointForm } from '@/models'

const props = defineProps<{
  t: TranslateFn
}>()

const form = defineModel<EndpointForm>('form', { required: true })

const providerSelection = computed({
  get: () => form.value.provider,
  set(
    value:
      | 'generic'
      | 'minimax'
      | 'command_code'
      | 'opencode_go'
      | 'openrouter'
      | 'glm',
  ) {
    form.value.provider = value
    if (value !== 'minimax') {
      // Preset providers other than MiniMax carry no region, no service
      // tier, and no MiniMax builtin MCP privilege. Their base URL is
      // derived server-side, so the form no longer tracks one.
      form.value.provider_region = null
      form.value.service_tier = 'standard'
      form.value.mcp_enabled = false
      return
    }
    form.value.provider_region = form.value.provider_region ?? 'cn'
    // Preserve an explicit priority selection; normalize legacy/unknown
    // values to the standard default.
    form.value.service_tier =
      form.value.service_tier === 'priority' ? 'priority' : 'standard'
    if (!form.value.endpoint_id) {
      form.value.mcp_enabled = true
    }
  },
})
const providerRegionSelection = computed({
  get: () => form.value.provider_region ?? 'cn',
  set(value: 'cn' | 'global') {
    form.value.provider_region = value
  },
})
const isGeneric = computed(() => form.value.provider === 'generic')
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
      return
    }
    form.value.protocol_mode = 'manual'
    form.value.native_api_override = value
  },
})
const hasVersionPath = computed(() =>
  /\/v1\/?$/.test(form.value.base_url.trim()),
)
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
        { label: t('providerGlm'), value: 'glm' },
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
  <div
    v-if="isGeneric"
    class="grid gap-1 md:grid-cols-[9rem_minmax(0,1fr)] md:items-center"
  >
    <div class="flex items-center gap-1">
      <label class="text-xs text-muted" for="endpoint-base-url">
        {{ t('baseUrl') }}
      </label>
      <UTooltip :text="t('baseUrlHint')">
        <UButton
          type="button"
          size="xs"
          color="neutral"
          variant="ghost"
          icon="i-lucide-info"
          :aria-label="t('baseUrlHint')"
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
