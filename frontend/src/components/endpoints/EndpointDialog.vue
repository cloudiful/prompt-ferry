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
