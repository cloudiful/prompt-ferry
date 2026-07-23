import { computed, onMounted, ref } from 'vue'

import { useLocale } from '@/composables/useLocale'
import { useNotifier } from '@/composables/useNotifier'
import type { User } from '@/generated/admin-api'
import type { NewUserForm } from '@/models'
import { createUsersWorkspaceView } from '@/models/users'
import { useUsersStore } from '@/stores/users'

export function useUsersPage() {
  const usersStore = useUsersStore()
  const { t } = useLocale()
  const { notifyApiError, notifySuccess } = useNotifier()
  const busy = computed(() => usersStore.loading)
  const createUserVisible = ref(false)
  const resetPasswordUser = ref<User | null>(null)
  const resetPasswordValue = ref('')
  const createUserForm = ref<NewUserForm>({
    login_name: '',
    password: '',
    display_name: '',
    is_admin: false,
  })

  const usersWorkspace = computed(() =>
    createUsersWorkspaceView({
      busy: busy.value,
      users: usersStore.users,
    }),
  )
  const resetPasswordDialogVisible = computed({
    get: () => resetPasswordUser.value !== null,
    set: (visible: boolean) => {
      if (!visible) {
        resetPasswordUser.value = null
      }
    },
  })
  async function refresh(): Promise<void> {
    try {
      await usersStore.loadUsers()
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function saveUser(user: User): Promise<void> {
    try {
      await usersStore.saveUser(user.user_id, {
        display_name: user.display_name,
        is_admin: user.is_admin,
        is_active: user.is_active,
      })
      notifySuccess(t('savedUser'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function deleteUser(user: User): Promise<void> {
    if (!window.confirm(`${t('delete')} ${user.login_name}?`)) return
    try {
      await usersStore.removeUser(user.user_id)
      notifySuccess(t('userDeleted'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  function openResetPassword(user: User): void {
    resetPasswordUser.value = user
  }

  async function submitCreateUser(): Promise<void> {
    try {
      await usersStore.createNewUser(createUserForm.value)
      createUserVisible.value = false
      createUserForm.value = {
        login_name: '',
        password: '',
        display_name: '',
        is_admin: false,
      }
      notifySuccess(t('userCreated'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function submitResetPassword(): Promise<void> {
    if (!resetPasswordUser.value) return
    try {
      await usersStore.resetUserPassword(
        resetPasswordUser.value.user_id,
        resetPasswordValue.value,
      )
      resetPasswordValue.value = ''
      resetPasswordUser.value = null
      notifySuccess(t('passwordReset'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  onMounted(async () => {
    await refresh()
  })

  return {
    busy,
    createUserForm,
    createUserVisible,
    deleteUser,
    openResetPassword,
    refresh,
    resetPasswordDialogVisible,
    resetPasswordUser,
    resetPasswordValue,
    saveUser,
    submitCreateUser,
    submitResetPassword,
    t,
    usersWorkspace,
  }
}
