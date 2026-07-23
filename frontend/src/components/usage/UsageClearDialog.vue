<script setup lang="ts">
import { computed } from 'vue'
import type { User } from '@/generated/admin-api'
import type { RequestRecordClearForm } from '@/models'

const props = defineProps<{
  busy: boolean
  currentUserLabel: string
  isAdmin: boolean
  t: TranslateFn
  users: User[]
}>()

const visible = defineModel<boolean>('visible', { required: true })
const form = defineModel<RequestRecordClearForm>('form', { required: true })

defineEmits<{
  submit: []
}>()

const scopeOptions = computed(() => [
  { label: props.t('clearCurrentUserHistory'), value: 'current_user' },
  ...(props.isAdmin
    ? [
        {
          label: props.t('clearSelectedUserHistory'),
          value: 'target_user' as const,
        },
      ]
    : []),
  ...(props.isAdmin
    ? [{ label: props.t('clearAllRecords'), value: 'all_users' as const }]
    : []),
])

const userOptions = computed(() =>
  props.users.map((user) => ({
    label: `${user.display_name} / ${user.login_name}`,
    value: user.user_id,
  })),
)
</script>

<template>
  <UModal
    v-model:open="visible"
    :title="t('clearHistory')"
    :ui="{ content: 'sm:max-w-xl' }"
  >
    <template #body>
      <form class="grid gap-3 text-xs" @submit.prevent="$emit('submit')">
        <div class="grid gap-1">
          <label class="font-medium text-highlighted">{{
            t('clearScope')
          }}</label>
          <USelect
            v-model="form.scope"
            :items="scopeOptions"
            label-key="label"
            value-key="value"
            size="sm"
          />
        </div>

        <div v-if="form.scope === 'target_user'" class="grid gap-1">
          <label class="font-medium text-highlighted">{{
            t('selectUser')
          }}</label>
          <USelect
            :model-value="form.user_id ?? undefined"
            :items="userOptions"
            label-key="label"
            value-key="value"
            size="sm"
            @update:model-value="form.user_id = $event ?? null"
          />
        </div>

        <div
          v-else-if="form.scope === 'current_user'"
          class="rounded border border-default bg-muted px-3 py-2 text-muted"
        >
          {{ currentUserLabel }}
        </div>

        <label
          class="inline-flex min-h-8 items-center gap-2 text-[0.75rem] text-default"
        >
          <UCheckbox v-model="form.delete_all" />
          {{ t('clearAllRecords') }}
        </label>

        <div class="grid gap-3 sm:grid-cols-2">
          <div class="grid gap-1">
            <label class="font-medium text-highlighted">{{
              t('startTime')
            }}</label>
            <UInput
              v-model="form.start_at"
              type="datetime-local"
              size="sm"
              :disabled="form.delete_all"
            />
          </div>
          <div class="grid gap-1">
            <label class="font-medium text-highlighted">{{
              t('endTime')
            }}</label>
            <UInput
              v-model="form.end_at"
              type="datetime-local"
              size="sm"
              :disabled="form.delete_all"
            />
          </div>
        </div>

        <div
          class="rounded border border-default bg-muted px-3 py-2 leading-5 text-muted"
        >
          <div>{{ t('clearHistoryHint') }}</div>
          <div>{{ t('clearHistoryWarning') }}</div>
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
          <UButton type="submit" size="sm" color="error" :loading="busy">
            <UIcon name="i-lucide-trash-2" class="h-4 w-4" />
            {{ t('clearHistorySubmit') }}
          </UButton>
        </div>
      </form>
    </template>
  </UModal>
</template>
