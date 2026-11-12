import { type QueryClient, type QueryKey } from '@tanstack/react-query'
import type {
  ApprovalPageResponse,
  ApprovalRequest,
  ApprovalStatusFilter,
} from '@/generated/admin-api'
import {
  getApprovalOptions,
  getApprovalQueryKey,
  listApprovalsOptions,
  listApprovalsQueryKey,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { recordActionKey } from '@/stores/record-action-types'

/**
 * Approve and reject of one request share one key. They are two answers to the
 * same question, so only one of them may be in flight, and the key stays held
 * until the list and the open detail have settled behind the answer.
 */
export const APPROVAL_KEY_PREFIX = 'approval'

export const APPROVAL_PAGE_SIZE_KEY = 'approvals'
export const APPROVAL_PAGE_SIZE_FALLBACK = 10

export function approvalActionKey(approvalId: string): string {
  return recordActionKey(APPROVAL_KEY_PREFIX, approvalId)
}

export type ApprovalsIdentity = {
  user_id: number | null
  is_admin: boolean
}

export function approvalsIdentityKey(identity: ApprovalsIdentity): string {
  return `${identity.user_id ?? ''}:${identity.is_admin ? 'admin' : 'user'}`
}

/** The first row of the last page that still holds an approval. */
export function approvalsLastPageFirst(total: number, rows: number): number {
  return total > 0 && rows > 0 ? Math.floor((total - 1) / rows) * rows : 0
}

async function fetchFresh<T>(
  queryClient: QueryClient,
  queryKey: QueryKey,
  namespaceKey: QueryKey,
  options: Record<string, unknown>,
): Promise<T> {
  await queryClient.cancelQueries({ queryKey, exact: true })
  await queryClient.invalidateQueries({
    queryKey: namespaceKey,
    refetchType: 'none',
  })
  return queryClient.fetchQuery(options as never) as Promise<T>
}

/**
 * Read one page of approvals.
 *
 * An empty page beyond the end of the collection is not an empty result: the
 * pager asks for the last page that still holds rows and reads it again, so the
 * operator never lands on a blank page.
 */
export async function refreshApprovalList(
  queryClient: QueryClient,
  status: ApprovalStatusFilter,
  first: number,
  rows: number,
): Promise<ApprovalPageResponse> {
  let requestedFirst = first
  for (;;) {
    const query = { status, first: requestedFirst, rows }
    const page = await fetchFresh<ApprovalPageResponse>(
      queryClient,
      listApprovalsQueryKey({ query }),
      listApprovalsQueryKey(),
      {
        ...listApprovalsOptions({ query }),
        staleTime: 0,
      },
    )
    const pageRows = page.rows > 0 ? page.rows : rows
    if (
      pageRows > 0 &&
      page.approvals.length === 0 &&
      page.total > 0 &&
      requestedFirst >= page.total
    ) {
      const correctedFirst = approvalsLastPageFirst(page.total, pageRows)
      if (correctedFirst !== requestedFirst) {
        requestedFirst = correctedFirst
        continue
      }
    }
    return page
  }
}

export async function refreshApprovalDetail(
  queryClient: QueryClient,
  approvalId: string,
): Promise<ApprovalRequest> {
  const path = { approval_id: approvalId }
  return fetchFresh(
    queryClient,
    getApprovalQueryKey({ path }),
    getApprovalQueryKey({ path }),
    {
      ...getApprovalOptions({ path }),
      staleTime: 0,
    },
  )
}
