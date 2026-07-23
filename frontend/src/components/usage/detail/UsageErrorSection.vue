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

const errorSummaryText = computed(() => {
  if (!props.event?.error_message) return ''
  return props.event.error_code
    ? `${props.event.error_code}: ${props.event.error_message}`
    : props.event.error_message
})
const upstreamErrorText = computed(() => props.event?.upstream_error_body || '')

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
    v-if="event?.error_message || event?.upstream_error_body"
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
        class="break-all rounded border border-error bg-error/10 p-3 text-error"
      >
        {{ errorSummaryText }}
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
  </div>
</template>
