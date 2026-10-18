<script setup lang="ts">
import type { TableColumn } from '@nuxt/ui'
import { computed } from 'vue'
import type { User } from '@/generated/admin-api'
import type { UserListItemView, UsersWorkspaceView } from '@/models/users'
import TablePagination from '@/components/shared/TablePagination.vue'
import { STANDARD_PAGE_SIZE_OPTIONS } from '@/table-pagination'

defineEmits<{
  openResetPassword: [user: User]
  saveUser: [user: User]
  deleteUser: [user: User]
  page: [event: TablePageChange]
}>()

const props = withDefaults(
  defineProps<{
    /** Which rows are gated, and by which action. Absent means nothing is. */
    pendingKeys?: Readonly<Record<string, string>>
    t: TranslateFn
    workspace: UsersWorkspaceView
  }>(),
  { pendingKeys: () => ({}) },
)

/** One user's write is in flight: its row's controls are gated. */
const isPending = (userId: number) =>
  Object.hasOwn(props.pendingKeys, String(userId))

/** Only the control that started the write spins; the rest are just disabled. */
const pendingAction = (userId: number, action: string) =>
  isPending(userId) && props.pendingKeys[String(userId)] === action

const columns = computed<TableColumn<UserListItemView>[]>(() => [
  { accessorKey: 'login_name', header: props.t('user') },
  { id: 'displayName', header: props.t('displayName') },
  { id: 'admin', header: props.t('adminShort') },
  { id: 'active', header: props.t('active') },
  { id: 'actions' },
])

const mobileToggleLabelClass =
  'inline-flex min-h-5 items-center gap-1 text-[0.68rem] text-default'
</script>

<template>
  <section class="grid min-w-0 max-w-full gap-3">
    <div
      v-if="!workspace.has_users"
      class="rounded-xl border border-default bg-default px-4 py-6 text-sm text-dimmed"
    >
      {{ t('noUser') }}
    </div>

    <div class="grid gap-3 md:hidden">
      <div
        v-for="item in workspace.user_items"
        :key="item.user_id"
        class="grid gap-2 rounded-lg border border-default bg-default p-3"
      >
        <div class="flex items-start justify-between gap-2">
          <div class="grid min-w-0 gap-px">
            <div
              class="truncate text-[0.82rem] leading-[1.1] font-semibold text-highlighted"
            >
              {{ item.login_name }}
            </div>
            <div
              class="flex flex-wrap items-center gap-x-1.5 gap-y-px text-[0.64rem] leading-[1.12] text-dimmed"
            >
              <span>{{ t('id') }} {{ item.user_id }}</span>
              <span v-if="item.user.display_name" class="truncate">{{
                item.user.display_name
              }}</span>
            </div>
          </div>
          <div class="flex flex-wrap items-center justify-end gap-px">
            <UButton
              size="sm"
              color="neutral"
              variant="ghost"
              :loading="pendingAction(item.user.user_id, 'reset')"
              :disabled="isPending(item.user.user_id)"
              @click="$emit('openResetPassword', item.user)"
            >
              <UIcon name="i-lucide-key-round" class="h-4 w-4" />
            </UButton>
            <UButton
              size="sm"
              variant="ghost"
              :loading="pendingAction(item.user.user_id, 'save')"
              :disabled="isPending(item.user.user_id)"
              @click="$emit('saveUser', item.user)"
            >
              <UIcon name="i-lucide-save" class="h-4 w-4" />
            </UButton>
            <UButton
              size="sm"
              color="error"
              variant="ghost"
              :loading="pendingAction(item.user.user_id, 'delete')"
              :disabled="isPending(item.user.user_id)"
              @click="$emit('deleteUser', item.user)"
            >
              <UIcon name="i-lucide-trash-2" class="h-4 w-4" />
            </UButton>
          </div>
        </div>

        <div class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-1">
          <UInput
            v-model="item.user.display_name"
            class="w-full"
            size="sm"
            :placeholder="t('displayName')"
          />
          <UButton
            size="sm"
            variant="ghost"
            :loading="pendingAction(item.user.user_id, 'save')"
            :disabled="isPending(item.user.user_id)"
            @click="$emit('saveUser', item.user)"
          >
            <UIcon name="i-lucide-save" class="h-4 w-4" />
          </UButton>
        </div>

        <div class="flex flex-wrap items-center gap-x-2 gap-y-1">
          <label :class="mobileToggleLabelClass"
            ><UCheckbox
              v-model="item.user.is_admin"
              :aria-label="t('userAdminToggle', { name: item.login_name })"
            />{{ t('adminShort') }}</label
          >
          <label :class="mobileToggleLabelClass"
            ><UCheckbox
              v-model="item.user.is_active"
              :aria-label="t('userActiveToggle', { name: item.login_name })"
            />{{ t('active') }}</label
          >
        </div>
      </div>
    </div>

    <UTable
      v-if="workspace.has_users"
      :data="workspace.user_items"
      :columns="columns"
      class="hidden min-w-0 md:block"
      :ui="{ th: 'whitespace-nowrap' }"
    >
      <template #empty>
        <div class="px-4 py-6 text-sm text-dimmed">
          {{ t('noUser') }}
        </div>
      </template>
      <template #login_name-cell="{ row }">
        <div class="font-semibold whitespace-nowrap text-highlighted">
          {{ row.original.login_name }}
        </div>
      </template>
      <template #displayName-cell="{ row }">
        <UInput
          v-model="row.original.user.display_name"
          size="sm"
          :placeholder="t('displayName')"
          class="w-full"
        />
      </template>
      <template #admin-cell="{ row }">
        <UCheckbox
          v-model="row.original.user.is_admin"
          :aria-label="t('userAdminToggle', { name: row.original.login_name })"
        />
      </template>
      <template #active-cell="{ row }">
        <UCheckbox
          v-model="row.original.user.is_active"
          :aria-label="t('userActiveToggle', { name: row.original.login_name })"
        />
      </template>
      <template #actions-cell="{ row }">
        <div class="flex flex-nowrap items-center justify-end gap-1">
          <UButton
            size="sm"
            color="neutral"
            variant="ghost"
            :aria-label="t('resetPassword')"
            :loading="pendingAction(row.original.user.user_id, 'reset')"
            :disabled="isPending(row.original.user.user_id)"
            @click="$emit('openResetPassword', row.original.user)"
            ><UIcon name="i-lucide-key-round" class="h-4 w-4"
          /></UButton>
          <UButton
            size="sm"
            variant="ghost"
            :aria-label="t('save')"
            :loading="pendingAction(row.original.user.user_id, 'save')"
            :disabled="isPending(row.original.user.user_id)"
            @click="$emit('saveUser', row.original.user)"
            ><UIcon name="i-lucide-save" class="h-4 w-4"
          /></UButton>
          <UButton
            size="sm"
            color="error"
            variant="ghost"
            :aria-label="t('delete')"
            :loading="pendingAction(row.original.user.user_id, 'delete')"
            :disabled="isPending(row.original.user.user_id)"
            @click="$emit('deleteUser', row.original.user)"
            ><UIcon name="i-lucide-trash-2" class="h-4 w-4"
          /></UButton>
        </div>
      </template>
    </UTable>
    <TablePagination
      :first="workspace.first"
      :rows="workspace.rows"
      :total="workspace.total"
      :page-size-options="STANDARD_PAGE_SIZE_OPTIONS"
      @change="$emit('page', $event)"
    />
  </section>
</template>
