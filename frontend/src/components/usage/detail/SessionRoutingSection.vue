<script setup lang="ts">
import { computed } from 'vue'
import FlatSection from '@/components/shared/FlatSection.vue'
import type {
  ConversationEndpointOverrideView,
  Option,
  RequestRecordDetailView,
  SessionRouteOptionsView,
} from '@/models'

const AUTOMATIC_KEY_VALUE = '__automatic__'

const props = defineProps<{
  event: RequestRecordDetailView
  conversationOverride: ConversationEndpointOverrideView | null
  overrideSaving: boolean
  options: Option[]
  sessionRouteOptions: SessionRouteOptionsView | null
  sessionRouteOptionsLoading: boolean
  t: TranslateFn
}>()

const selectedOverrideEndpointId = defineModel<string>(
  'selectedOverrideEndpointId',
  { required: true },
)
const selectedOverrideEndpointKeyId = defineModel<string>(
  'selectedOverrideEndpointKeyId',
  { required: true },
)

const selectedEndpointKeys = computed(() => [
  { label: props.t('autoRouteKey'), value: AUTOMATIC_KEY_VALUE },
  ...(props.sessionRouteOptions?.options
    .find((option) => option.endpoint_id === selectedOverrideEndpointId.value)
    ?.keys.map((key) => ({ label: key.key_label, value: key.key_id })) ?? []),
])

const emit = defineEmits<{
  saveOverride: []
  clearOverride: []
  resetAffinity: []
}>()

function resetEndpointKey(): void {
  selectedOverrideEndpointKeyId.value = AUTOMATIC_KEY_VALUE
}

function displayKey(
  keyLabel: string | null | undefined,
  keyId: string | null | undefined,
): string {
  return keyLabel || keyId || props.t('autoRouteKey')
}
</script>

<template>
  <FlatSection>
    <template #header>
      <div class="flex min-w-0 flex-1 flex-wrap items-center gap-x-4 gap-y-1">
        <h2
          class="m-0 text-[0.98rem] leading-[1.3] font-semibold text-highlighted"
        >
          {{ t('sessionRouting') }}
        </h2>
        <div
          class="flex min-w-0 flex-wrap items-center gap-x-4 gap-y-1 text-xs text-dimmed"
        >
          <span>
            {{ t('sessionId') }}:
            <span class="break-all">{{ event.conversation_id || '-' }}</span>
          </span>
          <span>
            {{ t('currentUpstream') }}:
            <span class="break-all">{{
              sessionRouteOptions?.current_upstream_label ||
              event.upstream_label
            }}</span>
            /
            <span class="break-all">{{
              displayKey(
                sessionRouteOptions?.current_endpoint_key_label,
                sessionRouteOptions?.current_endpoint_key_id,
              )
            }}</span>
          </span>
          <span>
            {{ t('currentOverride') }}:
            <span class="break-all">{{
              conversationOverride?.endpoint_name ||
              conversationOverride?.endpoint_id ||
              '-'
            }}</span>
            <span v-if="conversationOverride"> / </span>
            <span v-if="conversationOverride" class="break-all">{{
              displayKey(
                conversationOverride.endpoint_key_label,
                conversationOverride.endpoint_key_id,
              )
            }}</span>
          </span>
        </div>
      </div>
    </template>
    <div class="grid gap-2">
      <div v-if="sessionRouteOptions" class="grid gap-2">
        <div class="flex flex-col gap-2 lg:flex-row lg:items-center">
          <USelect
            v-model="selectedOverrideEndpointId"
            class="w-full lg:flex-1"
            size="sm"
            :items="options"
            label-key="label"
            value-key="value"
            :placeholder="t('selectUpstream')"
            @update:model-value="resetEndpointKey"
          />
          <USelect
            v-model="selectedOverrideEndpointKeyId"
            class="w-full lg:flex-1"
            size="sm"
            :items="selectedEndpointKeys"
            label-key="label"
            value-key="value"
            :placeholder="t('selectUpstreamKey')"
          />
          <UButton
            size="sm"
            class="lg:shrink-0"
            :loading="overrideSaving"
            :disabled="!selectedOverrideEndpointId"
            @click="emit('saveOverride')"
          >
            {{ t('setOverride') }}
          </UButton>
          <UButton
            size="sm"
            class="lg:shrink-0"
            color="neutral"
            variant="outline"
            :loading="overrideSaving"
            :disabled="!conversationOverride"
            @click="emit('clearOverride')"
          >
            {{ t('clearOverride') }}
          </UButton>
          <UButton
            size="sm"
            class="lg:shrink-0"
            color="error"
            variant="outline"
            :loading="overrideSaving"
            @click="emit('resetAffinity')"
          >
            {{ t('resetAffinity') }}
          </UButton>
        </div>
      </div>
      <div v-else-if="event.conversation_id" class="text-xs text-dimmed">
        {{
          sessionRouteOptionsLoading
            ? t('loading')
            : t('sessionRoutingUnavailable')
        }}
      </div>
    </div>
  </FlatSection>
</template>
