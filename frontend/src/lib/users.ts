import type { QueryClient } from '@tanstack/react-query'
import type { UserPageResponse } from '@/generated/admin-api'
import {
  listUsersOptions,
  listUsersQueryKey,
} from '@/generated/admin-api/@tanstack/react-query.gen'

export function usersPagesQueryKey() {
  return listUsersQueryKey()
}

export function usersPageQueryKey(first: number, rows: number) {
  return listUsersQueryKey({ query: { first, rows } })
}

/**
 * Reload one page of the user list after a write: cancel the read of this
 * page, drop every cached copy of the list, then read the page again.
 */
export async function refreshUsersPage(
  queryClient: QueryClient,
  first: number,
  rows: number,
): Promise<UserPageResponse> {
  await queryClient.cancelQueries({
    queryKey: usersPageQueryKey(first, rows),
    exact: true,
  })
  await queryClient.invalidateQueries({
    queryKey: usersPagesQueryKey(),
    refetchType: 'none',
  })
  return queryClient.fetchQuery(listUsersOptions({ query: { first, rows } }))
}
