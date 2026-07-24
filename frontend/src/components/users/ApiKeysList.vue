<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
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

const columns = computed<TableColumn<ApiKeyItemView>[]>(() => [
  { accessorKey: 'label', header: props.t('name') },
  { id: 'status', header: props.t('status') },
  { id: 'secret', header: props.t('apiKey') },
  { id: 'actions' },
])

defineEmits<{
  toggleKeySecret: [keyId: number]
  copyKeySecret: [keyId: number]
  toggleKey: [keyId: number]
  deleteKey: [keyId: number]
  page: [event: TablePageChange]
}>()
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
            <div class="truncate text-[0.82rem] font-semibold text-highlighted">
              {{ keyItem.label }}
            </div>
          </div>
          <UBadge
            :label="keyItem.enabled_label"
            :color="keyItem.enabled ? 'success' : 'warning'"
          />
        </div>

        <div class="flex items-center justify-between gap-3">
          <div
            class="text-[0.7rem] font-bold tracking-wide text-dimmed uppercase"
          >
            {{ t('active') }}
          </div>
          <label class="inline-flex items-center gap-2">
            <USwitch
              :model-value="keyItem.enabled"
              @update:model-value="$emit('toggleKey', keyItem.key_id)"
            />
            <span class="text-xs text-dimmed">{{ keyItem.enabled_label }}</span>
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
        <div class="font-semibold text-highlighted">
          {{ row.original.label }}
        </div>
      </template>
      <template #status-cell="{ row }">
        <div class="flex items-center gap-3">
          <USwitch
            :model-value="row.original.enabled"
            @update:model-value="$emit('toggleKey', row.original.key_id)"
          />
          <UBadge
            :label="row.original.enabled_label"
            :color="row.original.enabled ? 'success' : 'warning'"
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
