<script setup lang="ts">
import { computed } from 'vue'
import ProviderIcon from '@/components/providers/ProviderIcon.vue'
import SettingsFieldRow from '@/components/shared/SettingsFieldRow.vue'
import {
  clearAdminApiKeyOutsideOpenAi,
  normalizeProviderPlan,
} from '@/admin-mappers'
import type { EndpointPlan } from '@/generated/admin-api'
import type { EndpointForm } from '@/models'
import { endpointFormProtocol } from '@/models/endpoints/service-tier'

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
    // Issue #589 P2c: the Admin API Key is OpenAI-only; drop any typed value
    // on a provider switch (the server clears the stored key too).
    clearAdminApiKeyOutsideOpenAi(form.value)
    if (value !== 'minimax') {
      // Preset providers other than MiniMax carry no region and no MiniMax
      // builtin MCP privilege. Their base URL is derived server-side, so the
      // form no longer tracks one. Issue #644: a configured free-form
      // service tier is provider-agnostic, so a provider switch keeps it
      // instead of clearing a value the operator set in the settings
      // subpage.
      form.value.provider_region = null
      form.value.mcp_enabled = false
      return
    }
    form.value.provider_region = form.value.provider_region ?? 'cn'
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
const protocolSelection = computed({
  get: () => endpointFormProtocol(form.value),
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
const baseUrlHint = computed(() =>
  hasVersionPath.value
    ? `${props.t('baseUrlHint')} ${props.t('baseUrlVersionWarning')}`
    : props.t('baseUrlHint'),
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
  <SettingsFieldRow
    v-if="isOpenAi"
    :label="t('endpointPlan')"
    :hint="planHint"
    for-id="endpoint-plan"
  >
    <USelect
      id="endpoint-plan"
      v-model="planSelection"
      class="w-full"
      :items="planOptions"
      label-key="label"
      value-key="value"
    />
  </SettingsFieldRow>
  <SettingsFieldRow
    v-if="isMinimax"
    :label="t('providerRegion')"
    for-id="endpoint-provider-region"
  >
    <USelect
      id="endpoint-provider-region"
      v-model="providerRegionSelection"
      class="w-full"
      :items="[
        { label: t('providerRegionCn'), value: 'cn' },
        { label: t('providerRegionGlobal'), value: 'global' },
      ]"
      label-key="label"
      value-key="value"
    />
  </SettingsFieldRow>
  <SettingsFieldRow
    v-if="isGeneric"
    :label="t('baseUrl')"
    :hint="baseUrlHint"
    for-id="endpoint-base-url"
  >
    <UInput
      id="endpoint-base-url"
      v-model="form.base_url"
      class="w-full"
      :placeholder="t('baseUrl')"
    />
  </SettingsFieldRow>
</template>
