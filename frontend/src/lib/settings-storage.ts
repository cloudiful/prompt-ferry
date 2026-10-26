/**
 * The admin Storage settings tab: the raw object store, read and saved by hand.
 *
 * Two things make this tab different from the network and review ports. The
 * first is that the two S3 credentials are write-only: the server echoes only
 * their presence, so the store keeps the operator's drafts apart from the flags
 * and can never display a stored secret. The second is that the whole setting is
 * unavailable on a backend that cannot keep it, which the server answers with a
 * structured capability code; that becomes an explicit state, so the warning and
 * its retry read do not depend on matching an error message.
 *
 * Everything else follows the accepted pattern: one scoped read through the app
 * query client with a read generation, one guarded write under `setting:storage`
 * with the form captured before the first await, a per-field merge that keeps a
 * newer edit, secret drafts cleared only where a save consumed them, and a
 * success report only for a write that reached the server.
 */

import type { QueryClient } from '@tanstack/react-query'
import { create } from 'zustand'
import type {
  MeResponse,
  RawObjectStoreSettingsResponse,
} from '@/generated/admin-api'
import { getRawObjectStore, setRawObjectStore } from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { capturePayload } from '@/stores/record-action-types'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import {
  STORAGE_KEY,
  clearSavedSecretDrafts,
  createRawObjectStoreForm,
  formFromRawObjectStore,
  isRawObjectStoreCapabilityUnavailable,
  mergeRawObjectStoreResponse,
  presenceFromRawObjectStore,
  rawObjectStoreDirty,
  rawObjectStoreFormToRequest,
  storageBucketMissing,
  type RawObjectStoreField,
  type RawObjectStoreForm,
  type RawObjectStorePresence,
} from './settings-storage-model'

const SETTINGS_STORAGE_NAMESPACE = ['settings-storage'] as const
const RAW_OBJECT_STORE_QUERY_KEY = [
  ...SETTINGS_STORAGE_NAMESPACE,
  'raw-object-store',
] as const

type SettingsIdentity = Pick<MeResponse, 'user_id' | 'login_name' | 'is_admin'>

export function storageIdentityKey(identity: SettingsIdentity | null): string {
  if (!identity) return ''
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin}`
}

export type SettingsStorageState = {
  identityKey: string
  form: RawObjectStoreForm
  baseline: RawObjectStoreForm
  presence: RawObjectStorePresence
  loaded: boolean
  loading: boolean
  loadError: boolean
  unavailable: boolean
  unavailableMessage: string
  saveFailed: boolean
  initializeIdentity: (identity: SettingsIdentity | null) => void
  resetIdentity: (identity: SettingsIdentity | null) => void
  ensureLoaded: (queryClient: QueryClient) => Promise<void>
  refresh: (queryClient: QueryClient) => Promise<void>
  setField: <K extends RawObjectStoreField>(
    field: K,
    value: RawObjectStoreForm[K],
  ) => void
  save: () => Promise<AsyncActionOutcome<void>>
}

let readGeneration = 0
let reportedNotifier: {
  notifyApiError: (cause: unknown) => void
  notifyBucketRequired: () => void
  notifySaved: () => void
} | null = null

/**
 * Register the notifications the tab reports through, so the store never reaches
 * into a React context and the tab owns the notification surface.
 */
export function setSettingsStorageNotifier(
  notifier: {
    notifyApiError: (cause: unknown) => void
    notifyBucketRequired: () => void
    notifySaved: () => void
  } | null,
): void {
  reportedNotifier = notifier
}

/** Drop a read that is still in flight once the operator edits or saves. */
function invalidatePendingRead(): void {
  if (!useSettingsStorageStore.getState().loading) return
  ++readGeneration
  useSettingsStorageStore.setState({ loading: false })
}

/**
 * Drop the tab: an in-flight read commits nothing, and the form — which holds
 * the operator's secret drafts — is emptied so nothing sensitive survives the
 * tab on the module that outlives it.
 */
export function disposeSettingsStorage(): void {
  ++readGeneration
  useSettingsStorageStore.setState({
    form: createRawObjectStoreForm(),
    baseline: createRawObjectStoreForm(),
    presence: { has_s3_access_key: false, has_s3_secret_key: false },
    loaded: false,
    loading: false,
    loadError: false,
    unavailable: false,
    unavailableMessage: '',
    saveFailed: false,
  })
}

async function fetchFresh(
  queryClient: QueryClient,
): Promise<RawObjectStoreSettingsResponse> {
  await queryClient.cancelQueries({
    queryKey: RAW_OBJECT_STORE_QUERY_KEY,
    exact: true,
  })
  await queryClient.invalidateQueries({
    queryKey: SETTINGS_STORAGE_NAMESPACE,
    refetchType: 'none',
  })
  return queryClient.fetchQuery({
    queryKey: RAW_OBJECT_STORE_QUERY_KEY,
    queryFn: async () => expectData(await getRawObjectStore<true>(withData())),
    staleTime: 0,
  })
}

export const useSettingsStorageStore = create<SettingsStorageState>(
  (set, get) => ({
    identityKey: '',
    form: createRawObjectStoreForm(),
    baseline: createRawObjectStoreForm(),
    presence: { has_s3_access_key: false, has_s3_secret_key: false },
    loaded: false,
    loading: false,
    loadError: false,
    unavailable: false,
    unavailableMessage: '',
    saveFailed: false,

    initializeIdentity(identity) {
      const key = storageIdentityKey(identity)
      if (get().identityKey === key) return
      get().resetIdentity(identity)
    },

    resetIdentity(identity) {
      ++readGeneration
      set({
        identityKey: storageIdentityKey(identity),
        form: createRawObjectStoreForm(),
        baseline: createRawObjectStoreForm(),
        presence: { has_s3_access_key: false, has_s3_secret_key: false },
        loaded: false,
        loading: false,
        loadError: false,
        unavailable: false,
        unavailableMessage: '',
        saveFailed: false,
      })
    },

    async ensureLoaded(queryClient) {
      const state = get()
      if (state.identityKey === '' || state.loaded || state.loading) return
      await get().refresh(queryClient)
    },

    async refresh(queryClient) {
      const state = get()
      if (state.identityKey === '') return
      if (rawObjectStoreDirty(state.form, state.baseline)) return
      const identityKey = state.identityKey
      const generation = ++readGeneration
      set({ loading: true, loadError: false })
      try {
        const response = await fetchFresh(queryClient)
        if (generation !== readGeneration) return
        if (get().identityKey !== identityKey) return
        const form = formFromRawObjectStore(response)
        set({
          form,
          baseline: form,
          presence: presenceFromRawObjectStore(response),
          loaded: true,
          loading: false,
          loadError: false,
          unavailable: false,
          unavailableMessage: '',
          saveFailed: false,
        })
      } catch (cause) {
        if (generation !== readGeneration) return
        if (get().identityKey !== identityKey) return
        const unavailable = isRawObjectStoreCapabilityUnavailable(cause)
        set({
          loading: false,
          loadError: true,
          unavailable,
          unavailableMessage: unavailable ? formatCause(cause) : '',
        })
      }
    },

    setField(field, value) {
      invalidatePendingRead()
      set({ form: { ...get().form, [field]: value } })
    },

    async save() {
      const state = get()
      // Saving before a successful read would submit the empty defaults over a
      // configured store.
      if (state.identityKey === '' || !state.loaded || state.unavailable) {
        return { executed: false }
      }
      const identityKey = state.identityKey
      const submitted = capturePayload(state.form)
      invalidatePendingRead()

      let bucketMissing = false
      try {
        const outcome = await runAsyncAction(
          STORAGE_KEY,
          async () => {
            if (storageBucketMissing(submitted)) {
              bucketMissing = true
              // The guard releases the key; nothing reached the server.
              throw new StorageBucketRequiredError()
            }
            const body = rawObjectStoreFormToRequest(submitted)
            const response = expectData(
              await setRawObjectStore<true>(withData({ body })),
            )
            if (get().identityKey !== identityKey) return
            const latest = get()
            set({
              form: clearSavedSecretDrafts(
                mergeRawObjectStoreResponse(latest.form, submitted, response),
                submitted,
              ),
              // The baseline is the last accepted value, per field: a field or a
              // secret draft edited during the write stays dirty against it.
              baseline: formFromRawObjectStore(response),
              presence: presenceFromRawObjectStore(response),
              loaded: true,
              saveFailed: false,
            })
          },
          'storage',
        )
        if (outcome.executed) {
          set({ saveFailed: false })
          reportedNotifier?.notifySaved()
        }
        return outcome
      } catch (cause) {
        set({ saveFailed: true })
        if (bucketMissing) reportedNotifier?.notifyBucketRequired()
        else reportedNotifier?.notifyApiError(cause)
        return { executed: false }
      }
    },
  }),
)

/** The S3 bucket is required for the s3 backend; reported before any request. */
export class StorageBucketRequiredError extends Error {
  constructor() {
    super('S3 bucket is required')
    this.name = 'StorageBucketRequiredError'
  }
}

/** A capability answer's own text, without any request payload. */
function formatCause(cause: unknown): string {
  if (cause && typeof cause === 'object' && 'error' in cause) {
    const envelope = (cause as { error?: unknown }).error
    if (envelope && typeof envelope === 'object') {
      const code = (envelope as { code?: unknown }).code
      const message = (envelope as { message?: unknown }).message
      if (typeof code === 'string' && typeof message === 'string') {
        return `${code}: ${message}`
      }
      if (typeof message === 'string') return message
    }
  }
  return ''
}
