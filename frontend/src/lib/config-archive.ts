/**
 * The configuration archive's server reads and the identity wiring the
 * settings page keeps for this tab.
 *
 * The archive's passphrases, selected file, preview and applied summary are
 * transient component state owned by the tab and its forms; only the audit
 * trail is server data and it belongs to TanStack Query under the generated
 * key. Every secret stays in memory: nothing here writes a passphrase, an
 * archive body or a selected file beyond the component that holds it.
 */

import { type QueryClient, type QueryKey } from '@tanstack/react-query'
import type { ReactFormExtendedApi } from '@tanstack/react-form'
import type {
  ConfigAuditEntry,
  ConfigAuditPage,
  MeResponse,
} from '@/generated/admin-api'
import {
  listConfigAuditOptions,
  listConfigAuditQueryKey,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { configArchivePreviousPageFirst } from './config-archive-model'

type ConfigArchiveIdentity = Pick<
  MeResponse,
  'user_id' | 'login_name' | 'is_admin'
>

export function configArchiveIdentityKey(
  identity: ConfigArchiveIdentity | null,
): string {
  if (!identity) return ''
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin}`
}

export function configArchiveAuditOptions(first: number, rows: number) {
  return listConfigAuditOptions({ query: { first, rows } })
}

export function configArchiveAuditQueryKey(first: number, rows: number) {
  return listConfigAuditQueryKey({ query: { first, rows } })
}

/**
 * Read one audit page for the current identity.
 *
 * An empty window at or past the total is not an empty trail: the last valid
 * page is read once instead. The caller keeps the returned page's `first` so
 * the pager follows a shrinking trail.
 */
export async function refreshConfigArchiveAudit(
  queryClient: QueryClient,
  first: number,
  rows: number,
): Promise<ConfigAuditPage> {
  let requestedFirst = first
  while (true) {
    const options = configArchiveAuditOptions(requestedFirst, rows)
    const queryKey = configArchiveAuditQueryKey(
      requestedFirst,
      rows,
    ) as QueryKey
    await queryClient.cancelQueries({ queryKey, exact: true })
    const page = (await queryClient.fetchQuery({
      ...options,
      staleTime: 0,
    } as never)) as ConfigAuditPage
    const previous = configArchivePreviousPageFirst(
      page.first,
      page.rows,
      page.total,
      page.entries.length,
    )
    if (previous !== null && previous !== requestedFirst) {
      requestedFirst = previous
      continue
    }
    return page
  }
}

export type { ConfigArchiveIdentity, ConfigAuditEntry }

export type ConfigArchivePassphraseValues = { passphrase: string }

export type ConfigArchiveExportForm = ReactFormExtendedApi<
  ConfigArchivePassphraseValues,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined,
  undefined
>

export type ConfigArchiveImportForm = ConfigArchiveExportForm
