<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import ProviderIcon from '@/components/providers/ProviderIcon.vue'
import ModelRouteTargetSettingsPage from '@/components/endpoints/ModelRouteTargetSettingsPage.vue'
import {
  canSaveTargets,
  createRoutingOptions,
  createTargetColumns,
  createTargetMeta,
  hasTargetSettings,
  targetProxySummary,
  targetScheduleSummary,
  targetScheduleTooltip,
} from '@/components/endpoints/modelRouteTargetHelpers'
import { useModelRouteTargetDrag } from '@/components/endpoints/useModelRouteTargetDrag'
import type { ModelRouteForm } from '@/models'
import type { ProviderEndpoint, User } from '@/generated/admin-api'
import type { EndpointOption } from '@/models/endpoints'

const props = defineProps<{
  busy: boolean
  endpoints: ProviderEndpoint[]
  endpointOptions: EndpointOption[]
  header: string
  t: TranslateFn
  users: User[]
}>()

const visible = defineModel<boolean>('visible', { required: true })
const form = defineModel<ModelRouteForm>('form', { required: true })

defineEmits<{
  save: []
}>()

// Whole-dialog two-level drill: main route form plus a per-target settings
// sub-page. Gear swaps the entire modal content; back returns to main.
// Draft lives in `form` so edits persist across view switches and save
// through the existing outer save button.
const view = ref<'main' | 'targetSettings'>('main')
const selectedTargetIndex = ref<number | null>(null)

function openTargetSettings(index: number): void {
  selectedTargetIndex.value = index
  view.value = 'targetSettings'
}

function backToMain(): void {
  view.value = 'main'
  selectedTargetIndex.value = null
}

watch(visible, (open) => {
  if (!open) {
    view.value = 'main'
    selectedTargetIndex.value = null
  }
})

function ensureTargets(): ModelRouteForm['targets'] {
  if (!form.value) return []
  if (!Array.isArray(form.value.targets)) form.value.targets = []
  return form.value.targets
}

function addTarget(): void {
  if (!form.value) return
  ensureTargets().push({
    endpoint_id: '',
    enabled: true,
    upstream_model: '',
    proxy_url_override: '',
    has_saved_proxy_url_override: false,
    active_windows: [],
    active_windows_touched: false,
    dev_system_normalize: false,
    // Issue #464: inherit (follow caller) by default.
    thinking_effort_override: null,
    // Issue #409: default Auto (follow caller), like createEmptyModelRouteForm.
    native_api: 'auto',
  })
}

function removeTarget(index: number): void {
  if (!form.value || !Array.isArray(form.value.targets)) return
  form.value.targets.splice(index, 1)
  if (selectedTargetIndex.value != null) {
    if (selectedTargetIndex.value === index) {
      view.value = 'main'
      selectedTargetIndex.value = null
    } else if (selectedTargetIndex.value > index) {
      selectedTargetIndex.value -= 1
    }
  }
}

const selectedTargetModel = computed({
  get: () => form.value?.targets?.[selectedTargetIndex.value ?? -1] ?? null,
  set: (value: ModelRouteForm['targets'][number] | null) => {
    if (selectedTargetIndex.value == null || !value) return
    if (form.value?.targets)
      form.value.targets[selectedTargetIndex.value] = value
  },
})

const selectedEndpointLabel = computed(() => {
  const target = selectedTargetModel.value
  if (!target) return ''
  return (
    props.endpointOptions.find((item) => item.value === target.endpoint_id)
      ?.label ?? ''
  )
})

const canSaveOuter = computed(() => canSaveTargets(form.value?.targets ?? []))

const {
  dragOverIndex,
  onRowDragStart,
  onRowDragOver,
  onDrop,
  onDragEnd,
  onGripKeydown,
} = useModelRouteTargetDrag(form)

const routingStrategyOptions = computed(() => createRoutingOptions(props.t))
const targetColumns = computed(() => createTargetColumns(props.t))
const targetTableMeta = computed(() => createTargetMeta(dragOverIndex))
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="header"
    :ui="{ content: 'sm:max-w-4xl' }"
  >
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('save')">
        <template v-if="view === 'main'">
          <div class="flex flex-wrap items-end gap-3">
            <USelect
              v-model="form.scope"
              class="w-28 shrink-0"
              :items="['admin', 'user']"
            />
            <UInput
              v-model="form.model_pattern"
              class="min-w-40 flex-1"
              :placeholder="t('modelPattern')"
            />
            <USelect
              v-model="form.routing_strategy"
              class="min-w-44 flex-1"
              :items="routingStrategyOptions"
              label-key="label"
              value-key="value"
            />
            <label
              class="inline-flex min-h-8 shrink-0 items-center justify-end pb-1 text-[0.75rem] text-default"
            >
              <USwitch v-model="form.enabled" :aria-label="t('status')" />
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
              <UButton
                type="button"
                size="sm"
                color="neutral"
                @click="addTarget"
                ><UIcon name="i-lucide-plus" class="h-4 w-4" />{{
                  t('addTarget')
                }}</UButton
              >
            </div>
            <UTable
              :data="form?.targets ?? []"
              :columns="targetColumns"
              :meta="targetTableMeta"
              class="min-w-0"
              :ui="{ th: 'whitespace-nowrap' }"
            >
              <template #order-cell="{ row }">
                <div
                  class="flex items-center gap-1"
                  draggable="true"
                  @dragstart="onRowDragStart($event, row.index)"
                  @dragover="onRowDragOver($event, row.index)"
                  @drop="onDrop($event, row.index)"
                  @dragend="onDragEnd"
                >
                  <div
                    data-target-drag-handle
                    tabindex="0"
                    role="button"
                    class="inline-flex cursor-grab items-center justify-center rounded p-1 text-muted focus-visible:outline-2 focus-visible:outline-primary active:cursor-grabbing"
                    :aria-label="t('target')"
                    @keydown="onGripKeydown($event, row.index)"
                  >
                    <UIcon name="i-lucide-grip-vertical" class="h-4 w-4" />
                  </div>
                </div>
              </template>
              <template #endpoint-cell="{ row }">
                <div
                  class="grid gap-2"
                  draggable="true"
                  @dragstart="onRowDragStart($event, row.index)"
                  @dragover="onRowDragOver($event, row.index)"
                  @drop="onDrop($event, row.index)"
                  @dragend="onDragEnd"
                >
                  <div
                    class="grid gap-2 md:grid-cols-[minmax(0,1.25fr)_minmax(0,1fr)_auto]"
                  >
                    <USelect
                      v-model="row.original.endpoint_id"
                      class="w-full"
                      :items="endpointOptions"
                      label-key="label"
                      value-key="value"
                      :placeholder="t('endpoint')"
                    >
                      <template #item-leading="{ item }">
                        <ProviderIcon :provider="item.provider" size="sm" />
                      </template>
                    </USelect>
                    <UInput
                      v-model="row.original.upstream_model"
                      class="w-full"
                      :placeholder="t('upstreamModelOptional')"
                    />
                    <div class="flex items-center gap-1">
                      <div
                        class="hidden shrink-0 items-center gap-1 text-muted lg:flex"
                      >
                        <span>{{ targetProxySummary(row.original, t) }}</span>
                        <span aria-hidden="true">·</span>
                        <UTooltip
                          :text="targetScheduleTooltip(row.original, t)"
                        >
                          <span>{{
                            targetScheduleSummary(row.original, t)
                          }}</span>
                        </UTooltip>
                      </div>
                      <UButton
                        type="button"
                        size="sm"
                        :color="
                          hasTargetSettings(row.original)
                            ? 'primary'
                            : 'neutral'
                        "
                        variant="ghost"
                        icon="i-lucide-settings-2"
                        :aria-label="t('targetSettings')"
                        :aria-pressed="hasTargetSettings(row.original)"
                        :title="t('targetSettingsHint')"
                        @click="openTargetSettings(row.index)"
                      />
                    </div>
                  </div>
                  <div class="flex items-center gap-1 text-muted lg:hidden">
                    <span>{{ targetProxySummary(row.original, t) }}</span>
                    <span aria-hidden="true">·</span>
                    <span>{{ targetScheduleSummary(row.original, t) }}</span>
                  </div>
                </div>
              </template>
              <template #status-cell="{ row }">
                <div
                  class="flex min-h-8 items-center justify-center"
                  draggable="true"
                  @dragstart="onRowDragStart($event, row.index)"
                  @dragover="onRowDragOver($event, row.index)"
                  @drop="onDrop($event, row.index)"
                  @dragend="onDragEnd"
                >
                  <USwitch
                    v-model="row.original.enabled"
                    :aria-label="t('status')"
                  />
                </div>
              </template>
              <template #actions-cell="{ row }">
                <div
                  class="flex items-center"
                  draggable="true"
                  @dragstart="onRowDragStart($event, row.index)"
                  @dragover="onRowDragOver($event, row.index)"
                  @drop="onDrop($event, row.index)"
                  @dragend="onDragEnd"
                >
                  <UButton
                    type="button"
                    size="sm"
                    color="error"
                    variant="ghost"
                    :aria-label="t('delete')"
                    @click="removeTarget(row.index)"
                    ><UIcon name="i-lucide-trash-2" class="h-4 w-4"
                  /></UButton>
                </div>
              </template>
            </UTable>
          </div>
        </template>
        <template v-else>
          <div class="flex items-center gap-1">
            <UButton
              type="button"
              size="xs"
              color="neutral"
              variant="ghost"
              icon="i-lucide-arrow-left"
              :aria-label="t('cancel')"
              @click="backToMain"
            />
            <span class="text-sm font-medium text-default">{{
              t('targetSettings')
            }}</span>
            <span v-if="selectedEndpointLabel" class="truncate text-muted">
              · {{ selectedEndpointLabel }}
            </span>
          </div>
          <ModelRouteTargetSettingsPage
            v-if="selectedTargetModel"
            v-model:target="selectedTargetModel"
            :t="t"
          />
        </template>
        <div class="flex justify-end gap-2 pt-1">
          <UButton
            type="button"
            size="sm"
            color="neutral"
            @click="visible = false"
            >{{ t('cancel') }}</UButton
          >
          <UButton
            type="submit"
            size="sm"
            :loading="busy"
            :disabled="!canSaveOuter"
            ><UIcon name="i-lucide-save" class="h-4 w-4" />{{
              t('save')
            }}</UButton
          >
        </div>
      </form>
    </template>
  </UModal>
</template>
