import { create } from 'zustand'
import type { QueryClient } from '@tanstack/react-query'
import type {
  ApprovalPageResponse,
  ApprovalRequest,
  ApprovalStatusFilter,
} from '@/generated/admin-api'
import {
  approveApproval,
  getApproval,
  listApprovals,
  rejectApproval,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import { useSessionStore } from './session'
import { readStorage, writeStorage } from '@/storage'
import {
  APPROVAL_PAGE_SIZE_OPTIONS,
  normalizePageSize,
  pageSizeStorageKey,
} from '@/models/pagination'
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

export const APPROVALS_LIST_NAMESPACE = ['approvals', 'list'] as const
export const APPROVALS_DETAIL_NAMESPACE = ['approvals', 'detail'] as const

export function approvalsListQueryKey(
  identity: ApprovalsIdentity,
  status: ApprovalStatusFilter,
  first: number,
  rows: number,
) {
  return [
    ...APPROVALS_LIST_NAMESPACE,
    approvalsIdentityKey(identity),
    status,
    first,
    rows,
  ] as const
}

export function approvalDetailQueryKey(
  identity: ApprovalsIdentity,
  approvalId: string,
) {
  return [
    ...APPROVALS_DETAIL_NAMESPACE,
    approvalsIdentityKey(identity),
    approvalId,
  ] as const
}

export async function loadApprovalPage(
  status: ApprovalStatusFilter,
  first: number,
  rows: number,
): Promise<ApprovalPageResponse> {
  return expectData(
    await listApprovals<true>(withData({ query: { status, first, rows } })),
  )
}

export async function loadApprovalDetail(
  approvalId: string,
): Promise<ApprovalRequest> {
  return expectData(
    await getApproval<true>(withData({ path: { approval_id: approvalId } })),
  )
}

async function fetchFresh<T>(
  queryClient: QueryClient,
  queryKey: readonly unknown[],
  namespace: readonly unknown[],
  load: () => Promise<T>,
): Promise<T> {
  await queryClient.cancelQueries({ queryKey, exact: true })
  await queryClient.invalidateQueries({
    queryKey: namespace,
    refetchType: 'none',
  })
  return queryClient.fetchQuery({ queryKey, queryFn: load, staleTime: 0 })
}

/** The first row of the last page that still holds an approval. */
export function approvalsLastPageFirst(total: number, rows: number): number {
  return total > 0 && rows > 0 ? Math.floor((total - 1) / rows) * rows : 0
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
  identity: ApprovalsIdentity,
  status: ApprovalStatusFilter,
  first: number,
  rows: number,
): Promise<ApprovalPageResponse> {
  let requestedFirst = first
  for (;;) {
    const page = await fetchFresh(
      queryClient,
      approvalsListQueryKey(identity, status, requestedFirst, rows),
      APPROVALS_LIST_NAMESPACE,
      () => loadApprovalPage(status, requestedFirst, rows),
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

const pageSizeKey = pageSizeStorageKey(APPROVAL_PAGE_SIZE_KEY)

function initialPageSize(): number {
  return normalizePageSize(
    readStorage(pageSizeKey),
    APPROVAL_PAGE_SIZE_FALLBACK,
    APPROVAL_PAGE_SIZE_OPTIONS,
  )
}

export type ApprovalsState = {
  identityKey: string | null
  identityUserId: number | null
  isAdmin: boolean
  status: ApprovalStatusFilter
  approvals: ApprovalRequest[]
  total: number
  first: number
  rows: number
  loading: boolean
  detail: ApprovalRequest | null
  detailId: string | null
  detailLoading: boolean
  initializeIdentity: (identity: ApprovalsIdentity) => void
  resetIdentity: (identity: ApprovalsIdentity | null) => void
  setStatus: (status: ApprovalStatusFilter) => void
  setPage: (first: number, rows: number) => void
  refresh: (queryClient: QueryClient) => Promise<void>
  loadList: (
    queryClient: QueryClient,
    first?: number,
    rows?: number,
  ) => Promise<void>
  openDetail: (queryClient: QueryClient, approvalId: string) => Promise<void>
  reloadDetail: (queryClient: QueryClient, approvalId: string) => Promise<void>
  closeDetail: () => void
  approve: (
    queryClient: QueryClient,
    approvalId: string,
  ) => Promise<AsyncActionOutcome<void>>
  reject: (
    queryClient: QueryClient,
    approvalId: string,
  ) => Promise<AsyncActionOutcome<void>>
}

let listGeneration = 0
let detailGeneration = 0

export const useApprovalsStore = create<ApprovalsState>((set, get) => ({
  identityKey: null,
  identityUserId: null,
  isAdmin: false,
  status: 'pending',
  approvals: [],
  total: 0,
  first: 0,
  rows: initialPageSize(),
  loading: false,
  detail: null,
  detailId: null,
  detailLoading: false,

  initializeIdentity(identity) {
    if (get().identityKey === approvalsIdentityKey(identity)) return
    get().resetIdentity(identity)
  },

  resetIdentity(identity) {
    const key = identity ? approvalsIdentityKey(identity) : null
    ++listGeneration
    ++detailGeneration
    set({
      identityKey: key,
      identityUserId: identity?.user_id ?? null,
      isAdmin: identity?.is_admin === true,
      status: 'pending',
      approvals: [],
      total: 0,
      first: 0,
      rows: initialPageSize(),
      loading: false,
      detail: null,
      detailId: null,
      detailLoading: false,
    })
  },

  setStatus(status) {
    set({ status, first: 0 })
  },

  setPage(first, rows) {
    const nextRows = APPROVAL_PAGE_SIZE_OPTIONS.includes(rows)
      ? rows
      : get().rows
    const nextFirst = Math.max(0, Math.floor(first / nextRows) * nextRows)
    writeStorage(pageSizeKey, String(nextRows))
    set({ first: nextFirst, rows: nextRows })
  },

  async refresh(queryClient) {
    await get().loadList(queryClient)
  },

  async loadList(queryClient, first, rows) {
    const state = get()
    if (state.identityKey === null) return
    const identity: ApprovalsIdentity = {
      user_id: state.identityUserId,
      is_admin: state.isAdmin,
    }
    const nextStatus = state.status
    const nextRows =
      rows !== undefined && APPROVAL_PAGE_SIZE_OPTIONS.includes(rows)
        ? rows
        : state.rows
    const nextFirst = Math.max(0, first ?? state.first)
    const generation = ++listGeneration
    set({ loading: true })
    try {
      const page = await refreshApprovalList(
        queryClient,
        identity,
        nextStatus,
        nextFirst,
        nextRows,
      )
      if (generation !== listGeneration) return
      set({
        approvals: page.approvals,
        total: page.total,
        first: page.first,
        rows: page.rows,
        loading: false,
      })
    } catch (cause) {
      if (generation === listGeneration) {
        // Rows of another filter or page must not survive a failed read as if
        // they were the current result.
        set({ approvals: [], total: 0, loading: false })
      }
      throw cause
    }
  },

  async openDetail(queryClient, approvalId) {
    const state = get()
    if (state.identityKey === null) return
    const identity: ApprovalsIdentity = {
      user_id: state.identityUserId,
      is_admin: state.isAdmin,
    }
    const generation = ++detailGeneration
    // The dialog opens on a successful read only, so a failed read leaves it
    // closed instead of showing a stale approval.
    set({ detail: null, detailId: null, detailLoading: true })
    try {
      const detail = await fetchFresh(
        queryClient,
        approvalDetailQueryKey(identity, approvalId),
        APPROVALS_DETAIL_NAMESPACE,
        () => loadApprovalDetail(approvalId),
      )
      if (generation !== detailGeneration) return
      set({ detail, detailId: approvalId, detailLoading: false })
    } catch (cause) {
      if (generation === detailGeneration) {
        set({ detail: null, detailId: null, detailLoading: false })
      }
      throw cause
    }
  },

  async reloadDetail(queryClient, approvalId) {
    const state = get()
    if (state.identityKey === null) return
    const identity: ApprovalsIdentity = {
      user_id: state.identityUserId,
      is_admin: state.isAdmin,
    }
    const generation = ++detailGeneration
    set({ detailLoading: true })
    try {
      const detail = await fetchFresh(
        queryClient,
        approvalDetailQueryKey(identity, approvalId),
        APPROVALS_DETAIL_NAMESPACE,
        () => loadApprovalDetail(approvalId),
      )
      if (generation !== detailGeneration) return
      set({ detail, detailId: approvalId, detailLoading: false })
    } catch (cause) {
      if (generation === detailGeneration) {
        set({ detail: null, detailId: null, detailLoading: false })
      }
      throw cause
    }
  },

  closeDetail() {
    ++detailGeneration
    set({ detail: null, detailId: null, detailLoading: false })
  },

  async approve(queryClient, approvalId) {
    return answerApproval(queryClient, get, set, approvalId, 'approve')
  },

  async reject(queryClient, approvalId) {
    return answerApproval(queryClient, get, set, approvalId, 'reject')
  },
}))

async function answerApproval(
  queryClient: QueryClient,
  get: () => ApprovalsState,
  set: (partial: Partial<ApprovalsState>) => void,
  approvalId: string,
  action: 'approve' | 'reject',
): Promise<AsyncActionOutcome<void>> {
  // The id is captured before the request, so a later answer cannot ride along.
  const capturedId = approvalId
  return runAsyncAction(
    approvalActionKey(capturedId),
    async () => {
      if (action === 'approve') {
        await approveApproval<true>(
          withData({ path: { approval_id: capturedId } }),
        )
      } else {
        await rejectApproval<true>(
          withData({ path: { approval_id: capturedId } }),
        )
      }
      // The response body is not the truth; the list and the open detail are.
      await get().loadList(queryClient)
      if (get().detailId === capturedId) {
        await get().reloadDetail(queryClient, capturedId)
      }
    },
    action,
  )
}

useSessionStore.subscribe((state, previous) => {
  const currentKey = approvalsIdentityKey({
    user_id: state.me?.user_id ?? null,
    is_admin: state.me?.is_admin === true,
  })
  const previousKey = approvalsIdentityKey({
    user_id: previous.me?.user_id ?? null,
    is_admin: previous.me?.is_admin === true,
  })
  if (currentKey !== previousKey) {
    useApprovalsStore
      .getState()
      .resetIdentity(
        state.me
          ? { user_id: state.me.user_id, is_admin: state.me.is_admin === true }
          : null,
      )
  }
})
