<script setup lang="ts">
withDefaults(
  defineProps<{
    busy: boolean
    generatedSecret: string
    /** The create's own write, for the selected user. */
    savePending?: boolean
    t: TranslateFn
  }>(),
  { savePending: false },
)

const visible = defineModel<boolean>('visible', { required: true })
const keyLabel = defineModel<string>('keyLabel', { required: true })

defineEmits<{
  copySecret: []
  submit: []
}>()
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="t('generateKey')"
    :dismissible="!savePending"
    :close="!savePending"
    :ui="{ content: 'sm:max-w-2xl' }"
  >
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('submit')">
        <UInput
          v-if="!generatedSecret"
          v-model="keyLabel"
          class="w-full"
          :placeholder="t('keyLabel')"
          autofocus
        />
        <div v-if="generatedSecret" class="flex min-w-0 items-center gap-3">
          <div class="ms-code min-w-0 flex-1 truncate text-sm text-default">
            {{ generatedSecret }}
          </div>
          <UButton
            type="button"
            size="sm"
            color="neutral"
            variant="outline"
            class="shrink-0"
            @click="$emit('copySecret')"
          >
            <UIcon name="i-lucide-copy" class="h-4 w-4" />
            {{ t('copy') }}
          </UButton>
        </div>
        <div class="flex justify-end gap-2 pt-1">
          <UButton
            v-if="!generatedSecret"
            type="submit"
            size="sm"
            :disabled="savePending"
            :loading="busy || savePending"
          >
            <UIcon name="i-lucide-check" class="h-4 w-4" />{{
              t('generateKey')
            }}
          </UButton>
        </div>
      </form>
    </template>
  </UModal>
</template>
