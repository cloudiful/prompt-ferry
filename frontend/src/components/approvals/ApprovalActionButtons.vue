<script setup lang="ts">
import type { ApprovalRequest } from '@/generated/admin-api'

const props = defineProps<{
  approval: ApprovalRequest
  layout: 'mobile' | 'desktop'
  pendingKeys?: Readonly<Record<string, string>>
  t: TranslateFn
}>()

const emit = defineEmits<{
  approve: [approval: ApprovalRequest]
  openDetail: [approval: ApprovalRequest]
  reject: [approval: ApprovalRequest]
}>()

function isPending(): boolean {
  return Object.hasOwn(props.pendingKeys ?? {}, props.approval.approval_id)
}

function pendingAction(action: 'approve' | 'reject'): boolean {
  return props.pendingKeys?.[props.approval.approval_id] === action
}
</script>

<template>
  <div
    :class="
      layout === 'mobile'
        ? 'grid gap-2 [&>button]:w-full [&>button]:justify-center'
        : 'flex gap-2'
    "
  >
    <UButton
      size="sm"
      color="neutral"
      variant="outline"
      @click="emit('openDetail', approval)"
    >
      {{ t('viewDetails') }}
    </UButton>
    <UButton
      v-if="approval.approval_status === 'pending'"
      size="sm"
      color="error"
      :variant="layout === 'mobile' ? 'outline' : 'ghost'"
      :loading="pendingAction('reject')"
      :disabled="isPending()"
      @click="emit('reject', approval)"
    >
      <template v-if="layout === 'mobile'">{{ t('reject') }}</template>
      <UIcon v-else name="i-lucide-x" class="h-4 w-4" />
    </UButton>
    <UButton
      v-if="approval.approval_status === 'pending'"
      size="sm"
      :loading="pendingAction('approve')"
      :disabled="isPending()"
      @click="emit('approve', approval)"
    >
      <template v-if="layout === 'mobile'">{{ t('approve') }}</template>
      <UIcon v-else name="i-lucide-check" class="h-4 w-4" />
    </UButton>
  </div>
</template>
