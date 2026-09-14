<script setup lang="ts">
import { computed } from 'vue'
import FlatSection from '@/components/shared/FlatSection.vue'
import JsonSyntaxBlock from '@/components/usage/detail/JsonSyntaxBlock.vue'
import UsageErrorSection from '@/components/usage/detail/UsageErrorSection.vue'
import type { RequestRecordDetailView } from '@/models'
import type { RequestRecordFormatting } from '@/models/request-record-formatting'
import MarkdownLog from '@/components/shared/MarkdownLog.vue'
import { copyText } from '@/composables/useClipboard'

const props = defineProps<{
  detailLoading: boolean
  event: RequestRecordDetailView | null
  formatting: RequestRecordFormatting
  t: TranslateFn
}>()

const visible = defineModel<boolean>('visible', { required: true })

const hasRequestPayload = computed(() => props.event?.request_raw_json != null)
const requestJsonText = computed(() =>
  hasRequestPayload.value
    ? (JSON.stringify(props.event?.request_raw_json, null, 2) ?? '')
    : '',
)
const responseJsonValue = computed(() => {
  const raw = props.event?.response_prompt
  if (!raw || raw.trim().length === 0) return null
  const trimmed = raw.trim()
  if (trimmed.startsWith('{') || trimmed.startsWith('[')) {
    try {
      return JSON.parse(trimmed) as unknown
    } catch {
      return raw
    }
  }
  return raw
})
const responseJsonText = computed(() =>
  responseJsonValue.value != null
    ? (JSON.stringify(responseJsonValue.value, null, 2) ?? '')
    : '',
)
const hasResponsePayload = computed(() => responseJsonValue.value != null)
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="t('requestDetails')"
    :ui="{ content: 'sm:max-w-5xl', body: 'max-h-[80vh] overflow-y-auto' }"
  >
    <template #body>
      <div v-if="event" class="grid gap-3 text-xs">
        <div class="grid items-stretch gap-3 lg:grid-cols-2">
          <FlatSection :title="t('mcpRequestPayload')">
            <div class="grid gap-2">
              <div class="flex flex-wrap gap-2">
                <UButton
                  v-if="hasRequestPayload"
                  size="sm"
                  color="neutral"
                  variant="ghost"
                  @click="copyText(requestJsonText)"
                >
                  {{ t('copy') }}
                </UButton>
              </div>
              <JsonSyntaxBlock
                v-if="hasRequestPayload"
                :value="event.request_raw_json"
              />
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
                v-if="hasResponsePayload"
                size="sm"
                color="neutral"
                variant="ghost"
                class="justify-self-start"
                @click="copyText(responseJsonText)"
              >
                {{ t('copy') }}
              </UButton>
              <JsonSyntaxBlock
                v-if="hasResponsePayload"
                :value="responseJsonValue"
              />
              <MarkdownLog
                v-else
                text=""
                :empty-text="
                  detailLoading ? t('loading') : t('contentLoggingOff')
                "
                max-height="18rem"
              />
            </div>
          </FlatSection>
        </div>

        <UsageErrorSection :event="event" :t="t" :visible="visible" />
      </div>
    </template>
  </UModal>
</template>
