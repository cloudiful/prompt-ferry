<script setup lang="ts">
import type { User } from '@/generated/admin-api'

withDefaults(
  defineProps<{
    busy: boolean
    /** This user's own write. Freezes the dialog while the guard holds it. */
    savePending?: boolean
    t: TranslateFn
    user: User | null
  }>(),
  { savePending: false },
)

const visible = defineModel<boolean>('visible', { required: true })
const password = defineModel<string>('password', { required: true })

defineEmits<{
  submit: []
}>()
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="t('resetPassword')"
    :dismissible="!savePending"
    :close="!savePending"
  >
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('submit')">
        <fieldset :disabled="savePending" class="grid gap-3">
          <div v-if="user" class="font-medium text-highlighted">
            {{ user.login_name }}
          </div>
          <UInput
            v-model="password"
            type="password"
            class="w-full"
            :placeholder="t('newPassword')"
            autofocus
          />
          <div class="flex justify-end gap-2 pt-1">
            <UButton
              type="button"
              size="sm"
              color="neutral"
              :disabled="savePending"
              @click="
                () => {
                  visible = false
                }
              "
              >{{ t('cancel') }}</UButton
            >
            <UButton type="submit" size="sm" :loading="busy || savePending"
              ><UIcon name="i-lucide-key-round" class="h-4 w-4" />{{
                t('resetPassword')
              }}</UButton
            >
          </div>
        </fieldset>
      </form>
    </template>
  </UModal>
</template>
