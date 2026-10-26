/**
 * The admin Review settings tab: the LLM review settings, read and saved by
 * hand.
 *
 * The stored settings include two secrets that the read echoes back verbatim, so
 * this store adds one rule the other tabs do not need: nothing can be saved
 * before a read has succeeded. Sending the empty default form would otherwise
 * clear the stored key and webhook token.
 *
 * Everything else follows the network tab: one scoped read through the app query
 * client, a read generation so a superseded response cannot commit, one guarded
 * write under `setting:llmReview` with the form captured before the first await,
 * a leaf-by-leaf merge that keeps a newer edit, and a success report only for a
 * write that reached the server.
 */

import type { QueryClient } from '@tanstack/react-query'
import { create } from 'zustand'
import type { LlmReviewSettings, MeResponse } from '@/generated/admin-api'
import { getLlmReviewSetting, setLlmReviewSetting } from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { capturePayload } from '@/stores/record-action-types'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import {
  LLM_REVIEW_KEY,
  createLlmReviewForm,
  formFromSettings,
  llmReviewDirtyFields,
  llmReviewFormToRequest,
  mergeLlmReviewResponse,
  parseLlmReviewHeaders,
  type LlmReviewField,
  type LlmReviewForm,
} from './settings-review-model'

const SETTINGS_REVIEW_NAMESPACE = ['settings-review'] as const
const LLM_REVIEW_QUERY_KEY = [
  ...SETTINGS_REVIEW_NAMESPACE,
  'llm-review',
] as const

type SettingsIdentity = Pick<MeResponse, 'user_id' | 'login_name' | 'is_admin'>

export function reviewIdentityKey(identity: SettingsIdentity | null): string {
  if (!identity) return ''
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin}`
}

type SettingsReviewState = {
  identityKey: string
  form: LlmReviewForm
  baseline: LlmReviewForm
  loaded: boolean
  loading: boolean
  loadError: boolean
  saveFailed: boolean
  initializeIdentity: (identity: SettingsIdentity | null) => void
  resetIdentity: (identity: SettingsIdentity | null) => void
  ensureLoaded: (queryClient: QueryClient) => Promise<void>
  refresh: (queryClient: QueryClient) => Promise<void>
  setField: <K extends LlmReviewField>(
    field: K,
    value: LlmReviewForm[K],
  ) => void
  save: () => Promise<AsyncActionOutcome<void>>
}

let readGeneration = 0
export type SettingsReviewNotifier = {
  /** A request failed; the caller formats the API error, never the payload. */
  notifyApiError: (cause: unknown) => void
  /** The extra-header text is malformed; nothing was sent. */
  notifyHeaderFormat: () => void
  notifySaved: () => void
}

let reportedNotifier: SettingsReviewNotifier | null = null

/**
 * Register the notifications the tab reports through, so the store never reaches
 * into a React context and the tab owns the notification surface.
 */
export function setSettingsReviewNotifier(
  notifier: SettingsReviewNotifier | null,
): void {
  reportedNotifier = notifier
}

/** Drop a read that is still in flight once the operator edits or saves. */
function invalidatePendingRead(): void {
  if (!useSettingsReviewStore.getState().loading) return
  ++readGeneration
  useSettingsReviewStore.setState({ loading: false })
}

/**
 * Drop the tab on unmount: an in-flight read commits nothing, the loading flag
 * is cleared, and the form — which holds the two stored secrets — is emptied so
 * no secret draft survives on the module that outlives the tab. A later mount
 * reads the settings again, so nothing is shown from the previous visit.
 */
export function disposeSettingsReview(): void {
  ++readGeneration
  useSettingsReviewStore.setState({
    form: createLlmReviewForm(),
    baseline: createLlmReviewForm(),
    loaded: false,
    loading: false,
    loadError: false,
    saveFailed: false,
  })
}

async function fetchFresh(
  queryClient: QueryClient,
): Promise<LlmReviewSettings> {
  await queryClient.cancelQueries({
    queryKey: LLM_REVIEW_QUERY_KEY,
    exact: true,
  })
  await queryClient.invalidateQueries({
    queryKey: SETTINGS_REVIEW_NAMESPACE,
    refetchType: 'none',
  })
  return queryClient.fetchQuery({
    queryKey: LLM_REVIEW_QUERY_KEY,
    queryFn: async () =>
      expectData(await getLlmReviewSetting<true>(withData())),
    staleTime: 0,
  })
}

export const useSettingsReviewStore = create<SettingsReviewState>(
  (set, get) => ({
    identityKey: '',
    form: createLlmReviewForm(),
    baseline: createLlmReviewForm(),
    loaded: false,
    loading: false,
    loadError: false,
    saveFailed: false,

    initializeIdentity(identity) {
      const key = reviewIdentityKey(identity)
      if (get().identityKey === key) return
      get().resetIdentity(identity)
    },

    resetIdentity(identity) {
      ++readGeneration
      set({
        identityKey: reviewIdentityKey(identity),
        form: createLlmReviewForm(),
        baseline: createLlmReviewForm(),
        loaded: false,
        loading: false,
        loadError: false,
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
      // A refresh never discards an unsaved draft.
      if (llmReviewDirtyFields(state.form, state.baseline).length > 0) return
      const identityKey = state.identityKey
      const generation = ++readGeneration
      set({ loading: true, loadError: false })
      try {
        const settings = await fetchFresh(queryClient)
        if (generation !== readGeneration) return
        if (get().identityKey !== identityKey) return
        const form = formFromSettings(settings)
        set({
          form,
          baseline: form,
          loaded: true,
          loading: false,
          loadError: false,
          saveFailed: false,
        })
      } catch {
        if (generation !== readGeneration) return
        if (get().identityKey !== identityKey) return
        set({ loading: false, loadError: true })
      }
    },

    setField(field, value) {
      invalidatePendingRead()
      set({ form: { ...get().form, [field]: value } })
    },

    async save() {
      const state = get()
      // Saving before a successful read would send the empty defaults and clear
      // the stored key and webhook token.
      if (state.identityKey === '' || !state.loaded) return { executed: false }
      const identityKey = state.identityKey
      const submitted = capturePayload(state.form)
      invalidatePendingRead()

      let headerFormatError = false
      try {
        const outcome = await runAsyncAction(
          LLM_REVIEW_KEY,
          async () => {
            const parsed = parseLlmReviewHeaders(submitted.webhook_headers_text)
            if (!parsed.ok) {
              headerFormatError = true
              // The guard releases the key; nothing reached the server.
              throw new LlmReviewHeaderFormatError()
            }
            const body = llmReviewFormToRequest(submitted, parsed.headers)
            const response = expectData(
              await setLlmReviewSetting<true>(withData({ body })),
            )
            if (get().identityKey !== identityKey) return
            const latest = get()
            set({
              form: mergeLlmReviewResponse(latest.form, submitted, response),
              // The baseline is the last accepted value, per leaf: a leaf edited
              // during the write stays dirty against it.
              baseline: formFromSettings(response),
              loaded: true,
              saveFailed: false,
            })
          },
          'settings',
        )
        if (outcome.executed) {
          set({ saveFailed: false })
          reportedNotifier?.notifySaved()
        }
        return outcome
      } catch (cause) {
        set({ saveFailed: true })
        if (headerFormatError) reportedNotifier?.notifyHeaderFormat()
        else reportedNotifier?.notifyApiError(cause)
        return { executed: false }
      }
    },
  }),
)

/** A malformed extra-header line, reported locally without any request. */
export class LlmReviewHeaderFormatError extends Error {
  constructor() {
    super('Header format must be Name: value')
    this.name = 'LlmReviewHeaderFormatError'
  }
}
