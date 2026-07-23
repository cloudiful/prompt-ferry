import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  createUser,
  deleteUser,
  listUsers,
  resetPassword,
  updateUser,
} from '../generated/admin-api'
import type {
  CreateUserRequest,
  User,
  UserUpdate,
} from '../generated/admin-api'
import { expectData, withData } from '../api'

export const useUsersStore = defineStore('users', () => {
  const users = ref<User[]>([])
  const loading = ref(false)

  const totalUsers = computed(() => users.value.length)

  async function loadUsers(): Promise<void> {
    loading.value = true
    try {
      users.value = expectData(await listUsers<true>(withData()))
    } finally {
      loading.value = false
    }
  }

  async function saveUser(userId: number, input: UserUpdate): Promise<User> {
    const saved = expectData(
      await updateUser<true>(
        withData({ path: { user_id: userId }, body: input }),
      ),
    )
    users.value = users.value.map((item) =>
      item.user_id === saved.user_id ? saved : item,
    )
    return saved
  }

  async function createNewUser(input: CreateUserRequest): Promise<User> {
    const created = expectData(
      await createUser<true>(withData({ body: input })),
    )
    users.value = [...users.value, created]
    return created
  }

  async function removeUser(userId: number): Promise<void> {
    await deleteUser<true>(withData({ path: { user_id: userId } }))
    users.value = users.value.filter((user) => user.user_id !== userId)
  }

  async function resetUserPassword(
    userId: number,
    password: string,
  ): Promise<void> {
    await resetPassword<true>(
      withData({ path: { user_id: userId }, body: { password } }),
    )
  }

  return {
    createNewUser,
    loading,
    loadUsers,
    removeUser,
    resetUserPassword,
    saveUser,
    totalUsers,
    users,
  }
})
