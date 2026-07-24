import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  createUser,
  deleteUser,
  listUsers,
  listUserOptions,
  resetPassword,
  updateUser,
} from '../generated/admin-api'
import type {
  CreateUserRequest,
  User,
  UserUpdate,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'

export const useUsersStore = defineStore('users', () => {
  const users = ref<User[]>([])
  const pageUsers = ref<User[]>([])
  const loading = ref(false)
  const first = ref(0)
  const rows = useStoredPageSize('users', 10, STANDARD_PAGE_SIZE_OPTIONS)
  const total = ref(0)

  const totalUsers = computed(() => total.value)

  async function loadUsers(): Promise<void> {
    loading.value = true
    try {
      users.value = expectData(await listUserOptions<true>(withData())).users
    } finally {
      loading.value = false
    }
  }

  async function loadPage(
    nextFirst = first.value,
    nextRows = rows.value,
  ): Promise<void> {
    first.value = nextFirst
    rows.value = nextRows
    loading.value = true
    try {
      const response = expectData(
        await listUsers<true>(
          withData({ query: { first: first.value, rows: rows.value } }),
        ),
      )
      pageUsers.value = response.users
      total.value = response.total
      first.value = response.first
      rows.value = response.rows
      if (
        pageUsers.value.length === 0 &&
        total.value > 0 &&
        first.value >= total.value
      ) {
        const previousFirst = Math.floor((total.value - 1) / rows.value) * rows.value
        await loadPage(previousFirst, rows.value)
      }
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
    pageUsers.value = pageUsers.value.map((item) =>
      item.user_id === saved.user_id ? saved : item,
    )
    return saved
  }

  async function createNewUser(input: CreateUserRequest): Promise<User> {
    const created = expectData(
      await createUser<true>(withData({ body: input })),
    )
    users.value = [...users.value, created]
    await loadPage()
    return created
  }

  async function removeUser(userId: number): Promise<void> {
    await deleteUser<true>(withData({ path: { user_id: userId } }))
    users.value = users.value.filter((user) => user.user_id !== userId)
    await loadPage()
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
    first,
    loading,
    loadPage,
    loadUsers,
    pageUsers,
    removeUser,
    resetUserPassword,
    saveUser,
    totalUsers,
    total,
    rows,
    users,
  }
})
