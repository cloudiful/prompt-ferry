import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  createClientKey,
  deleteClientKey,
  listClientKeys,
  listUserOptions,
  meCreateClientKey,
  meDeleteClientKey,
  meListClientKeys,
  meUpdateClientKey,
  updateClientKey,
} from '../generated/admin-api'
import type {
  ClientKey,
  CreateClientKeyResponse,
  UpdateClientKeyRequest,
  User,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'

export const SELF_USER_ID = 0

export const useApiKeysStore = defineStore('api-keys', () => {
  const users = ref<User[]>([])
  const selectedUserId = ref<number>(SELF_USER_ID)
  const keys = ref<ClientKey[]>([])
  const loading = ref(false)
  const usersLoading = ref(false)
  const first = ref(0)
  const rows = useStoredPageSize('api-keys', 10, STANDARD_PAGE_SIZE_OPTIONS)
  const total = ref(0)

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
    first.value = nextFirst
    rows.value = nextRows
    loading.value = true
    try {
      const page =
        targetUserId === SELF_USER_ID
          ? expectData(
              await meListClientKeys<true>(
                withData({ query: { first: first.value, rows: rows.value } }),
              ),
            )
          : expectData(
              await listClientKeys<true>(
                withData({
                  path: { user_id: targetUserId },
                  query: { first: first.value, rows: rows.value },
                }),
              ),
            )
      keys.value = page.keys
      total.value = page.total
      first.value = page.first
      rows.value = page.rows
      if (keys.value.length === 0 && total.value > 0 && first.value >= total.value) {
        const previousFirst =
          Math.floor((total.value - 1) / rows.value) * rows.value
        return await loadKeys(targetUserId, previousFirst, rows.value)
      }
      return page.keys
    } finally {
      loading.value = false
    }
  }

  async function changeSelectedUser(userId: number): Promise<void> {
    selectedUserId.value = userId
    await loadKeys(userId, 0, rows.value)
  }

  async function createKey(
    label?: string | null,
    forceUserId?: number,
  ): Promise<CreateClientKeyResponse> {
    const targetUserId = forceUserId ?? selectedUserId.value
    const created =
      targetUserId === SELF_USER_ID
        ? expectData(
            await meCreateClientKey<true>(
              withData({ body: { label: label ?? null } }),
            ),
          )
        : expectData(
            await createClientKey<true>(
              withData({
                path: { user_id: targetUserId },
                body: { label: label ?? null },
              }),
            ),
          )
    await loadKeys(targetUserId)
    return created
  }

  async function saveKey(
    keyId: number,
    input: UpdateClientKeyRequest,
    forceUserId?: number,
  ): Promise<ClientKey> {
    const targetUserId = forceUserId ?? selectedUserId.value
    const saved =
      targetUserId === SELF_USER_ID
        ? expectData(
            await meUpdateClientKey<true>(
              withData({ path: { key_id: keyId }, body: input }),
            ),
          )
        : expectData(
            await updateClientKey<true>(
              withData({
                path: { user_id: targetUserId, key_id: keyId },
                body: input,
              }),
            ),
          )
    await loadKeys(targetUserId)
    return saved
  }

  async function removeKey(keyId: number, forceUserId?: number): Promise<void> {
    const targetUserId = forceUserId ?? selectedUserId.value
    if (targetUserId === SELF_USER_ID) {
      await meDeleteClientKey<true>(withData({ path: { key_id: keyId } }))
    } else {
      await deleteClientKey<true>(
        withData({ path: { user_id: targetUserId, key_id: keyId } }),
      )
    }
    await loadKeys(targetUserId)
  }

  return {
    changeSelectedUser,
    createKey,
    first,
    hasUsers,
    keys,
    loadKeys,
    loadUsers,
    loading,
    removeKey,
    rows,
    saveKey,
    selectedUser,
    selectedUserId,
    total,
    users,
    usersLoading,
  }
})
