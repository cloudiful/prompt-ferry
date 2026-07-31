<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import TranscriptTextPreview from '@/components/usage/detail/TranscriptTextPreview.vue'
import { copyText } from '@/composables/useClipboard'
import { resetPreviewLevels } from '@/composables/useTextPreview'
import type { RequestRecordDetailView } from '@/models'

const props = defineProps<{
  event: RequestRecordDetailView | null
  t: TranslateFn
  visible: boolean
}>()

const upstreamErrorPreviewLevel = ref(1)

const isAborted = computed(() => props.event?.request_state === 'aborted')

const errorSummaryText = computed(() => {
  if (!props.event?.error_message) return ''
  return props.event.error_code
    ? `${props.event.error_code}: ${props.event.error_message}`
    : props.event.error_message
})
const upstreamErrorText = computed(() => props.event?.upstream_error_body || '')

const abortReasonText = computed(() => {
  switch (props.event?.abort_reason) {
    case 'downstream_closed':
      return props.t('abortReasonDownstreamClosed')
    case 'bridge_backpressure_full':
      return props.t('abortReasonBridgeBackpressureFull')
    case 'bridge_backpressure_bytes_limit':
      return props.t('abortReasonBridgeBackpressureBytesLimit')
    case 'worker_lease_expired':
      return props.t('abortReasonWorkerLeaseExpired')
    case 'valkey_lease_missing':
      return props.t('abortReasonValkeyLeaseMissing')
    case 'relay_unknown':
      return props.t('abortReasonRelayUnknown')
    default:
      return props.t('notAvailable')
  }
})

const abortFromStateText = computed(() => {
  switch (props.event?.abort_from_state) {
    case 'received':
      return props.t('requestStateReceived')
    case 'awaiting_approval':
      return props.t('requestStateAwaitingApproval')
    case 'upstream_processing':
      return props.t('requestStateUpstreamProcessing')
    default:
      return props.t('notAvailable')
  }
})

const abortResponseStartedText = computed(() => {
  if (props.event?.abort_response_started === true) {
    return props.t('responseStartedYes')
  }
  if (props.event?.abort_response_started === false) {
    return props.t('responseStartedNo')
  }
  return props.t('notAvailable')
})

watch(
  () => props.visible,
  (nextVisible) => {
    if (!nextVisible) resetPreviewLevels(upstreamErrorPreviewLevel)
  },
)

watch(
  () => props.event?.record_id,
  () => {
    resetPreviewLevels(upstreamErrorPreviewLevel)
  },
)
</script>

<template>
  <div
    v-if="event?.error_message || event?.upstream_error_body || isAborted"
    class="grid gap-3 rounded border border-default bg-muted p-3"
  >
    <div class="flex flex-wrap items-center justify-between gap-2">
      <div class="text-xs font-semibold text-muted">
        {{ t('errorDetails') }}
      </div>
      <div class="flex flex-wrap items-center gap-2">
        <UButton
          v-if="event.error_message"
          size="sm"
          color="neutral"
          variant="ghost"
          @click="copyText(errorSummaryText)"
        >
          {{ t('copy') }}
        </UButton>
        <UButton
          v-if="event.upstream_error_body"
          size="sm"
          color="neutral"
          variant="ghost"
          @click="copyText(upstreamErrorText)"
        >
          {{ t('copy') }} {{ t('upstreamErrorBody') }}
        </UButton>
      </div>
    </div>
    <div v-if="event.error_message" class="grid gap-1">
      <div class="text-xs font-semibold text-muted">
        {{ t('errorMessage') }}
      </div>
      <div
        :class="[
          'break-all rounded border p-3',
          isAborted
            ? 'border-default bg-muted text-muted'
            : 'border-error bg-error/10 text-error',
        ]"
      >
        {{ errorSummaryText }}
      </div>
    </div>
    <div v-if="isAborted" class="grid gap-2">
      <div class="text-xs font-semibold text-muted">
        {{ t('abortDetails') }}
      </div>
      <div class="grid gap-2 sm:grid-cols-3">
        <div class="grid gap-1">
          <div class="text-xs text-muted">{{ t('abortReason') }}</div>
          <div>{{ abortReasonText }}</div>
        </div>
        <div class="grid gap-1">
          <div class="text-xs text-muted">{{ t('abortFromState') }}</div>
          <div>{{ abortFromStateText }}</div>
        </div>
        <div class="grid gap-1">
          <div class="text-xs text-muted">{{ t('abortResponseStarted') }}</div>
          <div>{{ abortResponseStartedText }}</div>
        </div>
      </div>
    </div>
    <div v-if="event.upstream_error_body" class="grid gap-2">
      <div class="text-xs font-semibold text-muted">
        {{ t('upstreamErrorBody') }}
      </div>
      <TranscriptTextPreview
        v-model:level="upstreamErrorPreviewLevel"
        :all-label="t('showFullContent')"
        :collapse-label="t('collapseContent')"
        :empty-text="t('contentLoggingOff')"
        max-height="10rem"
        :more-label="t('showMoreContent')"
        mode="markdown"
        :text="upstreamErrorText"
        :truncated-label="t('truncatedPreview')"
      />
    </div>
    <div v-else-if="event.error_message || isAborted" class="text-xs text-muted">
      {{ t('noUpstreamErrorBody') }}
    </div>
  </div>
</template>
