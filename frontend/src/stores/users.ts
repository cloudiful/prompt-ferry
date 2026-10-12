import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { listUserOptions, listUsers } from '../generated/admin-api'
import type { User } from '../generated/admin-api'
import { expectData, withData } from '../api'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'
import { useAsyncAction } from '../composables/useAsyncAction'
import { USER_KEY_PREFIX, createUserActions } from './users-actions'
import { pendingRowsById } from './record-action-types'

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
        const previousFirst =
          Math.floor((total.value - 1) / rows.value) * rows.value
        await loadPage(previousFirst, rows.value)
      }
    } finally {
      loading.value = false
    }
  }

  // The guard lives here rather than at each call site, so a user can be gated
  // against every operation that would write that account.
  const guard = useAsyncAction()
  const actions = createUserActions({
    guard,
    loadPage: () => loadPage(),
    users: { users, pageUsers },
  })

  // Per-row view of the guard. `loading` stays the fetch-only busy flag, so a
  // write never blanks the whole page.
  const userPendingActions = computed(() =>
    pendingRowsById(guard.pendingActions.value, USER_KEY_PREFIX),
  )

  return {
    ...actions,
    first,
    isPending: guard.isPending,
    loading,
    loadPage,
    loadUsers,
    pageUsers,
    pendingActions: guard.pendingActions,
    rows,
    totalUsers,
    total,
    userPendingActions,
    users,
  }
})
