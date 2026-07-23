<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import type { ApprovalRequest } from '@/generated/admin-api'
import type { ApprovalFilter } from '@/models'
import { APPROVAL_PAGE_SIZE_OPTIONS } from '@/table-pagination'
import ApprovalDetailDialog from './ApprovalDetailDialog.vue'
import TablePagination from '@/components/shared/TablePagination.vue'
import WorkspacePagerBar from '@/components/shared/WorkspacePagerBar.vue'
const props = defineProps<{
  approvals: ApprovalRequest[]
  approvalFirst: number
  approvalRows: number
  approvalTotal: number
  busy: boolean
  formatTime: (value: string) => string
  t: TranslateFn
}>()
const approvalFilter = defineModel<ApprovalFilter>('approvalFilter', {
  required: true,
})
const detailVisible = defineModel<boolean>('detailVisible', { required: true })
const detailApproval = defineModel<ApprovalRequest | null>('detailApproval', {
  required: true,
})

const emit = defineEmits<{
  approvalPage: [event: TablePageChange]
  openDetail: [approval: ApprovalRequest]
  approveApproval: [approval: ApprovalRequest]
  rejectApproval: [approval: ApprovalRequest]
}>()

const now = ref(Date.now())
let timer: number | null = null

const filterOptions = computed(() => [
  { label: props.t('pendingApprovals'), value: 'pending' },
  { label: props.t('resolvedApprovals'), value: 'resolved' },
])
const columns = computed<TableColumn<ApprovalRequest>[]>(() => [
  { accessorKey: 'user_login_name', header: props.t('user') },
  { accessorKey: 'client_key_label', header: props.t('clientKey') },
  { accessorKey: 'model', header: props.t('model') },
  { accessorKey: 'review_reason', header: props.t('reviewReason') },
  { accessorKey: 'created_at', header: props.t('createdAt') },
  { id: 'remaining', header: props.t('remainingWait') },
  { accessorKey: 'approval_status', header: props.t('status') },
  { id: 'actions' },
])

const canPrev = computed(() => props.approvalFirst > 0)
const canNext = computed(
  () => props.approvalFirst + props.approvalRows < props.approvalTotal,
)

const pageStart = computed(() =>
  props.approvalTotal === 0 ? 0 : props.approvalFirst + 1,
)

const pageEnd = computed(() =>
  Math.min(props.approvalFirst + props.approvalRows, props.approvalTotal),
)
const mobileMetaLabelClass =
  'text-[0.7rem] font-bold tracking-wide text-dimmed uppercase'
const mobileMetaValueClass =
  'break-words text-[0.76rem] leading-[1.38] text-default'

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

function formatRemaining(approval: ApprovalRequest): string {
  if (approval.approval_status !== 'pending') return '-'
  const ms = Math.max(0, approval.wait_deadline_unix_ms - now.value)
  const seconds = Math.floor(ms / 1000)
  const minutes = Math.floor(seconds / 60)
  if (minutes > 0) return `${minutes}m ${seconds % 60}s`
  return `${seconds}s`
}

function mobilePage(direction: 'prev' | 'next'): void {
  const nextFirst =
    direction === 'prev'
      ? Math.max(0, props.approvalFirst - props.approvalRows)
      : props.approvalFirst + props.approvalRows

  emit('approvalPage', {
    first: nextFirst,
    page: Math.floor(nextFirst / props.approvalRows),
    pageCount: Math.max(1, Math.ceil(props.approvalTotal / props.approvalRows)),
    rows: props.approvalRows,
  } as TablePageChange)
}

onMounted(() => {
  timer = window.setInterval(() => {
    now.value = Date.now()
  }, 1000)
})

onBeforeUnmount(() => {
  if (timer != null) window.clearInterval(timer)
})
</script>

<template>
  <section class="grid gap-3">
    <section
      class="flex flex-wrap items-start justify-between gap-3 rounded-xl border border-default bg-default px-3 py-3 md:hidden"
    >
      <div
        class="grid w-full grid-cols-[auto_minmax(0,1fr)] items-center gap-2"
      >
        <UBadge :label="`${pageStart}-${pageEnd} / ${approvalTotal}`" />
        <USelect
          v-model="approvalFilter"
          class="min-w-0 w-full md:min-w-40 md:w-auto"
          size="sm"
          :items="filterOptions"
          label-key="label"
          value-key="value"
        />
      </div>
    </section>

    <template v-if="!approvals.length">
      <div
        class="rounded-xl border border-default bg-default px-4 py-6 text-sm text-dimmed"
      >
        {{ t('noApprovals') }}
      </div>
    </template>

    <template v-else>
      <div class="grid gap-3 md:hidden">
        <article
          v-for="approval in approvals"
          :key="approval.approval_id"
          class="grid gap-3 rounded-lg border border-default bg-default p-3"
        >
          <div class="flex items-start justify-between gap-2">
            <div class="min-w-0">
              <div
                class="text-[0.88rem] leading-[1.2] font-bold text-highlighted"
              >
                {{ approval.model || '-' }}
              </div>
              <div
                class="mt-px break-words text-[0.7rem] leading-[1.35] text-dimmed"
              >
                {{ approval.user_login_name || '-' }} /
                {{ approval.client_key_label || '-' }}
              </div>
            </div>
            <UBadge
              :label="approvalStatusLabel(approval.approval_status)"
              :color="approvalStatusSeverity(approval.approval_status)"
            />
          </div>

          <div class="grid gap-1.5">
            <div class="grid gap-px">
              <div :class="mobileMetaLabelClass">{{ t('reviewReason') }}</div>
              <div :class="mobileMetaValueClass">
                {{ approval.review_reason || '-' }}
              </div>
            </div>
            <div class="grid gap-px">
              <div :class="mobileMetaLabelClass">{{ t('createdAt') }}</div>
              <div :class="mobileMetaValueClass">
                {{ formatTime(approval.created_at) }}
              </div>
            </div>
            <div class="grid gap-px">
              <div :class="mobileMetaLabelClass">{{ t('remainingWait') }}</div>
              <div :class="mobileMetaValueClass">
                {{ formatRemaining(approval) }}
              </div>
            </div>
          </div>

          <div class="grid gap-2 [&>button]:w-full [&>button]:justify-center">
            <UButton
              size="sm"
              color="neutral"
              variant="outline"
              @click="$emit('openDetail', approval)"
              >{{ t('viewDetails') }}</UButton
            >
            <UButton
              v-if="approval.approval_status === 'pending'"
              size="sm"
              color="error"
              variant="outline"
              :loading="busy"
              @click="$emit('rejectApproval', approval)"
            >
              {{ t('reject') }}
            </UButton>
            <UButton
              v-if="approval.approval_status === 'pending'"
              size="sm"
              :loading="busy"
              @click="$emit('approveApproval', approval)"
            >
              {{ t('approve') }}
            </UButton>
          </div>
        </article>

        <WorkspacePagerBar
          v-if="approvalTotal > approvalRows"
          :can-next="canNext"
          :can-prev="canPrev"
          :end="pageEnd"
          :start="pageStart"
          :total="approvalTotal"
          @next="mobilePage('next')"
          @prev="mobilePage('prev')"
        />
      </div>

      <div class="hidden min-w-0 md:block">
        <div class="border-b border-default p-3">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <div class="text-xs text-muted">
              {{ pageStart }}-{{ pageEnd }} / {{ approvalTotal }}
            </div>
            <USelect
              v-model="approvalFilter"
              class="min-w-40"
              size="sm"
              :items="filterOptions"
              label-key="label"
              value-key="value"
            />
          </div>
        </div>
        <UTable
          :data="approvals"
          :columns="columns"
          :loading="busy"
          class="min-w-0"
        >
          <template #client_key_label-cell="{ row }">{{
            row.original.client_key_label || '-'
          }}</template>
          <template #model-cell="{ row }">{{
            row.original.model || '-'
          }}</template>
          <template #review_reason-cell="{ row }">
            <button
              type="button"
              class="inline-flex max-w-full items-center gap-1 truncate text-left text-primary whitespace-nowrap hover:text-highlighted"
              @click="$emit('openDetail', row.original)"
            >
              {{ row.original.review_reason || '-' }}
            </button>
          </template>
          <template #created_at-cell="{ row }">{{
            formatTime(row.original.created_at)
          }}</template>
          <template #remaining-cell="{ row }">
            <div class="flex items-center gap-2">
              <UIcon name="i-lucide-clock" class="h-4 w-4 text-muted" />
              <span>{{ formatRemaining(row.original) }}</span>
            </div>
          </template>
          <template #approval_status-cell="{ row }">
            <UBadge
              :label="approvalStatusLabel(row.original.approval_status)"
              :color="approvalStatusSeverity(row.original.approval_status)"
            />
          </template>
          <template #actions-cell="{ row }">
            <div class="flex gap-2">
              <UButton
                size="sm"
                color="neutral"
                variant="outline"
                @click="$emit('openDetail', row.original)"
                >{{ t('viewDetails') }}</UButton
              >
              <UButton
                v-if="row.original.approval_status === 'pending'"
                size="sm"
                color="error"
                variant="ghost"
                :loading="busy"
                @click="$emit('rejectApproval', row.original)"
                ><UIcon name="i-lucide-x" class="h-4 w-4"
              /></UButton>
              <UButton
                v-if="row.original.approval_status === 'pending'"
                size="sm"
                :loading="busy"
                @click="$emit('approveApproval', row.original)"
                ><UIcon name="i-lucide-check" class="h-4 w-4"
              /></UButton>
            </div>
          </template>
        </UTable>
        <TablePagination
          :first="approvalFirst"
          :rows="approvalRows"
          :total="approvalTotal"
          :page-size-options="APPROVAL_PAGE_SIZE_OPTIONS"
          @change="$emit('approvalPage', $event)"
        />
      </div>
    </template>

    <ApprovalDetailDialog
      v-model:visible="detailVisible"
      :approval="detailApproval"
      :busy="busy"
      :format-time="formatTime"
      :t="t"
      @approve="$emit('approveApproval', $event)"
      @reject="$emit('rejectApproval', $event)"
    />
  </section>
</template>
