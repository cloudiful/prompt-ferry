/**
 * The admin Requests settings tab: four reads, four guarded writes and the
 * autosave schedule that decides when a write is owed.
 *
 * The tab has no Save button. Every control edit arms a per-form debounce, and
 * the flush claims that form's resource key through the shared async-action
 * guard. The two retention controls share one key because the server keeps them
 * in one record: only one of them may be in flight, and the form that lost the
 * race retries once the key is released rather than spinning.
 *
 * Reads are the four requests-scoped settings endpoints only. They go through
 * the app query client so a refresh reuses the same cache entry, and a read
 * generation keeps a superseded response from committing over a newer load or a
 * newer edit.
 */

import type { QueryClient } from '@tanstack/react-query'
import { create } from 'zustand'
import type {
  MeResponse,
  ModelRouteWhitelistResponse,
  RequestContentLoggingMode,
  RequestContentLoggingResponse,
  UsageRetentionSettings,
} from '@/generated/admin-api'
import {
  getModelRouteWhitelist,
  getRequestContentLogging,
  getStreamDeltaBatching,
  getUsageRetention,
  setModelRouteWhitelist,
  setRequestContentLogging,
  setStreamDeltaBatching,
  setUsageRetention,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import {
  streamDeltaBatchingFormToRequest,
  streamDeltaBatchingToForm,
} from '@/admin-mappers/forms/model-route'
import type { StreamDeltaBatchingForm } from '@/models'
import { capturePayload } from '@/stores/record-action-types'
import {
  getPendingActions,
  runAsyncAction,
  subscribeToAsyncActions,
  type AsyncActionOutcome,
} from './async-action'
import {
  createModelRouteWhitelistForm,
  createRequestContentLoggingForm,
  createStreamDeltaBatchingForm,
  createUsageRetentionForm,
  mergeModelRouteWhitelistResponse,
  mergeRequestContentLoggingResponse,
  mergeStreamDeltaBatchingResponse,
  mergeUsageRetentionResponse,
  mirrorSharedRawRetention,
  settingsSnapshot,
  type ModelRouteWhitelistForm,
} from './settings-requests-model'
import {
  SETTING_FORM_ORDER,
  settingFormKey,
  settingKey,
  type SettingForm,
} from './settings-keys'

const SETTINGS_REQUESTS_NAMESPACE = ['settings-requests'] as const
const REQUEST_CONTENT_LOGGING_QUERY_KEY = [
  ...SETTINGS_REQUESTS_NAMESPACE,
  'request-content-logging',
] as const
const USAGE_RETENTION_QUERY_KEY = [
  ...SETTINGS_REQUESTS_NAMESPACE,
  'usage-retention',
] as const
const STREAM_DELTA_BATCHING_QUERY_KEY = [
  ...SETTINGS_REQUESTS_NAMESPACE,
  'stream-delta-batching',
] as const
const MODEL_ROUTE_WHITELIST_QUERY_KEY = [
  ...SETTINGS_REQUESTS_NAMESPACE,
  'model-route-whitelist',
] as const

/** The debounce each form's autosave uses, in milliseconds. */
export const SETTINGS_AUTOSAVE_DELAYS: Readonly<Record<SettingForm, number>> = {
  requestContentLogging: 250,
  usageRetention: 250,
  streamDeltaBatching: 300,
  modelRouteWhitelist: 150,
}

type SettingsIdentity = Pick<MeResponse, 'user_id' | 'login_name' | 'is_admin'>

type FormFlags = Record<SettingForm, boolean>

function emptyFlags(): FormFlags {
  return {
    requestContentLogging: false,
    usageRetention: false,
    streamDeltaBatching: false,
    modelRouteWhitelist: false,
  }
}

export function settingsIdentityKey(identity: SettingsIdentity | null): string {
  if (!identity) return ''
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin}`
}

type SettingsRequestsState = {
  identityKey: string
  requestContentLogging: RequestContentLoggingResponse
  usageRetention: UsageRetentionSettings
  streamDeltaBatching: StreamDeltaBatchingForm
  modelRouteWhitelist: ModelRouteWhitelistForm
  loaded: boolean
  loading: boolean
  loadError: boolean
  dirty: FormFlags
  failed: FormFlags
  initializeIdentity: (identity: SettingsIdentity | null) => void
  resetIdentity: (identity: SettingsIdentity | null) => void
  ensureLoaded: (queryClient: QueryClient) => Promise<void>
  refresh: (queryClient: QueryClient) => Promise<void>
  setRequestContentLoggingMode: (mode: RequestContentLoggingMode) => void
  setRawRetentionDays: (days: number) => void
  setApprovalRetentionDays: (days: number) => void
  setStreamDeltaBatching: (patch: Partial<StreamDeltaBatchingForm>) => void
  setModelRouteWhitelistEnabled: (enabled: boolean) => void
  saveRequestContentLogging: () => Promise<AsyncActionOutcome<void>>
  saveUsageRetention: () => Promise<AsyncActionOutcome<void>>
  saveStreamDeltaBatching: () => Promise<
    AsyncActionOutcome<StreamDeltaBatchingForm>
  >
  saveModelRouteWhitelist: () => Promise<
    AsyncActionOutcome<ModelRouteWhitelistResponse>
  >
  retryFailedSection: (form: SettingForm) => void
}

type AutosaveState = {
  timer: ReturnType<typeof setTimeout> | null
  saving: boolean
  inFlightSnapshot: string
  queued: boolean
  waitingForKey: boolean
  savedSnapshot: string
}

const autosaveStates = new Map<SettingForm, AutosaveState>()
let autosaveReady = false
let stopWatchingKeys: (() => void) | null = null
let reportFailure: ((cause: unknown) => void) | null = null
let readGeneration = 0

function autosaveState(form: SettingForm): AutosaveState {
  const existing = autosaveStates.get(form)
  if (existing) return existing
  const created: AutosaveState = {
    timer: null,
    saving: false,
    inFlightSnapshot: '',
    queued: false,
    waitingForKey: false,
    savedSnapshot: '',
  }
  autosaveStates.set(form, created)
  return created
}

function formValue(form: SettingForm): unknown {
  const state = useSettingsRequestsStore.getState()
  switch (form) {
    case 'requestContentLogging':
      return state.requestContentLogging
    case 'usageRetention':
      return state.usageRetention
    case 'streamDeltaBatching':
      return state.streamDeltaBatching
    case 'modelRouteWhitelist':
      return state.modelRouteWhitelist
  }
}

function formSnapshot(form: SettingForm): string {
  return settingsSnapshot(formValue(form))
}

function markFlags(
  flag: 'dirty' | 'failed',
  form: SettingForm,
  value: boolean,
) {
  const state = useSettingsRequestsStore.getState()
  if (state[flag][form] === value) return
  useSettingsRequestsStore.setState({
    [flag]: { ...state[flag], [form]: value },
  })
}

/**
 * Drop a read that is still in flight once the operator edits.
 *
 * A refresh started before an edit describes the state before that edit, so
 * committing it afterwards would silently revert the newer draft. The read's
 * completion checks the generation and commits nothing.
 */
function invalidatePendingRead(): void {
  if (!useSettingsRequestsStore.getState().loading) return
  ++readGeneration
  useSettingsRequestsStore.setState({ loading: false })
}

function markDirtyFromSnapshot(form: SettingForm): void {
  markFlags(
    'dirty',
    form,
    formSnapshot(form) !== autosaveState(form).savedSnapshot,
  )
}

/**
 * Register the notifier the tab shows write failures through, so the store
 * never reaches into a React context and the tab owns the notification surface.
 */
export function setSettingsRequestsNotifier(
  notify: ((cause: unknown) => void) | null,
): void {
  reportFailure = notify
}

/**
 * Arm the autosave with the values the server last confirmed.
 *
 * Mounting the tab, and refreshing it, are reads: until this runs no flush can
 * fire, so simply loading the page is never written back as an edit.
 */
export function adoptLoadedSettings(): void {
  for (const form of SETTING_FORM_ORDER) {
    const state = autosaveState(form)
    state.savedSnapshot = formSnapshot(form)
    state.queued = false
    state.waitingForKey = false
    markFlags('dirty', form, false)
    markFlags('failed', form, false)
  }
  autosaveReady = true
  watchKeyReleases()
}

/**
 * Re-arm the autosave after the tab remounts.
 *
 * The store outlives the tab, so a draft that was still pending when the tab
 * unmounted keeps its saved snapshot here. Re-arming restores the debounce and
 * schedules only the forms that are actually dirty, so a remount writes exactly
 * the edits that are still owed.
 */
export function armSettingsRequestsAutosave(): void {
  if (autosaveStates.size === 0) return
  autosaveReady = true
  watchKeyReleases()
  for (const form of SETTING_FORM_ORDER) {
    markDirtyFromSnapshot(form)
    if (useSettingsRequestsStore.getState().dirty[form]) {
      scheduleAutosave(form)
    }
  }
}

function watchKeyReleases(): void {
  if (stopWatchingKeys) return
  stopWatchingKeys = subscribeToAsyncActions(() => {
    if (!autosaveReady) return
    const pending = getPendingActions()
    for (const form of SETTING_FORM_ORDER) {
      const state = autosaveStates.get(form)
      if (!state?.waitingForKey) continue
      if (Object.hasOwn(pending, settingFormKey(form))) continue
      state.waitingForKey = false
      if (state.saving) continue
      state.queued = false
      void flushAutosave(form)
    }
  })
}

/**
 * Stop the debounce and the key-release watch.
 *
 * Leaving the tab must not leave a timer behind: a flush after unmount would
 * write an edit the operator can no longer see.
 */
export function disposeSettingsRequestsAutosave(): void {
  autosaveReady = false
  for (const form of SETTING_FORM_ORDER) {
    const state = autosaveStates.get(form)
    if (!state) continue
    if (state.timer) clearTimeout(state.timer)
    state.timer = null
    state.saving = false
    state.inFlightSnapshot = ''
    state.queued = false
    state.waitingForKey = false
  }
  stopWatchingKeys?.()
  stopWatchingKeys = null
}

function scheduleAutosave(form: SettingForm): void {
  if (!autosaveReady) return
  const state = autosaveState(form)
  markDirtyFromSnapshot(form)
  if (state.timer) clearTimeout(state.timer)
  state.timer = setTimeout(() => {
    state.timer = null
    void flushAutosave(form)
  }, SETTINGS_AUTOSAVE_DELAYS[form])
}

async function saveForm(
  form: SettingForm,
): Promise<AsyncActionOutcome<unknown>> {
  const store = useSettingsRequestsStore.getState()
  switch (form) {
    case 'requestContentLogging':
      return store.saveRequestContentLogging()
    case 'usageRetention':
      return store.saveUsageRetention()
    case 'streamDeltaBatching':
      return store.saveStreamDeltaBatching()
    case 'modelRouteWhitelist':
      return store.saveModelRouteWhitelist()
  }
}

async function flushAutosave(form: SettingForm): Promise<void> {
  if (!autosaveReady) return
  const state = autosaveState(form)
  const submitted = formSnapshot(form)
  if (state.saving) {
    if (submitted !== state.inFlightSnapshot) state.queued = true
    return
  }
  if (submitted === state.savedSnapshot) {
    markFlags('dirty', form, false)
    return
  }

  state.saving = true
  state.inFlightSnapshot = submitted
  try {
    const outcome = await saveForm(form)
    if (outcome.executed) {
      state.savedSnapshot =
        outcome.value === undefined
          ? submitted
          : settingsSnapshot(outcome.value)
      markFlags('failed', form, false)
      markDirtyFromSnapshot(form)
    } else {
      // Another form owns this shared resource key. Stay dirty and wait for the
      // owner to release it instead of retrying in a loop.
      state.queued = true
      state.waitingForKey = true
    }
  } catch (cause) {
    markFlags('failed', form, true)
    markFlags('dirty', form, true)
    reportFailure?.(cause)
  } finally {
    state.saving = false
    state.inFlightSnapshot = ''
    if (state.queued && !state.waitingForKey) {
      state.queued = false
      void flushAutosave(form)
    }
  }
}

async function fetchFresh<T>(
  queryClient: QueryClient,
  queryKey: readonly unknown[],
  load: () => Promise<T>,
): Promise<T> {
  await queryClient.cancelQueries({ queryKey, exact: true })
  await queryClient.invalidateQueries({
    queryKey: SETTINGS_REQUESTS_NAMESPACE,
    refetchType: 'none',
  })
  return queryClient.fetchQuery({ queryKey, queryFn: load, staleTime: 0 })
}

async function loadRequestContentLogging(): Promise<RequestContentLoggingResponse> {
  return expectData(await getRequestContentLogging<true>(withData()))
}

async function loadUsageRetention(): Promise<UsageRetentionSettings> {
  return expectData(await getUsageRetention<true>(withData()))
}

async function loadStreamDeltaBatching(): Promise<StreamDeltaBatchingForm> {
  return streamDeltaBatchingToForm(
    expectData(await getStreamDeltaBatching<true>(withData())),
  )
}

async function loadModelRouteWhitelist(): Promise<ModelRouteWhitelistForm> {
  return expectData(await getModelRouteWhitelist<true>(withData()))
}

export const useSettingsRequestsStore = create<SettingsRequestsState>(
  (set, get) => ({
    identityKey: '',
    requestContentLogging: createRequestContentLoggingForm(),
    usageRetention: createUsageRetentionForm(),
    streamDeltaBatching: createStreamDeltaBatchingForm(),
    modelRouteWhitelist: createModelRouteWhitelistForm(),
    loaded: false,
    loading: false,
    loadError: false,
    dirty: emptyFlags(),
    failed: emptyFlags(),

    initializeIdentity(identity) {
      const key = settingsIdentityKey(identity)
      if (get().identityKey === key) return
      get().resetIdentity(identity)
    },

    resetIdentity(identity) {
      ++readGeneration
      disposeSettingsRequestsAutosave()
      autosaveStates.clear()
      set({
        identityKey: settingsIdentityKey(identity),
        requestContentLogging: createRequestContentLoggingForm(),
        usageRetention: createUsageRetentionForm(),
        streamDeltaBatching: createStreamDeltaBatchingForm(),
        modelRouteWhitelist: createModelRouteWhitelistForm(),
        loaded: false,
        loading: false,
        loadError: false,
        dirty: emptyFlags(),
        failed: emptyFlags(),
      })
    },

    async ensureLoaded(queryClient) {
      const state = get()
      if (state.identityKey === '' || state.loaded || state.loading) return
      await get().refresh(queryClient)
    },

    async refresh(queryClient) {
      if (get().identityKey === '') return
      const generation = ++readGeneration
      set({ loading: true, loadError: false })
      try {
        const [
          requestContentLogging,
          usageRetention,
          streamDeltaBatching,
          modelRouteWhitelist,
        ] = await Promise.all([
          fetchFresh(
            queryClient,
            REQUEST_CONTENT_LOGGING_QUERY_KEY,
            loadRequestContentLogging,
          ),
          fetchFresh(
            queryClient,
            USAGE_RETENTION_QUERY_KEY,
            loadUsageRetention,
          ),
          fetchFresh(
            queryClient,
            STREAM_DELTA_BATCHING_QUERY_KEY,
            loadStreamDeltaBatching,
          ),
          fetchFresh(
            queryClient,
            MODEL_ROUTE_WHITELIST_QUERY_KEY,
            loadModelRouteWhitelist,
          ),
        ])
        if (generation !== readGeneration) return
        set({
          requestContentLogging,
          usageRetention,
          streamDeltaBatching,
          modelRouteWhitelist,
          loaded: true,
          loading: false,
          loadError: false,
        })
        adoptLoadedSettings()
      } catch {
        if (generation !== readGeneration) return
        set({ loading: false, loadError: true })
      }
    },

    setRequestContentLoggingMode(mode) {
      invalidatePendingRead()
      set({
        requestContentLogging: { ...get().requestContentLogging, mode },
      })
      scheduleAutosave('requestContentLogging')
    },

    setRawRetentionDays(days) {
      invalidatePendingRead()
      set({
        requestContentLogging: {
          ...get().requestContentLogging,
          raw_retention_days: days,
        },
      })
      scheduleAutosave('requestContentLogging')
    },

    setApprovalRetentionDays(days) {
      invalidatePendingRead()
      set({
        usageRetention: {
          ...get().usageRetention,
          approval_retention_days: days,
        },
      })
      scheduleAutosave('usageRetention')
    },

    setStreamDeltaBatching(patch) {
      invalidatePendingRead()
      set({
        streamDeltaBatching: { ...get().streamDeltaBatching, ...patch },
      })
      scheduleAutosave('streamDeltaBatching')
    },

    setModelRouteWhitelistEnabled(enabled) {
      invalidatePendingRead()
      set({ modelRouteWhitelist: { enabled } })
      scheduleAutosave('modelRouteWhitelist')
    },

    saveRequestContentLogging() {
      return runAsyncAction(
        settingKey('rawRetention'),
        async () => {
          const current = get()
          const submitted = capturePayload(current.requestContentLogging)
          const sharedRawRetention = current.usageRetention.raw_retention_days
          const response = expectData(
            await setRequestContentLogging<true>(withData({ body: submitted })),
          )
          const latest = get()
          set({
            requestContentLogging: mergeRequestContentLoggingResponse(
              latest.requestContentLogging,
              submitted,
              response,
            ),
            usageRetention: mirrorSharedRawRetention(
              latest.usageRetention,
              sharedRawRetention,
              response.raw_retention_days,
            ),
          })
        },
        'requestContentLogging',
      )
    },

    saveUsageRetention() {
      return runAsyncAction(
        settingKey('rawRetention'),
        async () => {
          const current = get()
          const submitted = capturePayload(current.usageRetention)
          const sharedRawRetention =
            current.requestContentLogging.raw_retention_days
          const response = expectData(
            await setUsageRetention<true>(withData({ body: submitted })),
          )
          const latest = get()
          set({
            usageRetention: mergeUsageRetentionResponse(
              latest.usageRetention,
              submitted,
              response,
            ),
            requestContentLogging: mirrorSharedRawRetention(
              latest.requestContentLogging,
              sharedRawRetention,
              response.raw_retention_days ?? sharedRawRetention,
            ),
          })
        },
        'usageRetention',
      )
    },

    saveStreamDeltaBatching() {
      return runAsyncAction(
        settingKey('streamDeltaBatching'),
        async () => {
          const submitted = capturePayload(get().streamDeltaBatching)
          const response = expectData(
            await setStreamDeltaBatching<true>(
              withData({ body: streamDeltaBatchingFormToRequest(submitted) }),
            ),
          )
          const confirmed = streamDeltaBatchingToForm(response)
          set({
            streamDeltaBatching: mergeStreamDeltaBatchingResponse(
              get().streamDeltaBatching,
              submitted,
              confirmed,
            ),
          })
          return confirmed
        },
        'streamDeltaBatching',
      )
    },

    saveModelRouteWhitelist() {
      return runAsyncAction(
        settingKey('modelRouteWhitelist'),
        async () => {
          const submitted = capturePayload(get().modelRouteWhitelist)
          const response = expectData(
            await setModelRouteWhitelist<true>(withData({ body: submitted })),
          )
          set({
            modelRouteWhitelist: mergeModelRouteWhitelistResponse(
              get().modelRouteWhitelist,
              submitted,
              response,
            ),
          })
          return response
        },
        'modelRouteWhitelist',
      )
    },

    retryFailedSection(form) {
      if (!autosaveReady || !get().failed[form]) return
      const state = autosaveState(form)
      if (state.timer) {
        clearTimeout(state.timer)
        state.timer = null
      }
      void flushAutosave(form)
    },
  }),
)
