<script setup lang="ts">
import PageIntro from '../components/PageIntro.vue'
import UsersPanel from '../components/users/UsersPanel.vue'
import CreateUserDialog from '../components/users/CreateUserDialog.vue'
import ResetPasswordDialog from '../components/users/ResetPasswordDialog.vue'
import { useUsersPage } from '@/composables/useUsersPage'

const {
  busy,
  createUserForm,
  createUserVisible,
  deleteUser,
  openResetPassword,
  resetPasswordUser,
  resetPasswordDialogVisible,
  resetPasswordValue,
  refresh,
  saveUser,
  submitCreateUser,
  submitResetPassword,
  t,
  usersWorkspace,
  onPage,
} = useUsersPage()
</script>

<template>
  <div class="grid min-w-0 max-w-full gap-3">
    <PageIntro :eyebrow="t('admin')" :title="t('user')">
      <template #actions>
        <UButton
          size="sm"
          color="neutral"
          variant="outline"
          :loading="busy"
          :aria-label="t('refresh')"
          @click="refresh"
        >
          <span>{{ t('refresh') }}</span>
        </UButton>
        <UButton
          size="sm"
          :aria-label="t('createUser')"
          @click="
            () => {
              createUserVisible = true
            }
          "
        >
          <span aria-hidden="true" class="md:hidden">新增</span>
          <span aria-hidden="true" class="hidden md:inline">{{
            t('createUser')
          }}</span>
        </UButton>
      </template>
    </PageIntro>

    <UsersPanel
      :t="t"
      :workspace="usersWorkspace"
      @open-reset-password="openResetPassword"
      @save-user="saveUser"
      @delete-user="deleteUser"
      @page="onPage"
    />

    <CreateUserDialog
      v-model:visible="createUserVisible"
      v-model:form="createUserForm"
      :busy="busy"
      :t="t"
      @submit="submitCreateUser"
    />

    <ResetPasswordDialog
      v-model:visible="resetPasswordDialogVisible"
      v-model:password="resetPasswordValue"
      :busy="busy"
      :t="t"
      :user="resetPasswordUser"
      @submit="submitResetPassword"
    />
  </div>
</template>
