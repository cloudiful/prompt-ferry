<script setup lang="ts">
import type { NewUserForm } from '@/models'

withDefaults(
  defineProps<{
    busy: boolean
    /** This dialog's own write. Freezes it while the guard holds its key. */
    savePending?: boolean
    t: TranslateFn
  }>(),
  { savePending: false },
)

const visible = defineModel<boolean>('visible', { required: true })
const form = defineModel<NewUserForm>('form', { required: true })

defineEmits<{
  submit: []
}>()
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="t('createUser')"
    :dismissible="!savePending"
    :close="!savePending"
  >
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('submit')">
        <fieldset :disabled="savePending" class="grid gap-3">
          <UFormField :label="t('loginName')">
            <UInput
              v-model="form.login_name"
              class="w-full"
              :placeholder="t('userLoginPlaceholder')"
            />
          </UFormField>
          <UFormField :label="t('displayName')">
            <UInput v-model="form.display_name" class="w-full" />
          </UFormField>
          <UFormField :label="t('userInitialPassword')">
            <UInput v-model="form.password" type="password" class="w-full" />
          </UFormField>
          <label
            class="inline-flex min-h-8 items-center gap-2 text-[0.75rem] text-default"
            ><UCheckbox v-model="form.is_admin" />{{ t('admin') }}</label
          >
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
              ><UIcon name="i-lucide-plus" class="h-4 w-4" />{{
                t('create')
              }}</UButton
            >
          </div>
        </fieldset>
      </form>
    </template>
  </UModal>
</template>
