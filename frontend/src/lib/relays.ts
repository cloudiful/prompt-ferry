import type { QueryClient } from '@tanstack/react-query'
import type {
  ManagedRelay,
  ManagedRelayListResponse,
  ManagedRelayPatchRequest,
  ManagedRelayRequest,
} from '@/generated/admin-api'
import {
  createRelay,
  deleteRelay,
  listRelays,
  reconnectRelay,
  updateRelay,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { recordActionKey } from '@/stores/record-action-types'

export const RELAY_ACTION_PREFIX = 'relay'
export const RELAY_CREATE_KEY = recordActionKey(RELAY_ACTION_PREFIX, null)

export function relayActionKey(relayId: string | null): string {
  return recordActionKey(RELAY_ACTION_PREFIX, relayId)
}

export function relayPageQueryKey(first: number, rows: number) {
  return ['relays', first, rows] as const
}

export async function loadRelayPage(
  first: number,
  rows: number,
): Promise<ManagedRelayListResponse> {
  return expectData(
    await listRelays<true>(withData({ query: { first, rows } })),
  )
}

export async function createManagedRelay(
  body: ManagedRelayRequest,
): Promise<ManagedRelay> {
  return expectData(await createRelay<true>(withData({ body })))
}

export async function updateManagedRelay(
  relayId: string,
  body: ManagedRelayPatchRequest,
): Promise<ManagedRelay> {
  return expectData(
    await updateRelay<true>(withData({ path: { relay_id: relayId }, body })),
  )
}

export async function deleteManagedRelay(relayId: string): Promise<void> {
  await deleteRelay<true>(withData({ path: { relay_id: relayId } }))
}

export async function reconnectManagedRelay(
  relayId: string,
): Promise<ManagedRelay> {
  return expectData(
    await reconnectRelay<true>(withData({ path: { relay_id: relayId } })),
  )
}

export async function loadRelayPageWithCorrection(
  first: number,
  rows: number,
): Promise<ManagedRelayListResponse> {
  const page = await loadRelayPage(first, rows)
  if (page.relays.length > 0 || page.total === 0 || page.first < page.total) {
    return page
  }

  const correctedFirst = Math.floor((page.total - 1) / page.rows) * page.rows
  return loadRelayPage(correctedFirst, page.rows)
}

export async function refreshRelayPage(
  queryClient: QueryClient,
  first: number,
  rows: number,
): Promise<{ first: number; page: ManagedRelayListResponse }> {
  const page = await queryClient.fetchQuery({
    queryKey: relayPageQueryKey(first, rows),
    queryFn: () => loadRelayPageWithCorrection(first, rows),
    staleTime: 0,
  })
  if (page.first !== first) {
    queryClient.setQueryData(relayPageQueryKey(page.first, page.rows), page)
  }
  return { first: page.first, page }
}
