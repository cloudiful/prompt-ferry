import type { QueryClient, UseQueryOptions } from '@tanstack/react-query'
import type { ClientKeyPageResponse } from '@/generated/admin-api'
import {
  listClientKeysOptions,
  listClientKeysQueryKey,
  meListClientKeysOptions,
  meListClientKeysQueryKey,
  meListModelsQueryKey,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { SELF_USER_ID } from '@/models/api-keys'

export function clientKeyPagesQueryKey(userId: number) {
  return userId === SELF_USER_ID
    ? meListClientKeysQueryKey()
    : listClientKeysQueryKey({ path: { user_id: userId } })
}

export function clientKeyPageQueryKey(
  userId: number,
  first: number,
  rows: number,
) {
  return userId === SELF_USER_ID
    ? meListClientKeysQueryKey({ query: { first, rows } })
    : listClientKeysQueryKey({
        path: { user_id: userId },
        query: { first, rows },
      })
}

/**
 * The options of one client-key page.
 *
 * The self list and the owner list are two endpoints with one result shape,
 * and the page reads either of them, so their generated options are widened to
 * one query type.
 */
export type ClientKeyPageOptions = UseQueryOptions<
  ClientKeyPageResponse,
  Error,
  ClientKeyPageResponse,
  readonly unknown[]
>

export function clientKeyPageOptions(
  userId: number,
  first: number,
  rows: number,
): ClientKeyPageOptions {
  return (userId === SELF_USER_ID
    ? meListClientKeysOptions({ query: { first, rows } })
    : listClientKeysOptions({
        path: { user_id: userId },
        query: { first, rows },
      })) as unknown as ClientKeyPageOptions
}

export function availableModelsQueryKey(first: number, rows: number) {
  return meListModelsQueryKey({ query: { first, rows } })
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
    const page = await queryClient.fetchQuery(
      clientKeyPageOptions(userId, requestedFirst, rows),
    )

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
