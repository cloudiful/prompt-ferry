<script setup lang="ts">
import { computed } from 'vue'
import DetailKeyValue from './DetailKeyValue.vue'
import FlatSection from '@/components/shared/FlatSection.vue'
import type { RequestRecordFullResponse } from '@/generated/admin-api'
import { deriveReasoningEffortDisplay } from '@/admin-mappers'
import type { RequestRecordDetailView } from '@/models'

const props = defineProps<{
  event: RequestRecordDetailView
  requestFull: RequestRecordFullResponse | null
  t: TranslateFn
}>()

const reasoningEffort = computed(() => {
  const candidates: unknown[] = [
    props.event.request_raw_json,
    props.requestFull?.request_raw_json,
  ]
  for (const candidate of candidates) {
    const effort = extractReasoningEffort(candidate)
    if (effort) return effort
  }
  return null
})

const reasoningEffortDisplay = computed(() =>
  deriveReasoningEffortDisplay(
    reasoningEffort.value,
    props.event.applied_thinking_effort_override,
    props.event.path,
  ),
)

function extractReasoningEffort(value: unknown): string | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const record = value as Record<string, unknown>
  const direct = record['reasoning_effort']
  if (typeof direct === 'string' && direct.trim().length > 0) {
    return direct.trim()
  }
  const reasoning = record['reasoning']
  if (reasoning && typeof reasoning === 'object' && !Array.isArray(reasoning)) {
    const effort = (reasoning as Record<string, unknown>)['effort']
    if (typeof effort === 'string' && effort.trim().length > 0) {
      return effort.trim()
    }
  }
  return null
}
</script>

<template>
  <FlatSection :title="t('requestContext')">
    <div class="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
      <DetailKeyValue :label="t('model')">
        {{ event.model_display }}
      </DetailKeyValue>
      <DetailKeyValue :label="t('upstream')">
        {{ event.target }}
      </DetailKeyValue>
      <DetailKeyValue :label="t('upstreamId')">
        <span class="break-all">{{ event.endpoint_id || '-' }}</span>
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
      <DetailKeyValue :label="t('clientKey')">
        {{ event.client_key_label || '-' }}
      </DetailKeyValue>
      <DetailKeyValue :label="t('conversationId')">
        <span class="break-all">{{ event.conversation_id || '-' }}</span>
      </DetailKeyValue>
      <DetailKeyValue :label="t('conversationSeq')">
        <span
          v-if="event.conversation_seq != null"
          class="flex flex-wrap gap-1"
        >
          <UBadge :label="`#${event.conversation_seq}`" />
          <UBadge v-if="event.is_first_turn" :label="t('firstTurn')" />
        </span>
        <span v-else>-</span>
      </DetailKeyValue>
      <DetailKeyValue
        v-if="reasoningEffortDisplay"
        :label="t('reasoningEffort')"
      >
        {{ reasoningEffortDisplay }}
      </DetailKeyValue>
      <!-- Reasoning effort is hidden when unavailable: neither the record nor
        the stored request payload carries reasoning_effort / reasoning.effort.
        Issue #546: a route target override renders as `caller → override`. -->
    </div>
  </FlatSection>
</template>
