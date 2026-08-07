<script setup lang="ts">
import { computed } from 'vue'
import FlatSection from '@/components/shared/FlatSection.vue'
import type {
  ConversationEndpointOverrideView,
  Option,
  RequestRecordDetailView,
  SessionRouteOptionsView,
} from '@/models'
import type { SessionAffinityState } from '@/generated/admin-api'

const AUTOMATIC_KEY_VALUE = '__automatic__'

const props = defineProps<{
  event: RequestRecordDetailView
  conversationOverride: ConversationEndpointOverrideView | null
  overrideSaving: boolean
  affinityResetting: boolean
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

const affinityStateLabel = computed(() => {
  const state: SessionAffinityState | undefined =
    props.sessionRouteOptions?.affinity?.state
  switch (state) {
    case 'active':
      return props.t('bindingActive')
    case 'stale_endpoint':
    case 'stale_key':
      return props.t('bindingStale')
    default:
      return props.t('bindingUnbound')
  }
})

const affinityEndpointLabel = computed(() => {
  const affinity = props.sessionRouteOptions?.affinity
  return affinity?.endpoint_name || affinity?.endpoint_id || '-'
})

const affinityKeyLabel = computed(() => {
  const affinity = props.sessionRouteOptions?.affinity
  return displayKey(affinity?.key_label, affinity?.key_id)
})

const sessionRouteActionsBusy = computed(
  () => props.overrideSaving || props.affinityResetting,
)

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
            {{ t('recordRoute') }}:
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
            {{ t('sessionBinding') }}:
            <span
              :class="{
                'text-green-500':
                  sessionRouteOptions?.affinity?.state === 'active',
                'text-amber-500':
                  sessionRouteOptions?.affinity?.state === 'stale_endpoint' ||
                  sessionRouteOptions?.affinity?.state === 'stale_key',
              }"
            >
              {{ affinityStateLabel }}
            </span>
            <span v-if="sessionRouteOptions?.affinity?.endpoint_id"> / </span>
            <span
              v-if="sessionRouteOptions?.affinity?.endpoint_id"
              class="break-all"
            >
              {{ affinityEndpointLabel }}
            </span>
            <span v-if="sessionRouteOptions?.affinity?.endpoint_id"> / </span>
            <span
              v-if="sessionRouteOptions?.affinity?.endpoint_id"
              class="break-all"
            >
              {{ affinityKeyLabel }}
            </span>
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
            :disabled="!selectedOverrideEndpointId || sessionRouteActionsBusy"
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
            :disabled="!conversationOverride || sessionRouteActionsBusy"
            @click="emit('clearOverride')"
          >
            {{ t('clearOverride') }}
          </UButton>
          <UButton
            size="sm"
            class="lg:shrink-0"
            color="error"
            variant="outline"
            :loading="affinityResetting"
            :disabled="sessionRouteActionsBusy"
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
