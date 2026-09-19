<script setup lang="ts">
const props = defineProps<{
  open: boolean
  rule: { index: number; pattern: string } | null
  t: TranslateFn
}>()

const emit = defineEmits<{
  cancel: []
  confirm: [index: number]
}>()

function onOpenChange(value: boolean): void {
  if (!value) emit('cancel')
}

function onConfirm(): void {
  if (props.rule) emit('confirm', props.rule.index)
}
</script>

<template>
  <UModal
    :open="open"
    :title="t('deleteRuleConfirmTitle')"
    :ui="{ content: 'sm:max-w-md' }"
    @update:open="onOpenChange"
  >
    <template #body>
      <div class="grid gap-3">
        <p class="m-0 text-sm text-default">
          {{ t('deleteRuleConfirmBody') }}
        </p>
        <p
          v-if="rule && rule.pattern"
          class="m-0 rounded-md border border-default bg-muted px-2 py-1 font-mono text-[0.78rem] text-default"
        >
          {{ rule.pattern }}
        </p>
        <div class="flex justify-end gap-2">
          <UButton
            type="button"
            size="sm"
            color="neutral"
            variant="outline"
            @click="$emit('cancel')"
          >
            {{ t('deleteRuleCancel') }}
          </UButton>
          <UButton type="button" size="sm" color="error" @click="onConfirm">
            <UIcon name="i-lucide-trash-2" class="h-4 w-4" />
            {{ t('deleteRuleConfirmSubmit') }}
          </UButton>
        </div>
      </div>
    </template>
  </UModal>
</template>
