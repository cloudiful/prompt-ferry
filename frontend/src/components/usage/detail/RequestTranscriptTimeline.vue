<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import JsonSyntaxBlock from '@/components/usage/detail/JsonSyntaxBlock.vue'
import type {
  RequestRecordDetail,
  RequestRecordFullMessage,
  RequestRecordFullResponse,
} from '@/generated/admin-api'

type TimelineItem = {
  id: string
  kind: 'request' | 'assistant_message' | 'output_items' | 'response_prompt'
  label: string
  timestampLabel: string
  value: unknown
}

const props = defineProps<{
  detail: RequestRecordDetail | null
  requestFull: RequestRecordFullResponse | null
  requestFullLoading: boolean
  responsePendingText: string
  t: TranslateFn
}>()

const expandedIds = ref<string[]>([])

const items = computed<TimelineItem[]>(() => {
  const merged: TimelineItem[] = []
  const messages = props.requestFull?.messages ?? []

  messages.forEach((message, index) => {
    merged.push({
      id: `request-${message.block_hash || index}`,
      kind: 'request',
      label: buildRequestLabel(message, index),
      timestampLabel: buildRequestMeta(message, index),
      value: message.content_json ?? message.preview_text ?? null,
    })
  })

  const rawJson =
    props.requestFull?.request_raw_json ?? props.detail?.request_raw_json
  if (!messages.length && rawJson != null) {
    merged.push({
      id: 'request-raw-json',
      kind: 'request',
      label: props.t('requestRawJson'),
      timestampLabel:
        props.requestFull?.request_storage_mode ||
        props.detail?.request_storage_mode ||
        'request',
      value: rawJson,
    })
  }

  if (props.detail?.assistant_message_json != null) {
    merged.push({
      id: 'assistant-message',
      kind: 'assistant_message',
      label: 'assistant · message',
      timestampLabel: props.detail.provider_response_id || 'assistant_message',
      value: props.detail.assistant_message_json,
    })
  }

  if (props.detail?.assistant_output_items_json != null) {
    merged.push({
      id: 'assistant-output-items',
      kind: 'output_items',
      label: 'assistant · output_items',
      timestampLabel: Array.isArray(props.detail.assistant_output_items_json)
        ? `${props.detail.assistant_output_items_json.length} items`
        : 'output_items',
      value: props.detail.assistant_output_items_json,
    })
  }

  if (
    merged.length === 0 &&
    props.detail?.response_prompt &&
    props.detail.response_prompt.trim().length > 0
  ) {
    merged.push({
      id: 'response-prompt',
      kind: 'response_prompt',
      label: 'assistant · response_prompt',
      timestampLabel: props.t('requestStateUpstreamProcessing'),
      value: { response_prompt: props.detail.response_prompt },
    })
  }

  return merged
})

watch(
  items,
  (nextItems) => {
    const nextSet = new Set(nextItems.map((item) => item.id))
    expandedIds.value = expandedIds.value.filter((id) => nextSet.has(id))
  },
  { immediate: true },
)

function buildRequestLabel(
  message: RequestRecordFullMessage,
  index: number,
): string {
  return `${message.role || 'message'} · ${message.block_hash || index + 1}`
}

function buildRequestMeta(
  message: RequestRecordFullMessage,
  index: number,
): string {
  if (message.same_as_turn != null)
    return `${props.t('sameAsTurn')} ${message.same_as_turn}`
  if (message.preview_text?.trim()) return message.preview_text.trim()
  return `#${index + 1}`
}

function isExpanded(id: string): boolean {
  return expandedIds.value.includes(id)
}

function toggleItem(id: string): void {
  expandedIds.value = isExpanded(id)
    ? expandedIds.value.filter((value) => value !== id)
    : [...expandedIds.value, id]
}
</script>

<template>
  <div class="grid gap-3">
    <div class="text-xs font-semibold text-muted">
      {{ t('rawMessages') }}
    </div>
    <div
      v-if="requestFullLoading && !requestFull && !items.length"
      class="text-xs text-dimmed"
    >
      {{ t('requestFullLoading') }}
    </div>
    <div v-else-if="!items.length" class="text-xs text-dimmed">
      {{ responsePendingText || t('noJsonViewData') }}
    </div>
    <div v-else class="grid gap-2">
      <div
        v-for="item in items"
        :key="item.id"
        class="overflow-hidden rounded-lg border border-default bg-default"
      >
        <button
          type="button"
          class="grid w-full grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-3 border-none bg-transparent px-4 py-3 text-left"
          @click="toggleItem(item.id)"
        >
          <UIcon
            name="i-lucide-chevron-right"
            class="h-4 w-4 text-dimmed transition-transform"
            :class="{ 'rotate-90': isExpanded(item.id) }"
          />
          <div class="min-w-0 truncate text-sm font-medium text-highlighted">
            {{ item.label }}
          </div>
          <div class="truncate text-xs text-dimmed">
            {{ item.timestampLabel }}
          </div>
        </button>
        <div
          v-if="isExpanded(item.id)"
          class="border-t border-default px-4 py-3"
        >
          <JsonSyntaxBlock :value="item.value" />
        </div>
      </div>
    </div>
  </div>
</template>
