<script setup lang="ts">
import type { User } from '@/generated/admin-api'

defineProps<{
  busy: boolean
  t: TranslateFn
  user: User | null
}>()

const visible = defineModel<boolean>('visible', { required: true })
const password = defineModel<string>('password', { required: true })

defineEmits<{
  submit: []
}>()
</script>

<template>
  <UModal v-model:open="visible" :title="t('resetPassword')">
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('submit')">
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
            @click="
              () => {
                visible = false
              }
            "
            >{{ t('cancel') }}</UButton
          >
          <UButton type="submit" size="sm" :loading="busy"
            ><UIcon name="i-lucide-key-round" class="h-4 w-4" />{{
              t('resetPassword')
            }}</UButton
          >
        </div>
      </form>
    </template>
  </UModal>
</template>
