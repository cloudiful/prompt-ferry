<script setup lang="ts">
import { computed, ref } from 'vue'
import type { User } from '@/generated/admin-api'
import type { EndpointForm } from '@/models'
import EndpointApiKeysEditor from '@/components/endpoints/EndpointApiKeysEditor.vue'
import EndpointProviderFields from '@/components/endpoints/EndpointProviderFields.vue'
import ProxySettingsDialog from '@/components/shared/ProxySettingsDialog.vue'
import RequestLimitFields from '@/components/shared/RequestLimitFields.vue'

defineProps<{
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

const proxyModalOpen = ref(false)

// INLINE-proxy-ui-a1 BUG fix: guard legacy forms missing Phase C fields so
// the dialog always renders instead of throwing on undefined access.
const hasProxy = computed(() => {
  const typed = (form.value?.proxy_url ?? '').trim() !== ''
  const saved = form.value?.has_saved_proxy_url ?? false
  return typed || saved
})

// Issue #368 Phase C: clear the saved proxy so the next save sends `""`
// (clear to direct). Leaving the field blank while `has_saved_proxy_url`
// is true omits the key and keeps the stored value (see
// `endpointFormToRequest`).
function clearProxyUrl(): void {
  if (!form.value) return
  form.value.proxy_url = ''
  form.value.has_saved_proxy_url = false
}

function onProxySave(value: string): void {
  if (!form.value) return
  const trimmed = (value ?? '').trim()
  form.value.proxy_url = trimmed
  // Saving direct (`""`) must clear the saved flag so the next save sends
  // `""` (clear) instead of omitting the key (keep).
  if (trimmed === '') form.value.has_saved_proxy_url = false
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
        <EndpointApiKeysEditor v-model:form="form" :t="t" />
        <div
          class="flex items-center justify-between gap-3 border-t border-default pt-3"
        >
          <div class="flex items-center gap-1">
            <span class="text-xs font-medium text-default">
              {{ t('proxyUrl') }}
            </span>
            <UTooltip :text="t('proxyUrlHint')">
              <UButton
                type="button"
                size="xs"
                color="neutral"
                variant="ghost"
                icon="i-lucide-info"
                :aria-label="t('proxyUrlHint')"
              />
            </UTooltip>
            <UBadge
              v-if="form?.has_saved_proxy_url"
              :label="t('saved')"
              color="neutral"
            />
          </div>
          <UTooltip :text="t('proxyUrlHint')">
            <UButton
              type="button"
              size="sm"
              :color="hasProxy ? 'primary' : 'neutral'"
              variant="ghost"
              icon="i-lucide-globe"
              :aria-label="t('proxyUrl')"
              :aria-pressed="hasProxy"
              @click="proxyModalOpen = true"
            />
          </UTooltip>
        </div>
        <ProxySettingsDialog
          v-model:visible="proxyModalOpen"
          :initial-value="form?.proxy_url ?? ''"
          :has-saved="form?.has_saved_proxy_url ?? false"
          :hint="t('proxyUrlHint')"
          :t="t"
          @save="onProxySave"
          @clear="clearProxyUrl"
        />
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
        <RequestLimitFields
          v-model:form="form"
          daily-label="dailyRequestLimit"
          monthly-label="monthlyRequestLimit"
          :t="t"
        />
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
          <UButton type="submit" size="sm" :loading="busy"
            ><UIcon name="i-lucide-save" class="h-4 w-4" />{{
              t('saveEndpoint')
            }}</UButton
          >
        </div>
      </form>
    </template>
  </UModal>
</template>
