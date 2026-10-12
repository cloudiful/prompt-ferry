import type { Ref } from 'vue'

import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import type { CreateUserRequest, User, UserUpdate } from '@/generated/admin-api'
import {
  createUser,
  deleteUser,
  resetPassword,
  updateUser,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { capturePayload, recordActionKey } from '@/stores/record-action-types'

/**
 * Key namespace for the guarded user writes.
 *
 * Save, delete, and password reset of one user share a key: all three write the
 * same account, so two of them running at once would leave it in a state the
 * user never asked for. Create has its own, since there is no user to write yet.
 */
export const USER_KEY_PREFIX = 'user'

export function userActionKey(userId: number | null): string {
  return recordActionKey(
    USER_KEY_PREFIX,
    userId == null ? null : String(userId),
  )
}

type UsersSlice = {
  users: Ref<User[]>
  pageUsers: Ref<User[]>
}

export type UserActionDeps = {
  guard: AsyncActionGuard
  loadPage: () => Promise<void>
  users: UsersSlice
}

/**
 * The user writes. The two lists are patched in place from the response, so a
 * save does not refetch and a delete of the last row on a page cannot strand the
 * pager — the same behaviour the store had before the guard was added.
 */
export function createUserActions(deps: UserActionDeps) {
  const { guard, loadPage, users } = deps

  function saveUser(
    userId: number,
    input: UserUpdate,
  ): Promise<AsyncActionOutcome<User>> {
    const captured = capturePayload(input)
    return guard.run(
      userActionKey(userId),
      async () => {
        const saved = expectData(
          await updateUser<true>(
            withData({ path: { user_id: userId }, body: captured }),
          ),
        )
        users.users.value = users.users.value.map((item) =>
          item.user_id === saved.user_id ? saved : item,
        )
        users.pageUsers.value = users.pageUsers.value.map((item) =>
          item.user_id === saved.user_id ? saved : item,
        )
        return saved
      },
      'save',
    )
  }

  function createNewUser(
    input: CreateUserRequest,
  ): Promise<AsyncActionOutcome<User>> {
    const captured = capturePayload(input)
    return guard.run(
      userActionKey(null),
      async () => {
        const created = expectData(
          await createUser<true>(withData({ body: captured })),
        )
        users.users.value = [...users.users.value, created]
        await loadPage()
        return created
      },
      'save',
    )
  }

  function removeUser(userId: number): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      userActionKey(userId),
      async () => {
        await deleteUser<true>(withData({ path: { user_id: userId } }))
        users.users.value = users.users.value.filter(
          (user) => user.user_id !== userId,
        )
        await loadPage()
      },
      'delete',
    )
  }

  function resetUserPassword(
    userId: number,
    password: string,
  ): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      userActionKey(userId),
      async () => {
        await resetPassword<true>(
          withData({ path: { user_id: userId }, body: { password } }),
        )
      },
      'reset',
    )
  }

  return { createNewUser, removeUser, resetUserPassword, saveUser }
}

export type UserActions = ReturnType<typeof createUserActions>
