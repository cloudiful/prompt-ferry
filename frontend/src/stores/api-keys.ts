import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import {
  listClientKeys,
  listUserOptions,
  meListClientKeys,
} from '../generated/admin-api'
import type { ClientKey, User } from '../generated/admin-api'
import { expectData, withData } from '../api'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'
import { useAsyncAction } from '../composables/useAsyncAction'
import {
  CLIENT_KEY_PREFIX,
  SELF_USER_ID,
  createApiKeyActions,
} from './api-keys-actions'
import { pendingRowsById } from './record-action-types'

export { SELF_USER_ID }

export const useApiKeysStore = defineStore('api-keys', () => {
  const users = ref<User[]>([])
  const selectedUserId = ref<number>(SELF_USER_ID)
  const keys = ref<ClientKey[]>([])
  const loading = ref(false)
  const usersLoading = ref(false)
  const first = ref(0)
  const rows = useStoredPageSize('api-keys', 10, STANDARD_PAGE_SIZE_OPTIONS)
  const total = ref(0)
  let selectedContextGeneration = 0
  let latestViewLoadId = 0

  watch(
    selectedUserId,
    () => {
      selectedContextGeneration += 1
    },
    { flush: 'sync' },
  )

  const hasUsers = computed(() => users.value.length > 0)
  const selectedUser = computed(() =>
    selectedUserId.value === SELF_USER_ID
      ? null
      : (users.value.find((user) => user.user_id === selectedUserId.value) ??
        null),
  )

  async function loadUsers(): Promise<User[]> {
    usersLoading.value = true
    try {
      const loaded = expectData(await listUserOptions<true>(withData())).users
      users.value = loaded
      return loaded
    } finally {
      usersLoading.value = false
    }
  }

  async function loadKeys(
    forceUserId?: number,
    nextFirst = first.value,
    nextRows = rows.value,
  ): Promise<ClientKey[]> {
    const targetUserId = forceUserId ?? selectedUserId.value
    let requestedFirst = nextFirst
    let requestedRows = nextRows
    const contextGeneration = selectedContextGeneration
    const updatesView = targetUserId === selectedUserId.value
    const viewLoadId = updatesView ? ++latestViewLoadId : null
    const ownsView = () =>
      updatesView &&
      contextGeneration === selectedContextGeneration &&
      targetUserId === selectedUserId.value &&
      viewLoadId === latestViewLoadId

    if (ownsView()) {
      first.value = requestedFirst
      rows.value = requestedRows
      loading.value = true
    }

    try {
      while (true) {
        const page =
          targetUserId === SELF_USER_ID
            ? expectData(
                await meListClientKeys<true>(
                  withData({
                    query: { first: requestedFirst, rows: requestedRows },
                  }),
                ),
              )
            : expectData(
                await listClientKeys<true>(
                  withData({
                    path: { user_id: targetUserId },
                    query: { first: requestedFirst, rows: requestedRows },
                  }),
                ),
              )

        if (!ownsView()) return page.keys

        keys.value = page.keys
        total.value = page.total
        first.value = page.first
        rows.value = page.rows
        if (
          page.keys.length === 0 &&
          page.total > 0 &&
          page.first >= page.total
        ) {
          requestedFirst = Math.floor((page.total - 1) / page.rows) * page.rows
          requestedRows = page.rows
          first.value = requestedFirst
          rows.value = requestedRows
          continue
        }
        return page.keys
      }
    } catch (error) {
      if (!ownsView()) return []
      throw error
    } finally {
      if (ownsView()) loading.value = false
    }
  }

  async function changeSelectedUser(userId: number): Promise<void> {
    selectedUserId.value = userId
    await loadKeys(userId, 0, rows.value)
  }

  // The guard lives here rather than at each call site, so a key can be gated
  // against every operation that would write it — including a label committed
  // from the keyboard, which never goes through a dialog.
  const guard = useAsyncAction()
  const actions = createApiKeyActions({
    guard,
    loadKeys: (targetUserId) => loadKeys(targetUserId),
    selectedUserId,
  })

  // Per-row view of the guard. `loading` stays the fetch-only busy flag, so a
  // write never blanks the whole page.
  const clientKeyPendingActions = computed(() =>
    pendingRowsById(guard.pendingActions.value, CLIENT_KEY_PREFIX),
  )

  return {
    ...actions,
    changeSelectedUser,
    clientKeyPendingActions,
    first,
    hasUsers,
    isPending: guard.isPending,
    keys,
    loadKeys,
    loadUsers,
    loading,
    pendingActions: guard.pendingActions,
    rows,
    selectedUser,
    selectedUserId,
    total,
    users,
    usersLoading,
  }
})
