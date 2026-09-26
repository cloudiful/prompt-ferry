<script setup lang="ts">
import { computed } from 'vue'
import ProviderIcon from '@/components/providers/ProviderIcon.vue'
import { normalizeProviderPlan } from '@/admin-mappers'
import type { EndpointPlan } from '@/generated/admin-api'
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
      | 'glm'
      | 'deepseek'
      | 'openai',
  ) {
    form.value.provider = value
    // Issue #599 R2c: the plan axis is OpenAI-only; a provider switch away
    // resets it so a stale subscription selection is never submitted.
    form.value.plan = normalizeProviderPlan(value, form.value.plan)
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
const isOpenAi = computed(() => form.value.provider === 'openai')
// Issue #599 R2c: the ChatGPT subscription plan is claimable only after the
// OAuth login stored a token; the server rejects it otherwise.
const planSelection = computed<EndpointPlan>({
  get: () => normalizeProviderPlan(form.value.provider, form.value.plan),
  set(value) {
    form.value.plan = normalizeProviderPlan(form.value.provider, value)
  },
})
const planOptions = computed(() => [
  { label: props.t('endpointPlanPlatformApiKey'), value: 'platform_api_key' },
  {
    label: props.t('endpointPlanChatgptSubscription'),
    value: 'chatgpt_subscription',
    disabled: !form.value.has_oauth_token,
  },
])
const planHint = computed(() =>
  form.value.has_oauth_token
    ? props.t('endpointPlanHint')
    : props.t('endpointPlanLoginRequired'),
)
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
        { label: t('providerDeepSeek'), value: 'deepseek' },
        { label: t('providerOpenAi'), value: 'openai' },
      ]"
      label-key="label"
      value-key="value"
    >
      <template #item-leading="{ item }">
        <ProviderIcon :provider="item.value" size="sm" />
      </template>
    </USelect>
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
  <div
    v-if="isOpenAi"
    class="grid gap-1 md:grid-cols-[9rem_minmax(0,1fr)] md:items-center"
  >
    <div class="flex items-center gap-1">
      <label class="text-xs text-muted" for="endpoint-plan">
        {{ t('endpointPlan') }}
      </label>
      <UTooltip :text="t('endpointPlanHint')">
        <UButton
          type="button"
          size="xs"
          color="neutral"
          variant="ghost"
          icon="i-lucide-info"
          :aria-label="t('endpointPlanHint')"
        />
      </UTooltip>
    </div>
    <USelect
      id="endpoint-plan"
      v-model="planSelection"
      class="w-full"
      :items="planOptions"
      label-key="label"
      value-key="value"
    />
    <p
      v-if="!form.has_oauth_token"
      class="text-xs leading-snug text-warning md:col-start-2"
    >
      {{ planHint }}
    </p>
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
