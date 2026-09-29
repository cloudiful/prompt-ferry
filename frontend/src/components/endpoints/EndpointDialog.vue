<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { normalizeProviderPlan } from '@/admin-mappers'
import { useEndpointDialogValidation } from '@/composables/useEndpointDialogValidation'
import type { User } from '@/generated/admin-api'
import type { EndpointForm } from '@/models'
import EndpointAdminApiKeyFields from '@/components/endpoints/EndpointAdminApiKeyFields.vue'
import EndpointApiKeysEditor from '@/components/endpoints/EndpointApiKeysEditor.vue'
import EndpointOAuthSection from '@/components/endpoints/EndpointOAuthSection.vue'
import EndpointOrganizationUsage from '@/components/endpoints/EndpointOrganizationUsage.vue'
import EndpointProviderFields from '@/components/endpoints/EndpointProviderFields.vue'
import ServiceTierOverrideField from '@/components/shared/ServiceTierOverrideField.vue'
import ProxySettingsFields from '@/components/shared/ProxySettingsFields.vue'
import ScheduleWindowsFields from '@/components/shared/ScheduleWindowsFields.vue'
import SettingsFieldRow from '@/components/shared/SettingsFieldRow.vue'
import {
  endpointFormProtocol,
  supportsServiceTierFor,
} from '@/models/endpoints/service-tier'

const props = defineProps<{
  busy: boolean
  header: string
  t: TranslateFn
  users: User[]
}>()

const visible = defineModel<boolean>('visible', { required: true })
const form = defineModel<EndpointForm>('form', { required: true })

defineEmits<{
  save: []
}>()

// Whole-dialog two-level drill: main form plus settings sub-page.
// Gear swaps the entire modal content; back returns to the main form.
// Draft lives in `form` so edits persist across view switches and save
// through the existing outer save button.
const view = ref<'main' | 'settings'>('main')

function openSettings(): void {
  view.value = 'settings'
}

function backToMain(): void {
  view.value = 'main'
}

watch(visible, (open) => {
  if (!open) view.value = 'main'
})

// Issue #589 P2c: proxy/schedule validation moved to a composable so this
// dialog stays a thin shell; the outer save gates on the same rules.
const { canSaveOuter, hasEndpointSettings: hasBaseEndpointSettings } =
  useEndpointDialogValidation(form, props.t)

// Issue #644: the free-form service-tier override is provider-agnostic and
// lives in the settings subpage; Realtime uses WebSocket frames instead of the
// common JSON request body, so it is the only protocol that stays hidden.
const serviceTierEligible = computed(() =>
  form.value ? supportsServiceTierFor(endpointFormProtocol(form.value)) : true,
)

// Issue #644: a configured tier lights the settings gear even when proxy and
// schedule are untouched; the composable only covers proxy/schedule.
function hasEndpointServiceTier(): boolean {
  return (form.value?.service_tier ?? '').trim() !== ''
}

// Gear highlight when anything is non-default.
function hasEndpointSettings(): boolean {
  return hasBaseEndpointSettings() || hasEndpointServiceTier()
}

// Issue #599 R2e.1: the subscription plan authenticates through the ChatGPT
// OAuth login, so its API-key editor stays hidden. The provider gate keeps a
// stale plan from hiding the editor off OpenAI.
const isChatgptSubscription = computed(
  () =>
    normalizeProviderPlan(
      form.value?.provider ?? 'generic',
      form.value?.plan,
    ) === 'chatgpt_subscription',
)

// Issue #599 R2c: mirror the live OAuth state into the form. A token that
// appears (login completed) enables the subscription plan; a token that
// disappears (cleared/revoked) forces the platform plan back.
function applyOAuthStatus(next: {
  plan: EndpointForm['plan']
  has_oauth_token: boolean
}): void {
  const hadToken = form.value?.has_oauth_token ?? false
  if (!form.value) return
  form.value.has_oauth_token = next.has_oauth_token
  if (!next.has_oauth_token) {
    form.value.plan = 'platform_api_key'
    return
  }
  if (!hadToken) form.value.plan = next.plan
}
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="header"
    :ui="{ content: 'sm:max-w-4xl' }"
  >
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('save')">
        <template v-if="view === 'main'">
          <EndpointProviderFields v-model:form="form" :t="t" />
          <USelect
            v-if="form.scope === 'user'"
            :model-value="form.owner_user_id ?? undefined"
            class="w-full"
            :items="users"
            label-key="login_name"
            value-key="user_id"
            :placeholder="t('ownerUser')"
            @update:model-value="form.owner_user_id = $event ?? null"
          />
          <EndpointApiKeysEditor
            v-if="!isChatgptSubscription"
            v-model:form="form"
            :t="t"
          />
          <div
            v-if="form.provider === 'openai'"
            class="grid gap-3 border-t border-default pt-3"
          >
            <EndpointAdminApiKeyFields v-model:form="form" :t="t" />
            <EndpointOrganizationUsage
              :endpoint-id="form.endpoint_id"
              :provider="form.provider"
              :has-admin-api-key="form.has_admin_api_key"
              :t="t"
            />
          </div>
          <div
            v-if="form.provider === 'openai'"
            class="border-t border-default pt-3"
          >
            <EndpointOAuthSection
              v-if="form.endpoint_id"
              :endpoint-id="form.endpoint_id"
              :t="t"
              @status="applyOAuthStatus"
            />
            <p v-else class="text-xs text-dimmed">
              {{ t('endpointOAuthSaveFirst') }}
            </p>
          </div>
          <div class="grid gap-2 border-t border-default pt-3">
            <div class="flex items-center justify-between gap-3">
              <div class="flex items-center gap-1">
                <span class="text-xs font-medium text-default">
                  {{ t('endpointSettings') }}
                </span>
                <UTooltip :text="t('endpointSettingsHint')">
                  <UButton
                    type="button"
                    size="xs"
                    color="neutral"
                    variant="ghost"
                    icon="i-lucide-info"
                    :aria-label="t('endpointSettingsHint')"
                  />
                </UTooltip>
              </div>
              <UButton
                type="button"
                size="sm"
                :color="hasEndpointSettings() ? 'primary' : 'neutral'"
                variant="ghost"
                icon="i-lucide-settings-2"
                :aria-label="t('endpointSettings')"
                :aria-pressed="hasEndpointSettings()"
                :title="t('endpointSettingsHint')"
                @click="openSettings"
              />
            </div>
          </div>
          <div
            v-if="form.provider === 'minimax'"
            class="flex items-center justify-between gap-3 border-t border-default pt-3"
          >
            <div class="flex items-center gap-1">
              <label
                for="endpoint-minimax-mcp"
                class="text-xs font-medium text-default"
              >
                {{ t('minimaxMcp') }}
              </label>
              <UTooltip :text="t('minimaxMcpHint')">
                <UButton
                  type="button"
                  size="xs"
                  color="neutral"
                  variant="ghost"
                  icon="i-lucide-info"
                  :aria-label="t('minimaxMcpHint')"
                />
              </UTooltip>
            </div>
            <USwitch id="endpoint-minimax-mcp" v-model="form.mcp_enabled" />
          </div>
        </template>
        <template v-else>
          <div class="flex items-center gap-1">
            <UButton
              type="button"
              size="xs"
              color="neutral"
              variant="ghost"
              icon="i-lucide-arrow-left"
              :aria-label="t('cancel')"
              @click="backToMain"
            />
            <span class="text-sm font-medium text-default">{{
              t('endpointSettings')
            }}</span>
          </div>
          <div class="grid gap-3 rounded border border-default bg-muted p-3">
            <SettingsFieldRow :label="t('proxyUrl')" :hint="t('proxyUrlHint')">
              <ProxySettingsFields
                v-model:proxy-url="form.proxy_url"
                v-model:has-saved="form.has_saved_proxy_url"
                :t="t"
              />
            </SettingsFieldRow>
            <SettingsFieldRow
              :label="t('scheduleWindows')"
              :hint="t('scheduleWindowsHint')"
            >
              <ScheduleWindowsFields
                v-model:windows="form.active_windows"
                v-model:touched="form.active_windows_touched"
                :t="t"
              />
            </SettingsFieldRow>
            <ServiceTierOverrideField
              v-if="serviceTierEligible"
              v-model="form.service_tier"
              :t="t"
              input-id="endpoint-service-tier"
            />
          </div>
        </template>
        <div class="flex justify-end gap-2 pt-1">
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
            size="sm"
            :loading="busy"
            :disabled="!canSaveOuter"
            ><UIcon name="i-lucide-save" class="h-4 w-4" />{{
              t('saveEndpoint')
            }}</UButton
          >
        </div>
      </form>
    </template>
  </UModal>
</template>
