<script setup lang="ts">
import type { User } from '@/generated/admin-api'
import type { EndpointForm } from '@/models'
import EndpointApiKeysEditor from '@/components/endpoints/EndpointApiKeysEditor.vue'
import EndpointProviderFields from '@/components/endpoints/EndpointProviderFields.vue'
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

// Issue #368 Phase C: clear the saved proxy so the next save sends `""`
// (clear to direct). Leaving the field blank while `has_saved_proxy_url`
// is true omits the key and keeps the stored value (see
// `endpointFormToRequest`).
function clearProxyUrl(): void {
  form.value.proxy_url = ''
  form.value.has_saved_proxy_url = false
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
        <div class="grid gap-2 border-t border-default pt-3">
          <div class="flex items-center gap-1">
            <label
              for="endpoint-proxy-url"
              class="text-xs font-medium text-default"
            >
              {{ t('proxyUrl') }}
            </label>
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
          </div>
          <div class="flex min-w-0 items-center gap-2">
            <div v-if="form.has_saved_proxy_url" class="shrink-0">
              <UBadge :label="t('saved')" color="neutral" />
            </div>
            <UInput
              id="endpoint-proxy-url"
              v-model="form.proxy_url"
              type="password"
              class="min-w-0 flex-1"
              :placeholder="
                form.has_saved_proxy_url
                  ? t('savedSecret')
                  : t('proxyUrlPlaceholder')
              "
            />
            <UButton
              v-if="form.has_saved_proxy_url"
              type="button"
              size="sm"
              color="neutral"
              variant="ghost"
              :aria-label="t('proxyClear')"
              @click="clearProxyUrl"
            >
              <UIcon name="i-lucide-trash-2" class="h-4 w-4" />
            </UButton>
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
