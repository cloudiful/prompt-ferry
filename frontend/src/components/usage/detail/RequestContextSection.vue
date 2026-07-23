<script setup lang="ts">
import DetailKeyValue from './DetailKeyValue.vue'
import FlatSection from '@/components/shared/FlatSection.vue'
import type { RequestRecordDetailView } from '@/models'
import type { RequestRecordFormatting } from '@/models/request-record-formatting'

defineProps<{
  event: RequestRecordDetailView
  conversationSourceText: string
  installationIdText: string
  normalizedItemCountText: string | number
  requestCompressionText: string
  compressedBytesText: string
  decompressedBytesText: string
  compressionRatioText: string
  formatting: RequestRecordFormatting
  t: TranslateFn
}>()
</script>

<template>
  <FlatSection :title="t('requestContext')">
    <div class="grid gap-3">
      <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-5">
        <DetailKeyValue :label="t('status')">
          <div class="flex flex-wrap items-center gap-1">
            <UBadge
              :label="formatting.formatRequestStateLabel(event.request_state)"
              :color="formatting.requestStateSeverity(event.request_state)"
            />
            <UButton
              v-if="event.request_category === 'ai'"
              size="xs"
              color="neutral"
              variant="link"
              icon="i-lucide-receipt-text"
              :to="{ path: '/billing', query: { request_id: event.request_id } }"
              :aria-label="t('requestLinkBilling')"
              :label="t('requestLinkBilling')"
            />
          </div>
        </DetailKeyValue>
        <DetailKeyValue :label="t('tokens')">
          {{ formatting.formatCount(event.input_tokens) }} /
          {{ formatting.formatCount(event.output_tokens) }} /
          {{ formatting.formatCount(event.total_tokens) }}
        </DetailKeyValue>
        <DetailKeyValue :label="t('cachedTokens')">
          {{ formatting.formatCount(event.cached_tokens) }}
        </DetailKeyValue>
        <DetailKeyValue :label="t('cacheRate')">
          {{ formatting.formatPercent(event.cache_rate) }}
        </DetailKeyValue>
        <DetailKeyValue :label="t('upstream')">
          {{ event.target }}
        </DetailKeyValue>
        <DetailKeyValue :label="t('upstreamKey')">
          <span
            v-if="event.endpoint_key_label || event.endpoint_key_id"
            class="grid gap-0.5"
          >
            <span>{{ event.endpoint_key_label || '-' }}</span>
            <span v-if="event.endpoint_key_id" class="break-all text-dimmed">
              {{ event.endpoint_key_id }}
            </span>
          </span>
          <span v-else>-</span>
        </DetailKeyValue>
        <DetailKeyValue :label="t('requestCompression')">
          {{ requestCompressionText }}
        </DetailKeyValue>
        <DetailKeyValue :label="t('compressedBytes')">
          {{ compressedBytesText }}
        </DetailKeyValue>
        <DetailKeyValue :label="t('decompressedBytes')">
          {{ decompressedBytesText }}
        </DetailKeyValue>
        <DetailKeyValue :label="t('compressionRatio')">
          {{ compressionRatioText }}
        </DetailKeyValue>
      </div>
      <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-5">
        <DetailKeyValue :label="t('clientKey')">
          {{ event.client_key_label || '-' }}
        </DetailKeyValue>
        <DetailKeyValue :label="t('sessionState')">
          <UBadge
            :label="
              event.is_session_recognized
                ? t('sessionRecognized')
                : t('sessionUnrecognized')
            "
            :color="event.is_session_recognized ? 'success' : 'warning'"
          />
        </DetailKeyValue>
        <DetailKeyValue :label="t('sessionId')">
          <span class="break-all">{{ event.conversation_id || '-' }}</span>
        </DetailKeyValue>
        <DetailKeyValue :label="t('session')">
          <span class="flex flex-wrap gap-1">
            <UBadge :label="`#${event.conversation_seq}`" />
            <UBadge :label="t('firstTurn')" />
            <UBadge
              v-if="
                event.conversation_id &&
                !event.has_parent &&
                (event.conversation_seq ?? 0) > 1
              "
              :value="t('usageBranchStart')"
              color="warning"
            />
          </span>
        </DetailKeyValue>
        <DetailKeyValue :label="t('conversationSource')">
          {{ conversationSourceText }}
        </DetailKeyValue>
        <DetailKeyValue :label="t('clientInstallationId')">
          <span class="break-all">{{ installationIdText }}</span>
        </DetailKeyValue>
        <DetailKeyValue :label="t('normalizedItemCount')">
          {{ normalizedItemCountText }}
        </DetailKeyValue>
        <DetailKeyValue :label="t('storageSanitized')">
          {{
            event.storage_sanitized
              ? t('previousResponseIdPresent')
              : t('previousResponseIdMissing')
          }}
        </DetailKeyValue>
        <DetailKeyValue :label="t('hasPreviousResponseId')">
          {{
            event.request_has_previous_response_id
              ? t('previousResponseIdPresent')
              : t('previousResponseIdMissing')
          }}
        </DetailKeyValue>
        <DetailKeyValue :label="t('previousResponseId')">
          <span class="break-all">{{
            event.request_previous_response_id || '-'
          }}</span>
        </DetailKeyValue>
        <DetailKeyValue :label="t('parentFound')">
          {{
            event.request_previous_response_parent_found == null
              ? '-'
              : event.request_previous_response_parent_found
                ? t('parentFoundYes')
                : t('parentFoundNo')
          }}
        </DetailKeyValue>
        <DetailKeyValue :label="t('upstreamId')">
          <span class="break-all">{{ event.endpoint_id || '-' }}</span>
        </DetailKeyValue>
        <DetailKeyValue :label="t('providerResponseId')">
          <span class="break-all">{{ event.provider_response_id || '-' }}</span>
        </DetailKeyValue>
      </div>
    </div>
  </FlatSection>
</template>
