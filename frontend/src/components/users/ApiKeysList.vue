<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed, ref } from 'vue'
import type { ApiKeyItemView, ApiKeysWorkspaceView } from '@/models'
import TablePagination from '@/components/shared/TablePagination.vue'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/table-pagination'

const props = defineProps<{
  first: number
  rows: number
  total: number
  t: TranslateFn
  workspace: ApiKeysWorkspaceView
}>()

const draftLabels = ref<Record<number, string>>({})

const columns = computed<TableColumn<ApiKeyItemView>[]>(() => [
  { accessorKey: 'key_prefix', header: props.t('keyPrefix') },
  { accessorKey: 'label', header: props.t('name') },
  { id: 'status', header: props.t('status') },
  { id: 'secret', header: props.t('apiKey') },
  { id: 'actions' },
])

const emit = defineEmits<{
  toggleKeySecret: [keyId: number]
  copyKeySecret: [keyId: number]
  toggleKey: [keyId: number]
  renameKey: [keyId: number, label: string]
  deleteKey: [keyId: number]
  page: [event: TablePageChange]
}>()

function labelDraft(keyItem: ApiKeyItemView): string {
  return draftLabels.value[keyItem.key_id] ?? keyItem.label
}

function updateLabel(keyId: number, value: unknown): void {
  draftLabels.value[keyId] = String(value ?? '')
}

function commitLabel(keyItem: ApiKeyItemView): void {
  const label = (draftLabels.value[keyItem.key_id] ?? keyItem.label).trim()
  delete draftLabels.value[keyItem.key_id]
  if (!label || label === keyItem.label) return
  emit('renameKey', keyItem.key_id, label)
}
</script>

<template>
  <section class="grid min-w-0 max-w-full gap-3">
    <div
      v-if="!workspace.has_keys"
      class="rounded-xl border border-default bg-default px-4 py-6 text-sm text-dimmed"
    >
      {{ t('noClientKeys') }}
    </div>

    <div v-else class="grid gap-3 md:hidden">
      <div
        v-for="keyItem in workspace.key_items"
        :key="keyItem.key_id"
        class="grid gap-3 rounded-lg border border-default bg-default p-3"
      >
        <div class="flex items-center justify-between gap-2">
          <div class="min-w-0">
            <div class="truncate font-mono text-xs text-muted">
              {{ keyItem.key_prefix }}
            </div>
            <UInput
              :model-value="labelDraft(keyItem)"
              size="sm"
              class="mt-1 w-full"
              :aria-label="t('name')"
              @update:model-value="updateLabel(keyItem.key_id, $event)"
              @keydown.enter.prevent="commitLabel(keyItem)"
              @blur="commitLabel(keyItem)"
            />
          </div>
        </div>

        <div class="flex items-center justify-between gap-3">
          <div
            class="text-[0.7rem] font-bold tracking-wide text-dimmed uppercase"
          >
            {{ t('active') }}
          </div>
          <label class="inline-flex items-center">
            <USwitch
              :model-value="keyItem.enabled"
              :aria-label="t('status')"
              @update:model-value="$emit('toggleKey', keyItem.key_id)"
            />
          </label>
        </div>

        <div v-if="keyItem.secret" class="grid gap-2">
          <div
            v-if="keyItem.visible_secret"
            class="overflow-x-auto rounded-md bg-muted px-2.5 py-2 font-mono text-xs text-default"
          >
            {{ keyItem.secret }}
          </div>
          <div class="flex flex-wrap gap-2">
            <UButton
              size="sm"
              color="neutral"
              variant="ghost"
              @click="$emit('toggleKeySecret', keyItem.key_id)"
            >
              <UIcon
                :name="
                  keyItem.visible_secret ? 'i-lucide-eye-off' : 'i-lucide-eye'
                "
                class="h-4 w-4"
              />
              {{ keyItem.visible_secret ? t('hideSecret') : t('showSecret') }}
            </UButton>
            <UButton
              size="sm"
              color="neutral"
              variant="ghost"
              @click="$emit('copyKeySecret', keyItem.key_id)"
            >
              <UIcon name="i-lucide-copy" class="h-4 w-4" />
              {{ t('copy') }}
            </UButton>
          </div>
        </div>

        <div class="flex justify-end gap-2">
          <UButton
            size="sm"
            color="error"
            variant="ghost"
            :aria-label="t('delete')"
            @click="$emit('deleteKey', keyItem.key_id)"
          >
            {{ t('delete') }}
          </UButton>
        </div>
      </div>
    </div>

    <UTable
      v-if="workspace.has_keys"
      :data="workspace.key_items"
      :columns="columns"
      class="hidden min-w-0 md:block"
    >
      <template #label-cell="{ row }">
        <UInput
          :model-value="labelDraft(row.original)"
          size="sm"
          class="w-48 max-w-full"
          :aria-label="t('name')"
          @update:model-value="updateLabel(row.original.key_id, $event)"
          @keydown.enter.prevent="commitLabel(row.original)"
          @blur="commitLabel(row.original)"
        />
      </template>
      <template #key_prefix-cell="{ row }">
        <span class="font-mono text-xs text-muted">
          {{ row.original.key_prefix }}
        </span>
      </template>
      <template #status-cell="{ row }">
        <div class="flex items-center gap-3">
          <USwitch
            :model-value="row.original.enabled"
            :aria-label="t('status')"
            @update:model-value="$emit('toggleKey', row.original.key_id)"
          />
        </div>
      </template>
      <template #secret-cell="{ row }">
        <div v-if="row.original.secret" class="flex items-center gap-2">
          <div
            v-if="row.original.visible_secret"
            class="min-w-0 max-w-[18rem] overflow-x-auto rounded-md bg-muted px-2 py-1.5 font-mono text-xs text-default"
          >
            {{ row.original.secret }}
          </div>
          <div class="flex shrink-0 items-center gap-0.5">
            <UButton
              size="sm"
              color="neutral"
              variant="ghost"
              :aria-label="
                row.original.visible_secret ? t('hideSecret') : t('showSecret')
              "
              @click="$emit('toggleKeySecret', row.original.key_id)"
            >
              <UIcon
                :name="
                  row.original.visible_secret
                    ? 'i-lucide-eye-off'
                    : 'i-lucide-eye'
                "
                class="h-4 w-4"
              />
              {{
                row.original.visible_secret ? t('hideSecret') : t('showSecret')
              }}
            </UButton>
            <UButton
              size="sm"
              color="neutral"
              variant="ghost"
              :aria-label="t('copy')"
              @click="$emit('copyKeySecret', row.original.key_id)"
            >
              <UIcon name="i-lucide-copy" class="h-4 w-4" />
              {{ t('copy') }}
            </UButton>
          </div>
        </div>
        <span v-else class="text-xs text-dimmed">-</span>
      </template>
      <template #actions-cell="{ row }">
        <div class="flex justify-end gap-1">
          <UButton
            size="sm"
            color="error"
            variant="ghost"
            :aria-label="t('delete')"
            @click="$emit('deleteKey', row.original.key_id)"
          >
            {{ t('delete') }}
          </UButton>
        </div>
      </template>
    </UTable>
    <TablePagination
      :first="props.first"
      :rows="props.rows"
      :total="props.total"
      :page-size-options="STANDARD_PAGE_SIZE_OPTIONS"
      @change="$emit('page', $event)"
    />
  </section>
</template>
