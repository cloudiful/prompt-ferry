<script setup lang="ts">
import type { ApprovalRequest } from '@/generated/admin-api'

const props = defineProps<{
  busy: boolean
  t: TranslateFn
  approval: ApprovalRequest | null
  formatTime: (value: string) => string
}>()

const visible = defineModel<boolean>('visible', { required: true })

defineEmits<{
  approve: [approval: ApprovalRequest]
  reject: [approval: ApprovalRequest]
}>()

function payloadText(value: unknown): string {
  return value == null ? '' : JSON.stringify(value, null, 2)
}

function approvalStatusLabel(
  status: ApprovalRequest['approval_status'],
): string {
  return status === 'pending'
    ? props.t('approvalStatusPending')
    : props.t('approvalStatusResolved')
}

function approvalStatusSeverity(
  status: ApprovalRequest['approval_status'],
): 'neutral' | 'warning' {
  return status === 'pending' ? 'warning' : 'neutral'
}
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="t('approvalDetails')"
    :ui="{ content: 'sm:max-w-5xl' }"
  >
    <template #body>
      <template v-if="approval">
        <div class="grid gap-3">
          <div class="grid gap-2 text-xs sm:grid-cols-2 lg:grid-cols-4">
            <div>
              <div class="text-muted">{{ t('user') }}</div>
              <div>{{ approval.user_login_name || '-' }}</div>
            </div>
            <div>
              <div class="text-muted">{{ t('clientKey') }}</div>
              <div>{{ approval.client_key_label || '-' }}</div>
            </div>
            <div>
              <div class="text-muted">{{ t('model') }}</div>
              <div>{{ approval.model || '-' }}</div>
            </div>
            <div>
              <div class="text-muted">{{ t('status') }}</div>
              <UBadge
                :label="approvalStatusLabel(approval.approval_status)"
                :color="approvalStatusSeverity(approval.approval_status)"
              />
            </div>
          </div>
          <div class="grid gap-2">
            <div class="text-xs text-muted">{{ t('reviewReason') }}</div>
            <div class="rounded border border-default bg-muted px-3 py-2">
              {{ approval.review_reason || '-' }}
            </div>
          </div>
          <div class="grid gap-2">
            <div class="text-xs text-muted">
              {{ t('reviewCategories') }}
            </div>
            <div class="flex flex-wrap gap-2">
              <UBadge
                v-for="category in approval.review_categories"
                :key="category"
                :label="category"
              />
              <span
                v-if="approval.review_categories.length === 0"
                class="text-dimmed"
                >-</span
              >
            </div>
          </div>
          <div class="grid gap-2">
            <div class="text-xs text-muted">
              {{ t('requestPreview') }}
            </div>
            <pre class="ms-code whitespace-pre-wrap">{{
              approval.request_preview
            }}</pre>
          </div>
          <div v-if="approval.request_payload_json" class="grid gap-2">
            <div class="text-xs text-muted">
              {{ t('reviewPayload') }}
            </div>
            <pre class="ms-code max-h-80 overflow-auto">{{
              payloadText(approval.request_payload_json)
            }}</pre>
          </div>
          <div class="grid gap-2 text-xs sm:grid-cols-2">
            <div>
              <span class="text-muted">{{ t('createdAt') }}</span>
              {{ formatTime(approval.created_at) }}
            </div>
            <div>
              <span class="text-muted">{{ t('decidedAt') }}</span>
              {{ approval.decided_at ? formatTime(approval.decided_at) : '-' }}
            </div>
          </div>
        </div>
      </template>
    </template>
    <template #footer>
      <div class="flex flex-wrap justify-end gap-2">
        <UButton
          color="neutral"
          variant="ghost"
          @click="
            () => {
              visible = false
            }
          "
          >{{ t('cancel') }}</UButton
        >
        <UButton
          v-if="approval?.approval_status === 'pending'"
          color="error"
          variant="outline"
          :loading="busy"
          @click="approval && $emit('reject', approval)"
          >{{ t('reject') }}</UButton
        >
        <UButton
          v-if="approval?.approval_status === 'pending'"
          :loading="busy"
          @click="approval && $emit('approve', approval)"
          >{{ t('approve') }}</UButton
        >
      </div>
    </template>
  </UModal>
</template>
