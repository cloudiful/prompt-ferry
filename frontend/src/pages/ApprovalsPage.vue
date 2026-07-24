<script setup lang="ts">
import { onMounted, ref } from 'vue'
import PageIntro from '../components/PageIntro.vue'
import { useLocale } from '../composables/useLocale'
import { useNotifier } from '../composables/useNotifier'
import ApprovalsPanel from '../components/approvals/ApprovalsPanel.vue'
import type {
  ApprovalRequest,
  ApprovalStatusFilter,
} from '../generated/admin-api'
import { useApprovalsStore } from '../stores/approvals'

const { t } = useLocale()
const { notifyApiError, notifySuccess } = useNotifier()
const approvalsStore = useApprovalsStore()

const detailVisible = ref(false)
function formatTime(value: string): string {
  return new Date(value).toLocaleString()
}

async function refresh(): Promise<void> {
  try {
    await approvalsStore.refresh()
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function openDetail(approval: ApprovalRequest): Promise<void> {
  try {
    await approvalsStore.loadDetail(approval.approval_id)
    detailVisible.value = true
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function approve(approval: ApprovalRequest): Promise<void> {
  try {
    await approvalsStore.approve(approval.approval_id)
    notifySuccess(t('approvalApproved'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function reject(approval: ApprovalRequest): Promise<void> {
  try {
    await approvalsStore.reject(approval.approval_id)
    notifySuccess(t('approvalRejected'))
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function onPage(event: TablePageChange): Promise<void> {
  try {
    await approvalsStore.refresh(
      approvalsStore.currentFilter,
      event.first,
      event.rows,
    )
  } catch (cause) {
    notifyApiError(cause)
  }
}

async function onFilter(nextFilter: ApprovalStatusFilter): Promise<void> {
  try {
    await approvalsStore.refresh(nextFilter, 0, approvalsStore.rows)
  } catch (cause) {
    notifyApiError(cause)
  }
}

onMounted(async () => {
  await refresh()
})
</script>

<template>
  <div class="grid min-w-0 max-w-full gap-3">
    <PageIntro :eyebrow="t('review')" :title="t('approvals')">
      <template #actions>
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          :loading="approvalsStore.loading"
          @click="refresh"
          >{{ t('refresh') }}</UButton
        >
      </template>
    </PageIntro>

    <ApprovalsPanel
      v-model:approval-filter="approvalsStore.currentFilter"
      v-model:detail-visible="detailVisible"
      v-model:detail-approval="approvalsStore.detailApproval"
      :approvals="approvalsStore.approvals"
      :approval-first="approvalsStore.first"
      :approval-rows="approvalsStore.rows"
      :approval-total="approvalsStore.total"
      :busy="approvalsStore.loading || approvalsStore.detailLoading"
      :format-time="formatTime"
      :t="t"
      @approval-page="onPage"
      @update:approval-filter="onFilter"
      @open-detail="openDetail"
      @approve-approval="approve"
      @reject-approval="reject"
    />
  </div>
</template>
