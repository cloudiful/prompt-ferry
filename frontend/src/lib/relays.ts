import type { QueryClient } from '@tanstack/react-query'
import type { ManagedRelayListResponse } from '@/generated/admin-api'
import {
  listRelaysOptions,
  listRelaysQueryKey,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { recordActionKey } from '@/stores/record-action-types'

export const RELAY_ACTION_PREFIX = 'relay'
export const RELAY_CREATE_KEY = recordActionKey(RELAY_ACTION_PREFIX, null)

export function relayActionKey(relayId: string | null): string {
  return recordActionKey(RELAY_ACTION_PREFIX, relayId)
}

export function relayPagesQueryKey() {
  return listRelaysQueryKey()
}

export function relayPageQueryKey(first: number, rows: number) {
  return listRelaysQueryKey({ query: { first, rows } })
}

/**
 * Reload one page of the relay list after a write: cancel the read of this
 * page, drop every cached copy of the list, then read the page again.
 */
export async function refreshRelayPage(
  queryClient: QueryClient,
  first: number,
  rows: number,
): Promise<{ first: number; page: ManagedRelayListResponse }> {
  await queryClient.cancelQueries({
    queryKey: relayPageQueryKey(first, rows),
    exact: true,
  })
  await queryClient.invalidateQueries({
    queryKey: relayPagesQueryKey(),
    refetchType: 'none',
  })
  const page = await queryClient.fetchQuery(
    listRelaysOptions({ query: { first, rows } }),
  )
  return { first: page.first, page }
}
