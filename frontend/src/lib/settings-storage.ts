/**
 * The admin Storage settings tab: the raw object store, read by TanStack Query
 * and edited through one native form.
 *
 * The two S3 credentials are write-only: the server echoes only their presence,
 * so the form keeps the operator's drafts apart from the flags and can never
 * display a stored secret. The whole setting is unavailable on a backend that
 * cannot keep it; that structured answer becomes an explicit query state rather
 * than a message match. The tab is saved by hand, and a successful save clears
 * only the secret drafts it consumed.
 */

import { useEffect, useRef, useState } from 'react'
import { useForm, useStore } from '@tanstack/react-form'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type {
  MeResponse,
  RawObjectStoreSettingsResponse,
} from '@/generated/admin-api'
import {
  getRawObjectStoreOptions,
  getRawObjectStoreQueryKey,
  setRawObjectStoreMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { capturePayload } from '@/stores/record-action-types'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import { useLocale } from './i18n'
import { useSessionStore } from './session'
import { useAsyncAction } from './use-async-action'
import { formatEnvelopeCause } from './settings-refresh'
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
  storageRefreshBlocked,
  type RawObjectStoreForm,
  type RawObjectStorePresence,
} from './settings-storage-model'

type SettingsIdentity = Pick<MeResponse, 'user_id' | 'login_name' | 'is_admin'>

type SettingsStorageOptions = {
  onSaved?: () => void
}

export function storageIdentityKey(identity: SettingsIdentity | null): string {
  if (!identity) return ''
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin}`
}

const EMPTY_PRESENCE: RawObjectStorePresence = {
  has_s3_access_key: false,
  has_s3_secret_key: false,
}

export function useSettingsStorage(options: SettingsStorageOptions = {}) {
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const { t } = useLocale()
  const me = useSessionStore((state) => state.me)
  const identityKey = storageIdentityKey(me)
  const onSavedRef = useRef(options.onSaved)
  onSavedRef.current = options.onSaved

  const form = useForm({
    defaultValues: createRawObjectStoreForm(),
    validators: {
      onSubmit: ({ value }) =>
        storageBucketMissing(value) ? t('storageBucketRequired') : undefined,
    },
    onSubmit: async ({ value }) => {
      const outcome = await saveValues(value)
      if (outcome.executed) onSavedRef.current?.()
    },
  })
  const values = useStore(form.store, (state) => state.values)

  function applyForm(next: RawObjectStoreForm): void {
    for (const field of Object.keys(next) as Array<keyof RawObjectStoreForm>) {
      form.setFieldValue(field as never, next[field] as never, {
        dontRunListeners: true,
        dontUpdateMeta: true,
        dontValidate: true,
      })
    }
  }
  const [baseline, setBaseline] = useState<RawObjectStoreForm>(() =>
    createRawObjectStoreForm(),
  )
  const [saveFailed, setSaveFailed] = useState(false)
  const loadedRef = useRef(false)
  const valuesRef = useRef(values)
  const baselineRef = useRef(baseline)
  const queryDataRef = useRef<RawObjectStoreSettingsResponse | undefined>(
    undefined,
  )

  valuesRef.current = values
  baselineRef.current = baseline

  const query = useQuery({
    ...getRawObjectStoreOptions(),
    enabled: identityKey !== '',
    staleTime: Number.POSITIVE_INFINITY,
  })
  const saveMutation = useMutation(setRawObjectStoreMutation())

  const loaded = query.data !== undefined
  const loading = query.isFetching && !loaded
  const loadError = query.isError
  const unavailable = isRawObjectStoreCapabilityUnavailable(query.error)
  const unavailableMessage = unavailable
    ? (formatEnvelopeCause(query.error) ?? '')
    : ''
  const dirty = rawObjectStoreDirty(values, baseline)
  const presence = query.data
    ? presenceFromRawObjectStore(query.data)
    : EMPTY_PRESENCE
  const refreshBlocked = storageRefreshBlocked({
    pendingActions: asyncAction.pendingActions,
    dirty,
    failed: saveFailed,
    loading: query.isFetching,
    loaded,
  })

  useEffect(() => {
    const data = query.data
    if (!data || queryDataRef.current === data) return
    queryDataRef.current = data
    const mapped = formFromRawObjectStore(data)
    const pristine = !rawObjectStoreDirty(
      valuesRef.current,
      baselineRef.current,
    )
    if (loadedRef.current && !pristine) return
    loadedRef.current = true
    applyForm(mapped)
    setBaseline(mapped)
  }, [form, query.data])

  async function refresh(): Promise<void> {
    if (identityKey === '') return
    await queryClient.fetchQuery({
      ...getRawObjectStoreOptions(),
      staleTime: 0,
    })
  }

  async function saveValues(
    value: RawObjectStoreForm,
  ): Promise<AsyncActionOutcome<void>> {
    if (identityKey === '' || !loaded || unavailable) {
      return { executed: false }
    }
    const submitted = capturePayload(value)
    try {
      const outcome = await runAsyncAction(
        STORAGE_KEY,
        async () => {
          const body = rawObjectStoreFormToRequest(submitted)
          const response = await saveMutation.mutateAsync({ body })
          const latest = valuesRef.current
          applyForm(
            clearSavedSecretDrafts(
              mergeRawObjectStoreResponse(latest, submitted, response),
              submitted,
            ),
          )
          setBaseline(formFromRawObjectStore(response))
          loadedRef.current = true
          queryClient.setQueryData(getRawObjectStoreQueryKey(), response)
        },
        'storage',
      )
      if (outcome.executed) setSaveFailed(false)
      return outcome
    } catch (cause) {
      setSaveFailed(true)
      throw cause
    }
  }

  return {
    form,
    values,
    presence,
    loaded,
    loading,
    loadError,
    unavailable,
    unavailableMessage,
    dirty,
    saveFailed,
    refresh,
    refreshBlocked,
    pending: asyncAction.isPending(STORAGE_KEY),
    pendingOrigin: asyncAction.pendingOrigin(STORAGE_KEY),
  }
}
