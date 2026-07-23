<script setup lang="ts">
import { computed, watch } from 'vue'
import RequestTranscriptTimeline from '@/components/usage/detail/RequestTranscriptTimeline.vue'
import type { RequestRecordFullResponse } from '@/generated/admin-api'
import type { RequestRecordDetailView } from '@/models'
import { type RequestRecordDetailWithAssistant } from '@/models/request-record-transcript'
import { isRequestRecordTerminal } from '@/request-records'

const props = defineProps<{
  detailLoading: boolean
  event: RequestRecordDetailView | null
  requestFull: RequestRecordFullResponse | null
  requestFullLoading: boolean
  t: TranslateFn
  visible: boolean
}>()

const emit = defineEmits<{
  loadRequestFull: []
}>()

const detailEvent = computed<RequestRecordDetailWithAssistant | null>(
  () => props.event as RequestRecordDetailWithAssistant | null,
)

const responsePendingText = computed(() => {
  if (!props.event || isRequestRecordTerminal(props.event.request_state))
    return ''
  return props.t('processingResponsePending')
})

watch(
  () => props.visible,
  (nextVisible) => {
    if (!nextVisible) return
    if (
      props.event?.has_full_request &&
      !props.requestFull &&
      !props.requestFullLoading
    ) {
      emit('loadRequestFull')
    }
  },
)

watch(
  [
    () => props.visible,
    () => props.event?.record_id,
    () => props.event?.has_full_request,
    () => props.requestFull,
    () => props.requestFullLoading,
  ],
  () => {
    if (
      props.visible &&
      props.event?.has_full_request &&
      !props.requestFull &&
      !props.requestFullLoading
    ) {
      emit('loadRequestFull')
    }
  },
  { immediate: true },
)
</script>

<template>
  <RequestTranscriptTimeline
    :detail="detailEvent"
    :request-full="requestFull"
    :request-full-loading="requestFullLoading"
    :response-pending-text="responsePendingText"
    :t="t"
  />
</template>
