<script setup lang="ts">
import { computed } from 'vue'
import type { User } from '@/generated/admin-api'
import type { EndpointForm } from '@/models'
import EndpointApiKeysEditor from '@/components/endpoints/EndpointApiKeysEditor.vue'
import RequestLimitFields from '@/components/shared/RequestLimitFields.vue'

defineProps<{
  busy: boolean
  header: string
  t: TranslateFn
  users: User[]
}>()

const visible = defineModel<boolean>('visible', { required: true })
const form = defineModel<EndpointForm>('form', { required: true })
const hasVersionPath = computed(() =>
  /\/v1\/?$/.test(form.value.base_url.trim()),
)
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

defineEmits<{
  save: []
}>()
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="header"
    :ui="{ content: 'sm:max-w-4xl' }"
  >
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('save')">
        <div class="grid gap-3 md:grid-cols-[9rem_minmax(0,1fr)_12rem]">
          <USelect
            v-model="form.scope"
            class="w-full"
            :items="['admin', 'user']"
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
        <div
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
        <EndpointApiKeysEditor v-model:form="form" :t="t" />
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
