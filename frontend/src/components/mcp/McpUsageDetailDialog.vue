<script setup lang="ts">
import { computed, ref, type Ref, watch } from 'vue'
import PreviewExpansionActions from '@/components/shared/PreviewExpansionActions.vue'
import FlatSection from '@/components/shared/FlatSection.vue'
import DetailKeyValue from '@/components/usage/detail/DetailKeyValue.vue'
import DetailMetricCard from '@/components/usage/detail/DetailMetricCard.vue'
import UsageErrorSection from '@/components/usage/detail/UsageErrorSection.vue'
import type { RequestRecordDetailView } from '@/models'
import type { RequestRecordFormatting } from '@/models/request-record-formatting'
import MarkdownLog from '@/components/shared/MarkdownLog.vue'
import { copyText } from '@/composables/useClipboard'
import {
  collapsePreview,
  previewText,
  resetPreviewLevels,
  showAllPreview,
  showMorePreview,
} from '@/composables/useTextPreview'
import {
  formatBytes,
  formatCompressionRatio,
} from '@/composables/useUsageFormatting'

const props = defineProps<{
  detailLoading: boolean
  event: RequestRecordDetailView | null
  formatting: RequestRecordFormatting
  t: TranslateFn
}>()

const visible = defineModel<boolean>('visible', { required: true })
const responsePreviewLevel = ref(1)

const PREVIEW_STEP_LINES = 120
const PREVIEW_STEP_CHARS = 10_000

const requestJsonText = computed(() =>
  props.event?.request_raw_json != null
    ? JSON.stringify(props.event.request_raw_json, null, 2)
    : '',
)
const responseText = computed(() => props.event?.response_prompt || '')
const requestCompressionText = computed(
  () =>
    props.event?.http_request_content_encoding ||
    props.t('requestCompressionNone'),
)
const compressedBytesText = computed(() =>
  formatBytes(props.event?.http_request_compressed_bytes),
)
const decompressedBytesText = computed(() =>
  formatBytes(props.event?.http_request_decompressed_bytes),
)
const compressionRatioText = computed(() =>
  formatCompressionRatio(props.event?.http_request_compression_ratio),
)

const responsePreview = computed(() =>
  previewText(
    responseText.value,
    responsePreviewLevel.value,
    PREVIEW_STEP_CHARS,
    PREVIEW_STEP_LINES,
  ),
)
watch(visible, (nextVisible) => {
  if (!nextVisible) {
    resetPreviewLevels(responsePreviewLevel)
  }
})

function createPreviewActions(level: Ref<number>): {
  all: () => void
  collapse: () => void
  more: () => void
} {
  return {
    all: () => showAllPreview(level),
    collapse: () => collapsePreview(level),
    more: () => showMorePreview(level),
  }
}

const responsePreviewActions = createPreviewActions(responsePreviewLevel)
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="t('requestDetails')"
    :ui="{ content: 'sm:max-w-5xl', body: 'max-h-[80vh] overflow-y-auto' }"
  >
    <template #body>
      <div v-if="event" class="grid gap-3 text-xs">
        <FlatSection :title="t('requestContext')">
          <div class="grid gap-3">
            <div class="grid gap-2 sm:grid-cols-4">
              <DetailMetricCard :label="t('status')">
                <UBadge
                  :label="
                    formatting.formatRequestStateLabel(event.request_state)
                  "
                  :color="formatting.requestStateSeverity(event.request_state)"
                />
              </DetailMetricCard>
              <DetailMetricCard :label="t('totalLatency')">{{
                formatting.formatMs(event.duration_ms)
              }}</DetailMetricCard>
              <DetailMetricCard :label="t('mcpServer')">{{
                event.mcp_server_name || '-'
              }}</DetailMetricCard>
              <DetailMetricCard :label="t('mcpMethod')">{{
                event.mcp_protocol_method || '-'
              }}</DetailMetricCard>
            </div>
            <div class="grid gap-2 sm:grid-cols-4">
              <DetailMetricCard :label="t('requestCompression')">{{
                requestCompressionText
              }}</DetailMetricCard>
              <DetailMetricCard :label="t('compressedBytes')">{{
                compressedBytesText
              }}</DetailMetricCard>
              <DetailMetricCard :label="t('decompressedBytes')">{{
                decompressedBytesText
              }}</DetailMetricCard>
              <DetailMetricCard :label="t('compressionRatio')">{{
                compressionRatioText
              }}</DetailMetricCard>
            </div>
            <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
              <DetailKeyValue :label="t('mcpOperation')">
                <span class="break-all">{{
                  event.mcp_operation_name || '-'
                }}</span>
              </DetailKeyValue>
              <DetailKeyValue :label="t('clientKey')">
                {{ event.client_key_label || '-' }}
              </DetailKeyValue>
              <DetailKeyValue :label="t('userAgent')">
                <span class="break-all">{{
                  event.request_user_agent || '-'
                }}</span>
              </DetailKeyValue>
              <DetailKeyValue :label="t('mcpServerId')">
                <span class="break-all">{{ event.mcp_server_id || '-' }}</span>
              </DetailKeyValue>
              <DetailKeyValue :label="t('createdAt')">
                {{ event.created_at }}
              </DetailKeyValue>
              <DetailKeyValue :label="t('requestId')">
                <span class="break-all">{{ event.request_id }}</span>
              </DetailKeyValue>
            </div>
          </div>
        </FlatSection>

        <div class="grid items-stretch gap-3 lg:grid-cols-2">
          <FlatSection :title="t('mcpRequestPayload')">
            <div class="grid gap-2">
              <div class="flex flex-wrap gap-2">
                <UButton
                  v-if="requestJsonText"
                  size="sm"
                  color="neutral"
                  variant="ghost"
                  @click="copyText(requestJsonText)"
                >
                  {{ t('copy') }}
                </UButton>
              </div>
              <pre
                v-if="requestJsonText"
                class="ms-code max-h-[18rem] overflow-auto"
                >{{ requestJsonText }}</pre>
              <MarkdownLog
                v-else
                text=""
                :empty-text="t('contentLoggingOff')"
                max-height="18rem"
              />
            </div>
          </FlatSection>

          <FlatSection :title="t('mcpResponsePayload')">
            <div class="grid gap-2">
              <UButton
                v-if="responseText"
                size="sm"
                color="neutral"
                variant="ghost"
                class="justify-self-start"
                @click="copyText(responseText)"
              >
                {{ t('copy') }}
              </UButton>
              <MarkdownLog
                :text="responsePreview.text"
                :empty-text="
                  detailLoading ? t('loading') : t('contentLoggingOff')
                "
                max-height="18rem"
              />
              <PreviewExpansionActions
                :all-label="t('showFullContent')"
                :buttons-class="'flex flex-wrap gap-2'"
                :collapse-label="t('collapseContent')"
                :expanded="responsePreviewLevel > 1"
                :has-more="responsePreview.hasMore"
                :more-label="t('showMoreContent')"
                :truncated-label="t('truncatedPreview')"
                @all="responsePreviewActions.all"
                @collapse="responsePreviewActions.collapse"
                @more="responsePreviewActions.more"
              />
            </div>
          </FlatSection>
        </div>

        <UsageErrorSection :event="event" :t="t" :visible="visible" />
      </div>
    </template>
  </UModal>
</template>
