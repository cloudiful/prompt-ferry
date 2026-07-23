<script setup lang="ts">
import type { NewUserForm } from '@/models'

defineProps<{
  busy: boolean
  t: TranslateFn
}>()

const visible = defineModel<boolean>('visible', { required: true })
const form = defineModel<NewUserForm>('form', { required: true })

defineEmits<{
  submit: []
}>()
</script>

<template>
  <UModal v-model:open="visible" :title="t('createUser')">
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('submit')">
        <UInput
          v-model="form.login_name"
          class="w-full"
          placeholder="login_name"
        />
        <UInput
          v-model="form.display_name"
          class="w-full"
          :placeholder="t('displayName')"
        />
        <UInput
          v-model="form.password"
          type="password"
          class="w-full"
          :placeholder="t('userInitialPassword')"
        />
        <label
          class="inline-flex min-h-8 items-center gap-2 text-[0.75rem] text-default"
          ><UCheckbox v-model="form.is_admin" />{{ t('admin') }}</label
        >
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
            ><UIcon name="i-lucide-plus" class="h-4 w-4" />{{
              t('create')
            }}</UButton
          >
        </div>
      </form>
    </template>
  </UModal>
</template>
