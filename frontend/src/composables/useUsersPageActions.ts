import { computed, ref } from 'vue'

import type { User } from '@/generated/admin-api'
import type { NewUserForm } from '@/models'
import type { useUsersStore } from '@/stores/users'
import { CREATE_KEY } from '@/stores/record-action-types'

type UsersStore = ReturnType<typeof useUsersStore>

export type UsersPageActionDeps = {
  confirm: (message: string) => boolean
  notifyApiError: (cause: unknown) => void
  notifySuccess: (message: string) => void
  store: UsersStore
  t: (key: string) => string
}

const emptyUserForm = (): NewUserForm => ({
  login_name: '',
  password: '',
  display_name: '',
  is_admin: false,
})

/**
 * The page-owned half of the users workspace: the dialogs, the drafts they
 * edit, and the single completion each dialog is allowed to perform.
 *
 * Every operation is a thin wrapper around the store's guarded action and
 * reports success only when that action reports it ran, so a call the guard
 * refused produces no toast, no error, and no close.
 */
export function useUsersPageActions(deps: UsersPageActionDeps) {
  const { confirm, notifyApiError, notifySuccess, store, t } = deps

  const createUserVisible = ref(false)
  const createUserForm = ref<NewUserForm>(emptyUserForm())
  const resetPasswordUser = ref<User | null>(null)
  const resetPasswordValue = ref('')

  // Each dialog is identified by the generation it was opened in, so a write
  // that settles late can tell that the surface it was started from is no
  // longer the one on screen and must not close it.
  let createGeneration = 0
  let resetGeneration = 0

  const resetPasswordDialogVisible = computed({
    get: () => resetPasswordUser.value !== null,
    set: (visible: boolean) => {
      if (!visible) {
        resetPasswordUser.value = null
      }
    },
  })

  // A dialog's own write. It follows the record its draft belongs to, so a
  // write against another account never freezes or spins this dialog.
  const createUserPending = computed(() =>
    Object.hasOwn(store.userPendingActions, CREATE_KEY),
  )
  const resetPasswordPending = computed(() => {
    const userId = resetPasswordUser.value?.user_id
    return (
      userId != null && Object.hasOwn(store.userPendingActions, String(userId))
    )
  })

  async function saveUser(user: User): Promise<void> {
    // Captured before the request is claimed, so a later edit to the row cannot
    // change what this save sends.
    const body = {
      display_name: user.display_name,
      is_admin: user.is_admin,
      is_active: user.is_active,
    }
    try {
      const result = await store.saveUser(user.user_id, body)
      if (!result.executed) return
      notifySuccess(t('savedUser'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function deleteUser(user: User): Promise<void> {
    // A cancelled confirmation is not an action: no request, no key, no toast.
    if (!confirm(`${t('delete')} ${user.login_name}?`)) return
    try {
      const result = await store.removeUser(user.user_id)
      if (!result.executed) return
      notifySuccess(t('userDeleted'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  function openResetPassword(user: User): void {
    resetGeneration += 1
    resetPasswordValue.value = ''
    resetPasswordUser.value = user
  }

  async function submitCreateUser(): Promise<void> {
    const generation = createGeneration
    const body = { ...createUserForm.value }
    try {
      const result = await store.createNewUser(body)
      if (!result.executed) return
      if (createGeneration === generation) {
        createUserVisible.value = false
        createUserForm.value = emptyUserForm()
        createGeneration += 1
      }
      notifySuccess(t('userCreated'))
    } catch (cause) {
      // The dialog and its draft stay as they are so the create can be retried.
      notifyApiError(cause)
    }
  }

  async function submitResetPassword(): Promise<void> {
    const userId = resetPasswordUser.value?.user_id
    if (userId == null) return
    const generation = resetGeneration
    const password = resetPasswordValue.value
    try {
      const result = await store.resetUserPassword(userId, password)
      if (!result.executed) return
      if (resetGeneration === generation) {
        resetPasswordValue.value = ''
        resetPasswordUser.value = null
        resetGeneration += 1
      }
      notifySuccess(t('passwordReset'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  function openCreateUser(): void {
    createGeneration += 1
    createUserVisible.value = true
  }

  return {
    createUserForm,
    createUserPending,
    createUserVisible,
    deleteUser,
    openCreateUser,
    openResetPassword,
    resetPasswordDialogVisible,
    resetPasswordPending,
    resetPasswordUser,
    resetPasswordValue,
    saveUser,
    submitCreateUser,
    submitResetPassword,
  }
}
