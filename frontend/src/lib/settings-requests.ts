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
 * TanStack Query owns the four server reads; the native form owns the four
 * editable drafts. A read applies to a form only while that form still matches
 * its last accepted value, so a response that lands after an edit can never
 * revert the newer draft. The autosave and the in-memory handoff live inside
 * one session epoch: a retired epoch's snapshot is discarded, and its late
 * callbacks and responses never rearm a write under the new one.
 */

import { useEffect, useRef, useState } from 'react'
import { useForm, useStore } from '@tanstack/react-form'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type {
  MeResponse,
  ModelRouteWhitelistResponse,
  RequestContentLoggingResponse,
  UsageRetentionSettings,
} from '@/generated/admin-api'
import {
  getModelRouteWhitelistOptions,
  getModelRouteWhitelistQueryKey,
  getRequestContentLoggingOptions,
  getRequestContentLoggingQueryKey,
  getStreamDeltaBatchingOptions,
  getStreamDeltaBatchingQueryKey,
  getUsageRetentionOptions,
  getUsageRetentionQueryKey,
  setModelRouteWhitelistMutation,
  setRequestContentLoggingMutation,
  setStreamDeltaBatchingMutation,
  setUsageRetentionMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
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
} from './async-action'
import { useNotifier } from './notifier'
import { useSessionStore } from './session'
import { useAsyncAction } from './use-async-action'
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
  type SettingForm,
} from './settings-keys'

/** The debounce each form's autosave uses, in milliseconds. */
export const SETTINGS_AUTOSAVE_DELAYS: Readonly<Record<SettingForm, number>> = {
  requestContentLogging: 250,
  usageRetention: 250,
  streamDeltaBatching: 300,
  modelRouteWhitelist: 150,
}

type SettingsIdentity = Pick<MeResponse, 'user_id' | 'login_name' | 'is_admin'>

export function settingsIdentityKey(identity: SettingsIdentity | null): string {
  if (!identity) return ''
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin}`
}

/**
 * The epoch of the session that is authenticated right now.
 *
 * A mounted component keeps the epoch it was mounted under as the owner of
 * its own work, but only this live value decides whether that work still
 * belongs to the current session: a component that was unmounted by a logout
 * never re-renders, so the epoch it captured is stale by the time its
 * continuation runs.
 */
function currentSessionEpoch(): number {
  return useSessionStore.getState().epoch
}

const RETIRED_EPOCH_ERROR = 'retired_settings_epoch'

/**
 * Whether a rejection belongs to a session that is no longer the live one.
 *
 * A write or a read whose session was retired while it was in flight reports
 * this marker instead of the transport failure, so the caller can stay silent
 * where a current failure would be reported to the operator.
 */
export function isRetiredEpochError(cause: unknown): boolean {
  return cause instanceof Error && cause.message === RETIRED_EPOCH_ERROR
}

export type SettingsRequestsValues = {
  requestContentLogging: RequestContentLoggingResponse
  usageRetention: UsageRetentionSettings
  streamDeltaBatching: StreamDeltaBatchingForm
  modelRouteWhitelist: ModelRouteWhitelistForm
}

type FormFlags = Record<SettingForm, boolean>
type Snapshots = Record<SettingForm, string>

type RetainedRequestsDraft = {
  identityKey: string
  epoch: number
  values: SettingsRequestsValues
  saved: Snapshots
  failed: FormFlags
  loaded: boolean
}

let retainedDraft: RetainedRequestsDraft | null = null

function createValues(): SettingsRequestsValues {
  return {
    requestContentLogging: createRequestContentLoggingForm(),
    usageRetention: createUsageRetentionForm(),
    streamDeltaBatching: createStreamDeltaBatchingForm(),
    modelRouteWhitelist: createModelRouteWhitelistForm(),
  }
}

function cloneValues(values: SettingsRequestsValues): SettingsRequestsValues {
  return capturePayload(values)
}

function snapshotsOf(values: SettingsRequestsValues): Snapshots {
  return {
    requestContentLogging: settingsSnapshot(values.requestContentLogging),
    usageRetention: settingsSnapshot(values.usageRetention),
    streamDeltaBatching: settingsSnapshot(values.streamDeltaBatching),
    modelRouteWhitelist: settingsSnapshot(values.modelRouteWhitelist),
  }
}

function emptyFlags(): FormFlags {
  return {
    requestContentLogging: false,
    usageRetention: false,
    streamDeltaBatching: false,
    modelRouteWhitelist: false,
  }
}

function readRetainedDraft(
  identityKey: string,
  epoch: number,
): RetainedRequestsDraft | null {
  if (
    !retainedDraft ||
    retainedDraft.identityKey !== identityKey ||
    retainedDraft.epoch !== epoch
  ) {
    // A retired session's snapshot never comes back: same principal or not.
    retainedDraft = null
    return null
  }
  return retainedDraft
}

export function useSettingsRequests() {
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const { notifyApiError } = useNotifier()
  const me = useSessionStore((state) => state.me)
  const epoch = useSessionStore((state) => state.epoch)
  const identityKey = settingsIdentityKey(me)

  const initialRef = useRef<RetainedRequestsDraft | null | undefined>(undefined)
  if (initialRef.current === undefined) {
    initialRef.current = readRetainedDraft(identityKey, currentSessionEpoch())
  }
  const initial = initialRef.current ?? {
    identityKey,
    epoch,
    values: createValues(),
    saved: snapshotsOf(createValues()),
    failed: emptyFlags(),
    loaded: false,
  }

  /**
   * The epoch this form was mounted under.
   *
   * It identifies the owner of every write this form makes. Whether that
   * owner is still in charge is decided by the live session epoch, never by
   * this captured value.
   */
  const mountEpochRef = useRef(epoch)

  const form = useForm({
    defaultValues: cloneValues(initial.values),
  })
  const values = useStore(form.store, (state) => state.values)
  const [saved, setSaved] = useState<Snapshots>(initial.saved)
  const [failed, setFailed] = useState<FormFlags>(initial.failed)
  const [ready, setReady] = useState(false)
  const [refreshing, setRefreshing] = useState(false)
  const [refreshError, setRefreshError] = useState<unknown>(null)

  const valuesRef = useRef(values)
  const savedRef = useRef(saved)
  const failedRef = useRef(failed)
  const loadedRef = useRef(initial.loaded)
  const readyRef = useRef(false)
  const retainedRef = useRef<RetainedRequestsDraft>(initial)
  const suppressScheduleRef = useRef<Partial<Record<SettingForm, boolean>>>({})
  const timersRef = useRef<
    Partial<Record<SettingForm, ReturnType<typeof setTimeout> | null>>
  >({})
  const savingRef = useRef<FormFlags>(emptyFlags())
  const inFlightRef = useRef<Snapshots>(snapshotsOf(createValues()))
  const queuedRef = useRef<FormFlags>(emptyFlags())
  const waitingRef = useRef<FormFlags>(emptyFlags())
  const flushRef = useRef<(form: SettingForm) => void>(() => {})

  valuesRef.current = values
  savedRef.current = saved
  failedRef.current = failed
  retainedRef.current = {
    identityKey,
    epoch: mountEpochRef.current,
    values: cloneValues(values),
    saved: { ...saved },
    failed: { ...failed },
    loaded: loadedRef.current,
  }

  const requestContentLoggingQuery = useQuery({
    ...getRequestContentLoggingOptions(),
    enabled: identityKey !== '',
    staleTime: Number.POSITIVE_INFINITY,
  })
  const usageRetentionQuery = useQuery({
    ...getUsageRetentionOptions(),
    enabled: identityKey !== '',
    staleTime: Number.POSITIVE_INFINITY,
  })
  const streamDeltaBatchingQuery = useQuery({
    ...getStreamDeltaBatchingOptions(),
    enabled: identityKey !== '',
    staleTime: Number.POSITIVE_INFINITY,
  })
  const modelRouteWhitelistQuery = useQuery({
    ...getModelRouteWhitelistOptions(),
    enabled: identityKey !== '',
    staleTime: Number.POSITIVE_INFINITY,
  })

  const requestContentLoggingMutation = useMutation(
    setRequestContentLoggingMutation(),
  )
  const usageRetentionMutation = useMutation(setUsageRetentionMutation())
  const streamDeltaBatchingMutation = useMutation(
    setStreamDeltaBatchingMutation(),
  )
  const modelRouteWhitelistMutation = useMutation(
    setModelRouteWhitelistMutation(),
  )

  const queries = [
    requestContentLoggingQuery,
    usageRetentionQuery,
    streamDeltaBatchingQuery,
    modelRouteWhitelistQuery,
  ]
  const loaded =
    queries.every((query) => query.data !== undefined) || initial.loaded
  const loading = queries.some((query) => query.isFetching) && !loaded
  const loadError = queries.some((query) => query.isError)
  const dirty: FormFlags = {
    requestContentLogging:
      settingsSnapshot(values.requestContentLogging) !==
      saved.requestContentLogging,
    usageRetention:
      settingsSnapshot(values.usageRetention) !== saved.usageRetention,
    streamDeltaBatching:
      settingsSnapshot(values.streamDeltaBatching) !==
      saved.streamDeltaBatching,
    modelRouteWhitelist:
      settingsSnapshot(values.modelRouteWhitelist) !==
      saved.modelRouteWhitelist,
  }

  function setFormValue(formKey: SettingForm, value: unknown): void {
    suppressScheduleRef.current[formKey] = true
    form.setFieldValue(formKey as never, value as never, {
      dontRunListeners: true,
      dontUpdateMeta: true,
      dontValidate: true,
    })
  }

  function setSavedValue(formKey: SettingForm, value: unknown): void {
    const next = { ...savedRef.current, [formKey]: settingsSnapshot(value) }
    savedRef.current = next
    setSaved(next)
  }

  function applyRead(
    formKey: SettingForm,
    value: SettingsRequestsValues[SettingForm],
  ): void {
    const pristine =
      settingsSnapshot(valuesRef.current[formKey]) === savedRef.current[formKey]
    if (loadedRef.current && !pristine) return
    loadedRef.current = true
    setSavedValue(formKey, value)
    setFormValue(formKey, value)
  }

  useEffect(() => {
    const data = requestContentLoggingQuery.data as
      RequestContentLoggingResponse | undefined
    if (data) applyRead('requestContentLogging', data)
  }, [requestContentLoggingQuery.data])

  useEffect(() => {
    const data = usageRetentionQuery.data as UsageRetentionSettings | undefined
    if (data) applyRead('usageRetention', data)
  }, [usageRetentionQuery.data])

  useEffect(() => {
    const data = streamDeltaBatchingQuery.data
    if (data) {
      applyRead('streamDeltaBatching', streamDeltaBatchingToForm(data))
    }
  }, [streamDeltaBatchingQuery.data])

  useEffect(() => {
    const data = modelRouteWhitelistQuery.data
    if (data) applyRead('modelRouteWhitelist', { enabled: data.enabled })
  }, [modelRouteWhitelistQuery.data])

  useEffect(() => {
    if (readyRef.current || !loaded) return
    loadedRef.current = true
    readyRef.current = true
    setReady(true)
  }, [loaded])

  function clearTimer(formKey: SettingForm): void {
    const timer = timersRef.current[formKey]
    if (timer) clearTimeout(timer)
    timersRef.current[formKey] = null
  }

  function schedule(formKey: SettingForm): void {
    if (!readyRef.current) return
    clearTimer(formKey)
    timersRef.current[formKey] = setTimeout(() => {
      timersRef.current[formKey] = null
      flushRef.current(formKey)
    }, SETTINGS_AUTOSAVE_DELAYS[formKey])
  }

  async function dispatchMutation(
    formKey: SettingForm,
    submitted: SettingsRequestsValues[SettingForm],
  ): Promise<unknown> {
    // The write belongs to the epoch this form was mounted under; a session
    // that is no longer live must not send it.
    if (currentSessionEpoch() !== mountEpochRef.current) {
      throw new Error(RETIRED_EPOCH_ERROR)
    }
    switch (formKey) {
      case 'requestContentLogging':
        return requestContentLoggingMutation.mutateAsync({
          body: submitted as RequestContentLoggingResponse,
        })
      case 'usageRetention':
        return usageRetentionMutation.mutateAsync({
          body: submitted as UsageRetentionSettings,
        })
      case 'streamDeltaBatching':
        return streamDeltaBatchingMutation.mutateAsync({
          body: streamDeltaBatchingFormToRequest(
            submitted as StreamDeltaBatchingForm,
          ),
        })
      case 'modelRouteWhitelist':
        return modelRouteWhitelistMutation.mutateAsync({
          body: submitted as ModelRouteWhitelistForm,
        })
    }
  }

  function applySaveResponse(
    formKey: SettingForm,
    submitted: SettingsRequestsValues[SettingForm],
    response: unknown,
    sharedRawRetention: number | undefined,
  ): void {
    const latest = valuesRef.current
    switch (formKey) {
      case 'requestContentLogging': {
        const accepted = response as RequestContentLoggingResponse
        setFormValue(
          'requestContentLogging',
          mergeRequestContentLoggingResponse(
            latest.requestContentLogging,
            submitted as RequestContentLoggingResponse,
            accepted,
          ),
        )
        setFormValue(
          'usageRetention',
          mirrorSharedRawRetention(
            latest.usageRetention,
            sharedRawRetention,
            accepted.raw_retention_days,
          ),
        )
        setSavedValue('requestContentLogging', submitted)
        queryClient.setQueryData(getRequestContentLoggingQueryKey(), accepted)
        break
      }
      case 'usageRetention': {
        const accepted = response as UsageRetentionSettings
        setFormValue(
          'usageRetention',
          mergeUsageRetentionResponse(
            latest.usageRetention,
            submitted as UsageRetentionSettings,
            accepted,
          ),
        )
        setFormValue(
          'requestContentLogging',
          mirrorSharedRawRetention(
            latest.requestContentLogging,
            sharedRawRetention,
            accepted.raw_retention_days ?? sharedRawRetention,
          ),
        )
        setSavedValue('usageRetention', submitted)
        queryClient.setQueryData(getUsageRetentionQueryKey(), accepted)
        break
      }
      case 'streamDeltaBatching': {
        const confirmed = streamDeltaBatchingToForm(
          response as Parameters<typeof streamDeltaBatchingToForm>[0],
        )
        setFormValue(
          'streamDeltaBatching',
          mergeStreamDeltaBatchingResponse(
            latest.streamDeltaBatching,
            submitted as StreamDeltaBatchingForm,
            confirmed,
          ),
        )
        setSavedValue('streamDeltaBatching', confirmed)
        queryClient.setQueryData(getStreamDeltaBatchingQueryKey(), response)
        break
      }
      case 'modelRouteWhitelist': {
        const accepted = response as ModelRouteWhitelistResponse
        setFormValue(
          'modelRouteWhitelist',
          mergeModelRouteWhitelistResponse(
            latest.modelRouteWhitelist,
            submitted as ModelRouteWhitelistForm,
            accepted,
          ),
        )
        setSavedValue('modelRouteWhitelist', { enabled: accepted.enabled })
        queryClient.setQueryData(getModelRouteWhitelistQueryKey(), accepted)
        break
      }
    }
  }

  function markFailed(formKey: SettingForm, value: boolean): void {
    if (failedRef.current[formKey] === value) return
    const next = { ...failedRef.current, [formKey]: value }
    failedRef.current = next
    setFailed(next)
  }

  /**
   * Whether the session that owns this form is still the live one.
   *
   * The epoch this form was mounted under identifies the owner; the store's
   * current epoch decides whether that owner is still in charge. A component
   * that a logout unmounted never re-renders, so its own epoch value stays
   * the old one and cannot answer this question.
   */
  function isRetired(): boolean {
    return currentSessionEpoch() !== mountEpochRef.current
  }

  async function flush(formKey: SettingForm): Promise<void> {
    if (!readyRef.current || isRetired()) return
    const submitted = capturePayload(valuesRef.current[formKey])
    const snapshot = settingsSnapshot(submitted)
    const stateSaving = savingRef.current[formKey]
    if (stateSaving) {
      if (snapshot !== inFlightRef.current[formKey]) {
        queuedRef.current[formKey] = true
      }
      return
    }
    if (snapshot === savedRef.current[formKey]) return

    const sharedRawRetention =
      valuesRef.current.usageRetention.raw_retention_days
    savingRef.current[formKey] = true
    inFlightRef.current[formKey] = snapshot
    try {
      const outcome = await runAsyncAction(
        settingFormKey(formKey),
        async () => {
          const response = await dispatchMutation(formKey, submitted)
          if (isRetired()) return
          applySaveResponse(formKey, submitted, response, sharedRawRetention)
        },
        formKey,
      )
      if (isRetired()) return
      if (outcome.executed) {
        markFailed(formKey, false)
      } else {
        queuedRef.current[formKey] = true
        waitingRef.current[formKey] = true
      }
    } catch (cause) {
      // A late autosave callback from a retired epoch must neither notify nor
      // rearm; the snapshot it captured can never be written now.
      if (isRetired()) return
      markFailed(formKey, true)
      notifyApiError(cause)
    } finally {
      savingRef.current[formKey] = false
      inFlightRef.current[formKey] = ''
      if (isRetired()) return
      if (queuedRef.current[formKey] && !waitingRef.current[formKey]) {
        queuedRef.current[formKey] = false
        void flush(formKey)
      }
    }
  }

  flushRef.current = (formKey) => {
    void flush(formKey)
  }

  useEffect(() => {
    if (!ready) return
    if (currentSessionEpoch() !== mountEpochRef.current) return
    for (const formKey of SETTING_FORM_ORDER) {
      if (suppressScheduleRef.current[formKey]) {
        suppressScheduleRef.current[formKey] = false
        continue
      }
      const snapshot = settingsSnapshot(values[formKey])
      if (snapshot === savedRef.current[formKey]) {
        clearTimer(formKey)
        continue
      }
      if (savingRef.current[formKey]) {
        if (snapshot !== inFlightRef.current[formKey]) {
          queuedRef.current[formKey] = true
        }
        continue
      }
      schedule(formKey)
    }
  }, [epoch, ready, values])

  useEffect(() => {
    return subscribeToAsyncActions(() => {
      if (!readyRef.current) return
      const pending = getPendingActions()
      for (const formKey of SETTING_FORM_ORDER) {
        if (!waitingRef.current[formKey]) continue
        if (Object.hasOwn(pending, settingFormKey(formKey))) continue
        waitingRef.current[formKey] = false
        if (savingRef.current[formKey]) continue
        queuedRef.current[formKey] = false
        flushRef.current(formKey)
      }
    })
  }, [])

  useEffect(() => {
    const liveEpoch = currentSessionEpoch()
    if (identityKey === '' || liveEpoch !== mountEpochRef.current) {
      // A retired epoch's snapshot is discarded, never rewritten.
      if (retainedDraft && retainedDraft.epoch !== liveEpoch)
        retainedDraft = null
      return
    }
    retainedDraft = {
      identityKey,
      epoch: liveEpoch,
      values: cloneValues(values),
      saved: { ...saved },
      failed: { ...failed },
      loaded: loadedRef.current,
    }
  }, [failed, identityKey, saved, values])

  useEffect(() => {
    return () => {
      for (const formKey of SETTING_FORM_ORDER) clearTimer(formKey)
      const latest = retainedRef.current
      const liveEpoch = currentSessionEpoch()
      retainedDraft =
        latest.identityKey === '' || latest.epoch !== liveEpoch
          ? null
          : {
              identityKey: latest.identityKey,
              epoch: liveEpoch,
              values: cloneValues(latest.values),
              saved: { ...latest.saved },
              failed: { ...latest.failed },
              loaded: latest.loaded,
            }
    }
  }, [])

  /**
   * The manual refresh of the four settings reads.
   *
   * The group stays pending until every parallel read settled, so a failure
   * never releases the indicator while another read still runs. A failure of
   * the live session rejects with its cause and keeps the loaded values and
   * the drafts; a refresh whose session was retired in flight rejects with the
   * retired marker and changes nothing of the session that is logged in now.
   */
  async function refresh(): Promise<void> {
    if (identityKey === '') return
    if (currentSessionEpoch() !== mountEpochRef.current) {
      throw new Error(RETIRED_EPOCH_ERROR)
    }
    setRefreshing(true)
    setRefreshError(null)
    let failure: { cause: unknown } | null = null
    try {
      const settled = await Promise.allSettled([
        queryClient.fetchQuery({
          ...getRequestContentLoggingOptions(),
          staleTime: 0,
        }),
        queryClient.fetchQuery({
          ...getUsageRetentionOptions(),
          staleTime: 0,
        }),
        queryClient.fetchQuery({
          ...getStreamDeltaBatchingOptions(),
          staleTime: 0,
        }),
        queryClient.fetchQuery({
          ...getModelRouteWhitelistOptions(),
          staleTime: 0,
        }),
      ])
      for (const outcome of settled) {
        if (outcome.status === 'rejected') {
          failure = { cause: outcome.reason }
          break
        }
      }
    } finally {
      // The indicator ends with the group, not with its first failure. The
      // block never returns, so it cannot swallow a rejection.
      if (currentSessionEpoch() === mountEpochRef.current) setRefreshing(false)
    }
    if (currentSessionEpoch() !== mountEpochRef.current) {
      throw new Error(RETIRED_EPOCH_ERROR)
    }
    if (failure) {
      setRefreshError(failure.cause)
      throw failure.cause
    }
  }

  function retryFailedSection(formKey: SettingForm): void {
    if (!readyRef.current || !failedRef.current[formKey]) return
    if (currentSessionEpoch() !== mountEpochRef.current) return
    clearTimer(formKey)
    void flush(formKey)
  }

  const refreshBlocked =
    !loaded ||
    refreshing ||
    currentSessionEpoch() !== mountEpochRef.current ||
    queries.some((query) => query.isFetching) ||
    SETTING_FORM_ORDER.some(
      (formKey) =>
        dirty[formKey] ||
        failed[formKey] ||
        asyncAction.isPending(settingFormKey(formKey)),
    )

  return {
    form,
    values,
    loaded,
    loading,
    loadError,
    dirty,
    failed,
    ready,
    refreshing,
    refreshError,
    refresh,
    refreshBlocked,
    retryFailedSection,
    pendingOrigin: asyncAction.pendingOrigin,
    isPending: asyncAction.isPending,
  }
}
