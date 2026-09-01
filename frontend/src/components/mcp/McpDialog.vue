<script setup lang="ts">
import { computed, watch } from 'vue'
import type {
  McpCatalogResponse,
  McpQuotaGroup,
  User,
} from '@/generated/admin-api'
import type { McpForm } from '@/models'
import McpBearerTokensEditor from '@/components/mcp/McpBearerTokensEditor.vue'
import McpCredentialQuotaEditor from '@/components/mcp/McpCredentialQuotaEditor.vue'
import McpEnvironmentEditor from '@/components/mcp/McpEnvironmentEditor.vue'
import RequestLimitFields from '@/components/shared/RequestLimitFields.vue'

const props = defineProps<{
  busy: boolean
  catalogLoading: boolean
  header: string
  isAdmin: boolean
  t: TranslateFn
  catalog: McpCatalogResponse
  users: User[]
  quotaGroups: McpQuotaGroup[]
  learned?: {
    mode: string | null
    protocolVersion: string | null
    learnedAt: string | null
  } | null
}>()

const visible = defineModel<boolean>('visible', { required: true })
const form = defineModel<McpForm>('form', { required: true })

const settingsSectionClass =
  'grid gap-3 rounded border border-default bg-muted p-3'

const authModeItems = computed(() => [
  { label: props.t('authModeNone'), value: 'none' },
  { label: props.t('authModeBearer'), value: 'bearer' },
  { label: props.t('authModeBasic'), value: 'basic' },
])

const protocolVersionItems = computed(() => [
  { label: props.t('lifecycleManualProtocolVersionAuto'), value: '' },
  { label: '2026-07-28', value: '2026-07-28' },
  { label: '2025-11-25', value: '2025-11-25' },
  { label: '2025-06-18', value: '2025-06-18' },
  { label: '2025-03-26', value: '2025-03-26' },
  { label: '2024-11-05', value: '2024-11-05' },
])

const protocolVersionModel = computed<string>({
  get: () => form.value.lifecycle_manual_protocol_version ?? '',
  set: (value) => {
    form.value.lifecycle_manual_protocol_version = value ? value : null
  },
})

const transportSelection = computed<'http' | 'stdio' | 'builtin_minimax'>({
  get: () => form.value.transport,
  set(value) {
    form.value.transport = value
    // The `builtin_minimax` transport is only meaningful for managed rows
    // tied to a MiniMax source endpoint. Clearing the binding on switch
    // prevents the backend from re-creating the managed server for a row
    // that has been reconfigured as a plain http/stdio server.
    if (value !== 'builtin_minimax' && form.value.source_endpoint_id) {
      form.value.source_endpoint_id = null
    }
  },
})

// Defensive fallback: if the form is mutated elsewhere (reset, deep clone,
// template hydration) and ends up with a non-managed transport while still
// pointing at a source endpoint, drop the stale binding too.
watch(
  () => [form.value.transport, form.value.source_endpoint_id] as const,
  ([transport, sourceEndpointId]) => {
    if (
      transport !== 'builtin_minimax' &&
      sourceEndpointId !== null &&
      sourceEndpointId !== undefined
    ) {
      form.value.source_endpoint_id = null
    }
  },
)

const selectedTools = computed<string[]>({
  get: () =>
    form.value.tool_filter_mode === 'whitelist'
      ? form.value.allowed_tools
      : form.value.disabled_tools,
  set: (value) => {
    if (form.value.tool_filter_mode === 'whitelist') {
      form.value.allowed_tools = value
      return
    }
    form.value.disabled_tools = value
  },
})

const toolSelectionLabel = computed(() =>
  form.value.tool_filter_mode === 'whitelist'
    ? 'allowedTools'
    : 'disabledTools',
)
const toolSelectionPlaceholder = computed(() =>
  form.value.tool_filter_mode === 'whitelist'
    ? 'allowedToolsPlaceholder'
    : 'disabledToolsPlaceholder',
)
const hasCatalogPreview = computed(
  () =>
    props.catalog.tools.length > 0 ||
    props.catalog.resources.length > 0 ||
    props.catalog.prompts.length > 0,
)
const toolCatalogNeedsFilter = computed(() => props.catalog.tools.length > 8)
const resourceCatalogNeedsFilter = computed(
  () => props.catalog.resources.length > 8,
)
const toolOptions = computed(() =>
  props.catalog.tools.map((item) => ({
    ...item,
    description: item.description ?? undefined,
  })),
)
const resourceOptions = computed(() =>
  props.catalog.resources.map((item) => ({
    ...item,
    description: item.description ?? undefined,
  })),
)

defineEmits<{
  save: []
}>()
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="header"
    :ui="{
      content: 'sm:max-w-3xl',
      body: 'min-h-0 max-h-[80vh] overflow-hidden',
    }"
  >
    <template #body>
      <div
        class="grid max-h-[calc(90vh-7rem)] grid-rows-[minmax(0,1fr)_auto] gap-3 text-xs"
      >
        <form
          id="mcp-server-form"
          class="grid min-h-0 gap-3 overflow-x-hidden overflow-y-auto pr-1"
          @submit.prevent="$emit('save')"
        >
          <div
            v-if="isAdmin"
            class="grid gap-3 md:grid-cols-[10rem_minmax(0,1fr)_minmax(0,1fr)]"
          >
            <USelect
              v-model="form.scope"
              class="w-full"
              :items="['admin', 'user']"
            />
            <UInput
              v-model="form.name"
              class="w-full"
              :placeholder="t('name')"
            />
            <USelect
              v-model="form.aggregate_naming_mode"
              class="w-full"
              :items="[
                {
                  label: t('aggregateNamingModePassthroughPreferred'),
                  value: 'passthrough_preferred',
                },
                {
                  label: t('aggregateNamingModeQualifiedOnly'),
                  value: 'qualified_only',
                },
              ]"
              label-key="label"
              value-key="value"
              :disabled="Boolean(form.source_endpoint_id)"
            />
          </div>
          <USelect
            v-if="isAdmin && form.scope === 'user'"
            :model-value="form.owner_user_id ?? undefined"
            class="w-full"
            :items="users"
            label-key="login_name"
            value-key="user_id"
            :placeholder="t('ownerUser')"
            @update:model-value="form.owner_user_id = $event ?? null"
          />
          <div v-else-if="!isAdmin" class="grid gap-3 md:grid-cols-2">
            <UInput
              v-model="form.name"
              class="w-full"
              :placeholder="t('name')"
            />
            <USelect
              v-model="form.aggregate_naming_mode"
              class="w-full"
              :items="[
                {
                  label: t('aggregateNamingModePassthroughPreferred'),
                  value: 'passthrough_preferred',
                },
                {
                  label: t('aggregateNamingModeQualifiedOnly'),
                  value: 'qualified_only',
                },
              ]"
              label-key="label"
              value-key="value"
            />
          </div>
          <div class="grid gap-3 md:grid-cols-[10rem_minmax(0,1fr)]">
            <div class="flex min-w-0 items-center gap-1">
              <USelect
                v-model="transportSelection"
                class="min-w-0 flex-1"
                :items="[
                  { label: 'HTTP MCP', value: 'http' },
                  { label: 'stdio', value: 'stdio' },
                  ...(form.source_endpoint_id
                    ? [{ label: t('minimaxManaged'), value: 'builtin_minimax' }]
                    : []),
                ]"
                label-key="label"
                value-key="value"
              />
              <UTooltip
                v-if="form.transport === 'builtin_minimax'"
                :text="t('minimaxManagedHint')"
              >
                <UButton
                  type="button"
                  size="xs"
                  color="neutral"
                  variant="ghost"
                  icon="i-lucide-info"
                  :aria-label="t('minimaxManagedHint')"
                />
              </UTooltip>
            </div>
            <div
              v-if="form.transport === 'http'"
              class="flex min-w-0 items-center gap-1"
            >
              <UInput
                v-model="form.url"
                class="min-w-0 flex-1"
                placeholder="http://127.0.0.1:3000/mcp"
              />
            </div>
            <div
              v-else-if="form.transport === 'stdio'"
              class="flex min-w-0 items-center gap-1"
            >
              <UInput
                v-model="form.command_argv_text"
                class="min-w-0 flex-1"
                placeholder='["uvx", "minimax-coding-plan-mcp", "-y"]'
              />
              <UTooltip :text="t('stdioCommandHint')">
                <UButton
                  type="button"
                  size="xs"
                  color="neutral"
                  variant="ghost"
                  icon="i-lucide-info"
                  :aria-label="t('stdioCommandHint')"
                />
              </UTooltip>
            </div>
          </div>
          <div
            v-if="form.transport === 'http'"
            class="grid min-w-0 gap-2"
          >
            <div class="text-muted">{{ t('authMode') }}</div>
            <USelect
              v-model="form.auth_mode"
              class="w-full"
              :items="authModeItems"
              label-key="label"
              value-key="value"
            />
          </div>
          <McpBearerTokensEditor
            v-if="form.transport === 'http' && form.auth_mode === 'bearer'"
            v-model:tokens="form.bearer_tokens"
            :t="t"
          />
          <McpCredentialQuotaEditor
            v-if="
              form.transport === 'http' &&
              form.auth_mode === 'bearer' &&
              isAdmin
            "
            :server-id="form.server_id"
            :quota-groups="quotaGroups"
            :t="t"
          />
          <div
            v-if="form.transport === 'http' && form.auth_mode === 'basic'"
            class="grid gap-3 md:grid-cols-2"
          >
            <UInput
              v-model="form.basic_username"
              class="w-full"
              :placeholder="t('basicUsernamePlaceholder')"
            />
            <UInput
              v-model="form.basic_password"
              type="password"
              class="w-full"
              :placeholder="
                form.has_basic_password
                  ? t('savedSecret')
                  : t('basicPasswordPlaceholder')
              "
            />
          </div>
          <McpEnvironmentEditor
            v-if="form.transport === 'stdio'"
            v-model:variables="form.environment_variables"
            :t="t"
          />
          <RequestLimitFields
            v-model:form="form"
            daily-label="dailyCallLimit"
            monthly-label="monthlyCallLimit"
            :t="t"
          />
          <div :class="settingsSectionClass">
            <div class="grid gap-3 md:grid-cols-[repeat(3,minmax(0,1fr))]">
              <div class="grid min-w-0 gap-2">
                <div class="text-muted">{{ t('toolFilterMode') }}</div>
                <USelect
                  v-model="form.tool_filter_mode"
                  class="w-full"
                  :items="[
                    { label: t('toolFilterModeBlacklist'), value: 'blacklist' },
                    { label: t('toolFilterModeWhitelist'), value: 'whitelist' },
                  ]"
                  label-key="label"
                  value-key="value"
                />
              </div>
              <div class="grid min-w-0 gap-2">
                <div class="flex items-center gap-1 text-muted">
                  <span>{{ t('lifecyclePolicy') }}</span>
                  <UTooltip :text="t('lifecyclePolicyHint')">
                    <UButton
                      type="button"
                      size="xs"
                      color="neutral"
                      variant="ghost"
                      icon="i-lucide-info"
                      :aria-label="t('lifecyclePolicyHint')"
                    />
                  </UTooltip>
                </div>
                <USelect
                  v-model="form.lifecycle_policy"
                  class="w-full"
                  :items="[
                    { label: t('lifecyclePolicyAuto'), value: 'auto' },
                    {
                      label: t('lifecyclePolicyLegacy'),
                      value: 'legacy_initialize',
                    },
                  ]"
                  label-key="label"
                  value-key="value"
                />
              </div>
              <div class="grid min-w-0 gap-2">
                <div class="flex items-center gap-1 text-muted">
                  <span>{{ t('lifecycleManualProtocolVersion') }}</span>
                  <UTooltip :text="t('lifecycleManualProtocolVersionHint')">
                    <UButton
                      type="button"
                      size="xs"
                      color="neutral"
                      variant="ghost"
                      icon="i-lucide-info"
                      :aria-label="t('lifecycleManualProtocolVersionHint')"
                    />
                  </UTooltip>
                </div>
                <USelect
                  v-model="protocolVersionModel"
                  class="w-full font-mono"
                  :items="protocolVersionItems"
                  label-key="label"
                  value-key="value"
                />
              </div>
            </div>
            <div class="grid gap-3 md:grid-cols-[repeat(3,minmax(0,1fr))]">
              <div class="grid min-w-0 gap-2">
                <div class="text-muted">{{ t(toolSelectionLabel) }}</div>
                <div
                  v-if="catalogLoading"
                  class="flex min-h-11 items-center gap-2 rounded border border-default bg-default px-3 text-xs text-dimmed"
                >
                  <UIcon
                    name="i-lucide-loader-circle"
                    class="size-4 animate-spin"
                  />
                  <span>{{ t('loadingTools') }}</span>
                </div>
                <USelectMenu
                  v-else
                  v-model="selectedTools"
                  :items="toolOptions"
                  label-key="name"
                  value-key="name"
                  multiple
                  :search-input="toolCatalogNeedsFilter"
                  :placeholder="t(toolSelectionPlaceholder)"
                  class="w-52 max-w-full min-w-0"
                  :ui="{ value: 'block max-w-full truncate' }"
                />
              </div>
              <div class="grid min-w-0 gap-2">
                <div class="flex items-center gap-1 text-muted">
                  <span>{{ t('disabledResources') }}</span>
                  <UTooltip :text="t('disabledResourcesHint')">
                    <UButton
                      type="button"
                      size="xs"
                      color="neutral"
                      variant="ghost"
                      icon="i-lucide-info"
                      :aria-label="t('disabledResourcesHint')"
                    />
                  </UTooltip>
                </div>
                <USelectMenu
                  v-model="form.disabled_resources"
                  :items="resourceOptions"
                  label-key="name"
                  value-key="name"
                  multiple
                  :search-input="resourceCatalogNeedsFilter"
                  :placeholder="t('disabledResourcesPlaceholder')"
                  class="w-52 max-w-full min-w-0"
                  :ui="{ value: 'block max-w-full truncate' }"
                />
              </div>
            </div>
            <div
              v-if="hasCatalogPreview"
              class="grid gap-3 rounded border border-default bg-default p-3"
            >
              <div class="text-muted">{{ t('aggregatePreview') }}</div>
              <div v-if="catalog.tools.length" class="grid gap-1">
                <div class="text-xs font-medium text-highlighted">
                  {{ t('tools') }}
                </div>
                <div
                  v-for="item in catalog.tools"
                  :key="`tool-${item.name}`"
                  class="text-xs text-dimmed"
                >
                  <span class="font-mono text-highlighted">{{
                    item.name
                  }}</span>
                  <span> -> </span>
                  <span class="font-mono">{{
                    item.aggregate_names.join(', ')
                  }}</span>
                </div>
              </div>
              <div v-if="catalog.resources.length" class="grid gap-1">
                <div class="text-xs font-medium text-highlighted">
                  {{ t('resources') }}
                </div>
                <div
                  v-for="item in catalog.resources"
                  :key="`resource-${item.name}`"
                  class="text-xs text-dimmed"
                >
                  <span class="font-mono text-highlighted">{{
                    item.name
                  }}</span>
                  <span> -> </span>
                  <span class="font-mono">{{
                    item.aggregate_names.join(', ')
                  }}</span>
                </div>
              </div>
              <div v-if="catalog.prompts.length" class="grid gap-1">
                <div class="text-xs font-medium text-highlighted">
                  {{ t('prompts') }}
                </div>
                <div
                  v-for="item in catalog.prompts"
                  :key="`prompt-${item.name}`"
                  class="text-xs text-dimmed"
                >
                  <span class="font-mono text-highlighted">{{
                    item.name
                  }}</span>
                  <span> -> </span>
                  <span class="font-mono">{{
                    item.aggregate_names.join(', ')
                  }}</span>
                </div>
              </div>
            </div>
          </div>
        </form>
        <div class="flex justify-end gap-2 border-t border-default pt-3">
          <UButton
            type="button"
            size="sm"
            color="neutral"
            @click="
              () => {
                visible = false
              }
            "
            >{{ t('cancel') }}</UButton
          >
          <UButton
            type="submit"
            form="mcp-server-form"
            size="sm"
            :loading="busy"
            ><UIcon name="i-lucide-save" class="h-4 w-4" />{{
              t('save')
            }}</UButton
          >
        </div>
      </div>
    </template>
  </UModal>
</template>
