<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed, ref } from 'vue'
import ProviderIcon from '@/components/providers/ProviderIcon.vue'
import ModelRouteTargetSettings from '@/components/endpoints/ModelRouteTargetSettings.vue'
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
    // Issue #368 Phase C: masked per-target override; empty + no saved
    // means inherit.
    proxy_url_override: '',
    has_saved_proxy_url_override: false,
    // Issue #378 Phase J: untouched all-day schedule.
    active_windows: [],
    active_windows_touched: false,
    // Issue #392 Phase L: default-off normalize.
    dev_system_normalize: false,
  })
}

function removeTarget(index: number): void {
  if (!form.value || !Array.isArray(form.value.targets)) return
  form.value.targets.splice(index, 1)
}

// Issue #378 Phase H: grip-only drag reorder reuses the same splice
// move-order logic. `draggable` sits on each row-cell container so the whole
// row is the drop target; `dragstart` is gated to the grip handle.
const dragFromIndex = ref<number | null>(null)
const dragOverIndex = ref<number | null>(null)

function moveTargetTo(from: number, to: number): void {
  const targets = form.value?.targets
  if (!Array.isArray(targets)) return
  if (from < 0 || from >= targets.length) return
  if (to < 0 || to >= targets.length) return
  if (from === to) return
  const [target] = targets.splice(from, 1)
  if (target) targets.splice(to, 0, target)
}

function isGripTarget(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null
  return element?.closest?.('[data-target-drag-handle]') != null
}

function onRowDragStart(event: DragEvent, index: number): void {
  if (!isGripTarget(event.target)) {
    event.preventDefault()
    return
  }
  dragFromIndex.value = index
  if (event.dataTransfer) {
    event.dataTransfer.setData('text/plain', String(index))
    event.dataTransfer.effectAllowed = 'move'
  }
}

function onRowDragOver(event: DragEvent, index: number): void {
  event.preventDefault()
  dragOverIndex.value = index
  if (event.dataTransfer) event.dataTransfer.dropEffect = 'move'
}

function onDrop(event: DragEvent, index: number): void {
  event.preventDefault()
  const raw = event.dataTransfer?.getData('text/plain')
  const from = dragFromIndex.value ?? (raw ? Number(raw) : NaN)
  dragFromIndex.value = null
  dragOverIndex.value = null
  if (!Number.isInteger(from)) return
  moveTargetTo(from as number, index)
}

function onDragEnd(): void {
  dragFromIndex.value = null
  dragOverIndex.value = null
}

function onGripKeydown(event: KeyboardEvent, index: number): void {
  if (event.key === 'ArrowUp') {
    event.preventDefault()
    moveTargetTo(index, index - 1)
  } else if (event.key === 'ArrowDown') {
    event.preventDefault()
    moveTargetTo(index, index + 1)
  }
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

const targetColumns = computed<
  TableColumn<ModelRouteForm['targets'][number]>[]
>(() => [
  { id: 'order' },
  { id: 'endpoint', header: props.t('endpoint') },
  { id: 'status', header: props.t('status') },
  { id: 'actions' },
])

const targetTableMeta = computed(() => ({
  class: {
    tr: (row: { index: number }) =>
      row.index === dragOverIndex.value ? 'bg-elevated' : '',
  },
}))
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="header"
    :ui="{ content: 'sm:max-w-4xl' }"
  >
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('save')">
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
          <UTable
            :data="form?.targets ?? []"
            :columns="targetColumns"
            :meta="targetTableMeta"
            class="min-w-0"
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
                  <div class="flex items-center">
                    <ModelRouteTargetSettings
                      v-model:target="row.original"
                      :t="t"
                    />
                  </div>
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
