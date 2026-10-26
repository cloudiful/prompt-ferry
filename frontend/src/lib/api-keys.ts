import type { QueryClient } from '@tanstack/react-query'
import {
  createClientKey,
  deleteClientKey,
  listClientKeys,
  listUserOptions,
  meCreateClientKey,
  meDeleteClientKey,
  meListClientKeys,
  meListModels,
  meUpdateClientKey,
  updateClientKey,
} from '@/generated/admin-api'
import type {
  AvailableModelsResponse,
  ClientKey,
  ClientKeyPageResponse,
  CreateClientKeyRequest,
  CreateClientKeyResponse,
  UpdateClientKeyRequest,
  User,
  UserOptionsResponse,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { SELF_USER_ID } from '@/models/api-keys'

const clientKeysRoot = ['client-keys'] as const

export function clientKeyUsersQueryKey(userId: number) {
  return [...clientKeysRoot, 'users', userId] as const
}

export function clientKeyPagesQueryKey(userId: number) {
  return [...clientKeysRoot, 'page', userId] as const
}

export function clientKeyPageQueryKey(
  userId: number,
  first: number,
  rows: number,
) {
  return [...clientKeyPagesQueryKey(userId), first, rows] as const
}

export function availableModelsQueryKey(first: number, rows: number) {
  return ['available-models', first, rows] as const
}

export async function listClientKeysPage(
  userId: number,
  first: number,
  rows: number,
): Promise<ClientKeyPageResponse> {
  const result =
    userId === SELF_USER_ID
      ? await meListClientKeys<true>(withData({ query: { first, rows } }))
      : await listClientKeys<true>(
          withData({ path: { user_id: userId }, query: { first, rows } }),
        )
  return expectData(result)
}

export async function listClientKeyUsers(): Promise<User[]> {
  const result = expectData(
    await listUserOptions<true>(withData()),
  ) as UserOptionsResponse
  return result.users
}

export async function createClientKeyForUser(
  userId: number,
  body: CreateClientKeyRequest,
): Promise<CreateClientKeyResponse> {
  const result =
    userId === SELF_USER_ID
      ? await meCreateClientKey<true>(withData({ body }))
      : await createClientKey<true>(
          withData({ path: { user_id: userId }, body }),
        )
  return expectData(result)
}

export async function updateClientKeyForUser(
  userId: number,
  keyId: string,
  body: UpdateClientKeyRequest,
): Promise<ClientKey> {
  const result =
    userId === SELF_USER_ID
      ? await meUpdateClientKey<true>(
          withData({ path: { key_id: keyId }, body }),
        )
      : await updateClientKey<true>(
          withData({ path: { user_id: userId, key_id: keyId }, body }),
        )
  return expectData(result)
}

export async function deleteClientKeyForUser(
  userId: number,
  keyId: string,
): Promise<void> {
  if (userId === SELF_USER_ID) {
    await meDeleteClientKey<true>(withData({ path: { key_id: keyId } }))
  } else {
    await deleteClientKey<true>(
      withData({ path: { user_id: userId, key_id: keyId } }),
    )
  }
}

export async function listAvailableModels(
  first: number,
  rows: number,
): Promise<AvailableModelsResponse> {
  return expectData(
    await meListModels<true>(withData({ query: { first, rows } })),
  )
}

export async function refreshClientKeyPage(
  queryClient: QueryClient,
  userId: number,
  first: number,
  rows: number,
): Promise<ClientKeyPageResponse> {
  const currentPageKey = clientKeyPageQueryKey(userId, first, rows)
  await queryClient.cancelQueries({ queryKey: currentPageKey, exact: true })
  await queryClient.invalidateQueries({
    queryKey: clientKeyPagesQueryKey(userId),
    refetchType: 'none',
  })
  let requestedFirst = first

  while (true) {
    const page = await queryClient.fetchQuery({
      queryKey: clientKeyPageQueryKey(userId, requestedFirst, rows),
      queryFn: () => listClientKeysPage(userId, requestedFirst, rows),
      staleTime: 0,
    })

    if (
      page.rows > 0 &&
      page.keys.length === 0 &&
      requestedFirst >= page.total
    ) {
      const correctedFirst =
        page.total > 0
          ? Math.floor((page.total - 1) / page.rows) * page.rows
          : 0
      if (correctedFirst !== requestedFirst) {
        requestedFirst = correctedFirst
        continue
      }
    }

    return page
  }
}
