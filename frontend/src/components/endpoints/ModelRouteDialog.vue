<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import type { ModelRouteForm } from '@/models'
import type { ProviderEndpoint, User } from '@/generated/admin-api'

const props = defineProps<{
  busy: boolean
  endpoints: ProviderEndpoint[]
  endpointOptions: Array<{ label: string; value: string }>
  header: string
  t: TranslateFn
  users: User[]
}>()

const visible = defineModel<boolean>('visible', { required: true })
const form = defineModel<ModelRouteForm>('form', { required: true })

defineEmits<{
  save: []
}>()

function addTarget(): void {
  form.value.targets.push({
    endpoint_id: '',
    enabled: true,
    upstream_model: '',
    responses_continuation_policy: 'force_replay',
    chat_reasoning_replay_policy: 'auto',
  })
}

function removeTarget(index: number): void {
  form.value.targets.splice(index, 1)
}

function moveTarget(index: number, offset: -1 | 1): void {
  const targetIndex = index + offset
  if (targetIndex < 0 || targetIndex >= form.value.targets.length) return
  const [target] = form.value.targets.splice(index, 1)
  if (target) form.value.targets.splice(targetIndex, 0, target)
}

function endpointForTarget(endpointId: string): ProviderEndpoint | undefined {
  return props.endpoints.find((endpoint) => endpoint.endpoint_id === endpointId)
}

function defaultContinuationPolicy(
  endpointId: string,
): 'force_passthrough' | 'force_replay' {
  return ['responses', 'auto'].includes(
    endpointForTarget(endpointId)?.native_api ?? '',
  )
    ? 'force_passthrough'
    : 'force_replay'
}

function onTargetEndpointChange(
  target: ModelRouteForm['targets'][number],
): void {
  target.responses_continuation_policy = defaultContinuationPolicy(
    target.endpoint_id,
  )
}

function canUseForcePassthrough(endpointId: string): boolean {
  return ['responses', 'auto'].includes(
    endpointForTarget(endpointId)?.native_api ?? '',
  )
}

function continuationPolicyOptionsFor(endpointId: string): Array<{
  label: string
  value: 'force_passthrough' | 'force_replay'
}> {
  return canUseForcePassthrough(endpointId)
    ? continuationPolicyOptions.value
    : continuationPolicyOptions.value.filter(
        (option) => option.value === 'force_replay',
      )
}

const routingStrategyOptions = computed(() => [
  {
    label: props.t('routingStrategyClientKey'),
    value: 'client_key_rendezvous',
  },
  {
    label: props.t('routingStrategySessionAffinity'),
    value: 'responses_session_affinity',
  },
])

const continuationPolicyOptions = computed(
  (): Array<{
    label: string
    value: 'force_passthrough' | 'force_replay'
  }> => [
    { label: props.t('continuationPolicyForceReplay'), value: 'force_replay' },
    {
      label: props.t('continuationPolicyForcePassthrough'),
      value: 'force_passthrough',
    },
  ],
)

const reasoningReplayPolicyOptions = computed(
  (): Array<{
    label: string
    value: 'auto' | 'force_replay' | 'force_passthrough'
  }> => [
    { label: props.t('reasoningReplayPolicyAuto'), value: 'auto' },
    {
      label: props.t('reasoningReplayPolicyForceReplay'),
      value: 'force_replay',
    },
    {
      label: props.t('reasoningReplayPolicyForcePassthrough'),
      value: 'force_passthrough',
    },
  ],
)

const targetColumns = computed<
  TableColumn<ModelRouteForm['targets'][number]>[]
>(() => [
  { id: 'order' },
  { id: 'endpoint', header: props.t('endpoint') },
  { id: 'status', header: props.t('status') },
  { id: 'continuation', header: props.t('continuationPolicy') },
  { id: 'reasoning', header: props.t('reasoningReplayPolicy') },
  { id: 'actions' },
])
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="header"
    :ui="{ content: 'sm:max-w-4xl' }"
  >
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('save')">
        <div
          class="grid gap-3 xl:grid-cols-[9rem_minmax(0,1.2fr)_minmax(0,1.1fr)_auto]"
        >
          <USelect
            v-model="form.scope"
            class="w-full"
            :items="['admin', 'user']"
          />
          <UInput
            v-model="form.model_pattern"
            class="w-full"
            :placeholder="t('modelPattern')"
          />
          <USelect
            v-model="form.routing_strategy"
            class="w-full"
            :items="routingStrategyOptions"
            label-key="label"
            value-key="value"
          />
          <label
            class="inline-flex min-h-8 items-center justify-end gap-2 self-end pb-1 text-[0.75rem] text-default"
          >
            <span class="text-xs text-dimmed">{{
              form.enabled ? t('active') : t('disabled')
            }}</span>
            <USwitch v-model="form.enabled" />
          </label>
        </div>
        <div class="grid gap-3 md:grid-cols-3">
          <label class="grid gap-1">
            <span class="text-xs text-muted">{{ t('dailyRequestLimit') }}</span>
            <UInputNumber
              v-model="form.daily_max_requests"
              class="w-full"
              size="sm"
              :min="1"
              :use-grouping="false"
            />
          </label>
          <label class="grid gap-1">
            <span class="text-xs text-muted">{{
              t('monthlyRequestLimit')
            }}</span>
            <UInputNumber
              v-model="form.monthly_max_requests"
              class="w-full"
              size="sm"
              :min="1"
              :use-grouping="false"
            />
          </label>
        </div>
        <USelect
          v-if="form.scope === 'user'"
          :model-value="form.owner_user_id ?? undefined"
          class="w-full xl:max-w-[16rem]"
          :items="users"
          label-key="login_name"
          value-key="user_id"
          :placeholder="t('ownerUser')"
          @update:model-value="form.owner_user_id = $event ?? null"
        />

        <div class="grid gap-2">
          <div class="flex items-center justify-between gap-3">
            <div class="text-sm font-medium text-highlighted">
              {{ t('target') }}
            </div>
            <UButton type="button" size="sm" color="neutral" @click="addTarget"
              ><UIcon name="i-lucide-plus" class="h-4 w-4" />{{
                t('addTarget')
              }}</UButton
            >
          </div>
          <UTable :data="form.targets" :columns="targetColumns" class="min-w-0">
            <template #order-cell="{ row }">
              <div class="flex gap-1">
                <UButton
                  icon="i-lucide-chevron-up"
                  color="neutral"
                  variant="ghost"
                  size="xs"
                  :disabled="row.index === 0"
                  @click="moveTarget(row.index, -1)"
                />
                <UButton
                  icon="i-lucide-chevron-down"
                  color="neutral"
                  variant="ghost"
                  size="xs"
                  :disabled="row.index === form.targets.length - 1"
                  @click="moveTarget(row.index, 1)"
                />
              </div>
            </template>
            <template #endpoint-cell="{ row }">
              <div
                class="grid gap-2 md:grid-cols-[minmax(0,1.25fr)_minmax(0,1fr)]"
              >
                <USelect
                  v-model="row.original.endpoint_id"
                  class="w-full"
                  :items="endpointOptions"
                  label-key="label"
                  value-key="value"
                  :placeholder="t('endpoint')"
                  @update:model-value="onTargetEndpointChange(row.original)"
                />
                <UInput
                  v-model="row.original.upstream_model"
                  class="w-full"
                  :placeholder="t('upstreamModelOptional')"
                />
              </div>
            </template>
            <template #status-cell="{ row }">
              <label
                class="inline-flex min-h-8 items-center justify-center gap-2 text-[0.75rem] text-default"
                ><UCheckbox v-model="row.original.enabled" />{{
                  row.original.enabled ? t('active') : t('disabled')
                }}</label
              >
            </template>
            <template #continuation-cell="{ row }">
              <USelect
                v-model="row.original.responses_continuation_policy"
                class="w-full"
                :items="continuationPolicyOptionsFor(row.original.endpoint_id)"
                label-key="label"
                value-key="value"
              />
            </template>
            <template #reasoning-cell="{ row }">
              <USelect
                v-model="row.original.chat_reasoning_replay_policy"
                class="w-full"
                :items="reasoningReplayPolicyOptions"
                label-key="label"
                value-key="value"
              />
            </template>
            <template #actions-cell="{ row }">
              <UButton
                type="button"
                size="sm"
                color="error"
                variant="ghost"
                @click="removeTarget(row.index)"
                ><UIcon name="i-lucide-trash-2" class="h-4 w-4"
              /></UButton>
            </template>
          </UTable>
        </div>

        <div class="flex justify-end gap-2 pt-1">
          <UButton
            type="button"
            size="sm"
            color="neutral"
            @click="
              () => {
                visible = false
              }
            "
            >{{ t('cancel') }}</UButton
          >
          <UButton type="submit" size="sm" :loading="busy"
            ><UIcon name="i-lucide-save" class="h-4 w-4" />{{
              t('save')
            }}</UButton
          >
        </div>
      </form>
    </template>
  </UModal>
</template>
