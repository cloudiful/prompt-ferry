<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import type { Option } from '@/models'
import type { UsageDetailWorkspaceView } from '@/models/usage'
import RequestContextSection from './RequestContextSection.vue'
import SessionRoutingSection from './SessionRoutingSection.vue'
import UsageErrorSection from './UsageErrorSection.vue'
import UsageTranscriptSection from './UsageTranscriptSection.vue'

const AUTOMATIC_KEY_VALUE = '__automatic__'

const props = defineProps<{
  detail: UsageDetailWorkspaceView
  t: TranslateFn
}>()

const emit = defineEmits<{
  saveConversationOverride: [
    selection: {
      endpointId: string
      endpointKeyId: string | null
    },
  ]
  clearConversationOverride: []
  resetSessionAffinity: []
  loadRequestFull: []
  loadMoreRequestFull: []
}>()

const visible = defineModel<boolean>('visible', { required: true })
const selectedOverrideEndpointId = ref('')
const selectedOverrideEndpointKeyId = ref(AUTOMATIC_KEY_VALUE)

const routeOptionChoices = computed<Option[]>(
  () =>
    props.detail.session_route_options?.options.map((option) => ({
      label: option.endpoint_name,
      value: option.endpoint_id,
    })) ?? [],
)
const showSessionRouting = computed(
  () =>
    Boolean(props.detail.record?.conversation_id) ||
    Boolean(props.detail.session_route_options) ||
    Boolean(props.detail.conversation_override),
)

watch(visible, (nextVisible) => {
  if (!nextVisible) {
    selectedOverrideEndpointId.value = ''
    selectedOverrideEndpointKeyId.value = AUTOMATIC_KEY_VALUE
  }
})

watch(
  () => props.detail.session_route_options?.override_endpoint_id,
  (value) => {
    selectedOverrideEndpointId.value = value || ''
  },
  { immediate: true },
)

watch(
  () => props.detail.session_route_options?.override_endpoint_key_id,
  (value) => {
    selectedOverrideEndpointKeyId.value = value || AUTOMATIC_KEY_VALUE
  },
  { immediate: true },
)

function saveOverride(): void {
  if (!selectedOverrideEndpointId.value) return
  emit('saveConversationOverride', {
    endpointId: selectedOverrideEndpointId.value,
    endpointKeyId:
      selectedOverrideEndpointKeyId.value === AUTOMATIC_KEY_VALUE
        ? null
        : selectedOverrideEndpointKeyId.value,
  })
}
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="t('requestDetails')"
    :ui="{ content: 'sm:max-w-6xl', body: 'max-h-[80vh] overflow-y-auto' }"
  >
    <template #body>
      <div v-if="detail.record" class="grid gap-3 text-xs">
        <div class="grid gap-3">
          <RequestContextSection
            :event="detail.record"
            :request-full="detail.request_full"
            :t="t"
          />
          <SessionRoutingSection
            v-if="showSessionRouting"
            v-model:selected-override-endpoint-id="selectedOverrideEndpointId"
            v-model:selected-override-endpoint-key-id="
              selectedOverrideEndpointKeyId
            "
            :event="detail.record"
            :conversation-override="detail.conversation_override"
            :override-saving="detail.override_saving"
            :affinity-resetting="detail.affinity_resetting"
            :options="routeOptionChoices"
            :session-route-options="detail.session_route_options"
            :session-route-options-loading="
              detail.session_route_options_loading
            "
            :t="t"
            @save-override="saveOverride"
            @clear-override="emit('clearConversationOverride')"
            @reset-affinity="emit('resetSessionAffinity')"
          />
        </div>
        <UsageErrorSection :event="detail.record" :t="t" :visible="visible" />
        <UsageTranscriptSection
          :detail-loading="detail.detail_loading"
          :event="detail.record"
          :request-full="detail.request_full"
          :request-full-loading="detail.request_full_loading"
          :request-full-loading-more="detail.request_full_loading_more"
          :t="t"
          :visible="visible"
          @load-request-full="emit('loadRequestFull')"
          @load-more-request-full="emit('loadMoreRequestFull')"
        />
      </div>
      <div
        v-else-if="detail.detail_loading"
        class="grid gap-2 py-6 text-xs text-dimmed"
      >
        <div>{{ t('loading') }}</div>
      </div>
    </template>
  </UModal>
</template>
