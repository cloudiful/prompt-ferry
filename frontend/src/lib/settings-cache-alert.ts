/**
 * The admin cache-alert settings tab: the DingTalk alert policy, read by
 * TanStack Query and edited through one native form.
 *
 * The shape follows the storage port. The DingTalk secret and the SMTP password
 * are write-only: the server echoes only their presence flags, the local drafts
 * decide keep/replace, and a successful save clears exactly the drafts it
 * consumed. The whole setting is unavailable on a backend that cannot keep it;
 * that structured answer becomes an explicit query state.
 */

import { useEffect, useRef, useState } from 'react'
import { useForm, useStore } from '@tanstack/react-form'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type {
  CacheAlertSettingsResponse,
  MeResponse,
} from '@/generated/admin-api'
import {
  getCacheAlertSettingOptions,
  getCacheAlertSettingQueryKey,
  setCacheAlertSettingMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { capturePayload } from '@/stores/record-action-types'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import { useSessionStore } from './session'
import { useAsyncAction } from './use-async-action'
import { formatEnvelopeCause } from './settings-refresh'
import {
  CACHE_ALERT_KEY,
  cacheAlertDirty,
  cacheAlertFormToRequest,
  cacheAlertRefreshBlocked,
  cacheAlertSmtpIssues,
  clearSavedCacheAlertSecret,
  createCacheAlertForm,
  formFromCacheAlertResponse,
  isCacheAlertCapabilityUnavailable,
  mergeCacheAlertResponse,
  setCacheAlertSecretDraft,
  setCacheAlertSmtpPasswordDraft,
  type CacheAlertForm,
  type CacheAlertSmtpForm,
  type CacheAlertSmtpLeaf,
} from './settings-cache-alert-model'

type SettingsIdentity = Pick<MeResponse, 'user_id' | 'login_name' | 'is_admin'>

export function cacheAlertIdentityKey(
  identity: SettingsIdentity | null,
): string {
  if (!identity) return ''
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin}`
}

type SettingsCacheAlertOptions = {
  onSaved?: () => void
}

export function useSettingsCacheAlert(options: SettingsCacheAlertOptions = {}) {
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const me = useSessionStore((state) => state.me)
  const identityKey = cacheAlertIdentityKey(me)
  const onSavedRef = useRef(options.onSaved)
  onSavedRef.current = options.onSaved

  const form = useForm({
    defaultValues: createCacheAlertForm(),
    validators: {
      onSubmit: ({ value }) =>
        cacheAlertSmtpIssues(value).length > 0 ? 'invalid_smtp' : undefined,
    },
    onSubmit: async ({ value }) => {
      const outcome = await saveValues(value)
      if (outcome.executed) onSavedRef.current?.()
    },
  })
  const values = useStore(form.store, (state) => state.values)

  function applyForm(next: CacheAlertForm): void {
    for (const field of Object.keys(next) as Array<keyof CacheAlertForm>) {
      form.setFieldValue(field as never, next[field] as never, {
        dontRunListeners: true,
        dontUpdateMeta: true,
        dontValidate: true,
      })
    }
  }
  const [baseline, setBaseline] = useState<CacheAlertForm>(() =>
    createCacheAlertForm(),
  )
  const [saveFailed, setSaveFailed] = useState(false)
  const loadedRef = useRef(false)
  const valuesRef = useRef(values)
  const baselineRef = useRef(baseline)
  const queryDataRef = useRef<CacheAlertSettingsResponse | undefined>(undefined)

  valuesRef.current = values
  baselineRef.current = baseline

  const query = useQuery({
    ...getCacheAlertSettingOptions(),
    enabled: identityKey !== '',
    staleTime: Number.POSITIVE_INFINITY,
  })
  const saveMutation = useMutation(setCacheAlertSettingMutation())

  const loaded = query.data !== undefined
  const loading = query.isFetching && !loaded
  const loadError = query.isError
  const unavailable = isCacheAlertCapabilityUnavailable(query.error)
  const unavailableMessage = unavailable
    ? (formatEnvelopeCause(query.error) ?? '')
    : ''
  const dirty = cacheAlertDirty(values, baseline)
  const refreshBlocked = cacheAlertRefreshBlocked({
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
    const mapped = formFromCacheAlertResponse(data)
    const pristine = !cacheAlertDirty(valuesRef.current, baselineRef.current)
    if (loadedRef.current && !pristine) return
    loadedRef.current = true
    applyForm(mapped)
    setBaseline(mapped)
  }, [form, query.data])

  async function refresh(): Promise<void> {
    if (identityKey === '') return
    await queryClient.fetchQuery({
      ...getCacheAlertSettingOptions(),
      staleTime: 0,
    })
  }

  async function saveValues(
    value: CacheAlertForm,
  ): Promise<AsyncActionOutcome<void>> {
    if (identityKey === '' || !loaded || unavailable) {
      return { executed: false }
    }
    const submitted = capturePayload(value)
    try {
      const outcome = await runAsyncAction(
        CACHE_ALERT_KEY,
        async () => {
          const body = cacheAlertFormToRequest(submitted)
          const response = await saveMutation.mutateAsync({ body })
          const latest = valuesRef.current
          applyForm(
            clearSavedCacheAlertSecret(
              mergeCacheAlertResponse(latest, submitted, response),
              submitted,
            ),
          )
          setBaseline(formFromCacheAlertResponse(response))
          loadedRef.current = true
          queryClient.setQueryData(getCacheAlertSettingQueryKey(), response)
        },
        'cacheAlert',
      )
      if (outcome.executed) setSaveFailed(false)
      return outcome
    } catch (cause) {
      setSaveFailed(true)
      throw cause
    }
  }

  function setField<K extends Exclude<keyof CacheAlertForm, 'smtp'>>(
    field: K,
    value: CacheAlertForm[K],
  ): void {
    form.setFieldValue(field as never, value as never)
  }

  function setSecretDraft(value: string): void {
    form.setFieldValue(
      'secret',
      setCacheAlertSecretDraft(valuesRef.current, value).secret,
    )
  }

  function setSmtpField<K extends CacheAlertSmtpLeaf>(
    field: K,
    value: CacheAlertSmtpForm[K],
  ): void {
    const smtp = { ...valuesRef.current.smtp, [field]: value }
    form.setFieldValue('smtp', smtp)
  }

  function setSmtpPasswordDraft(value: string): void {
    form.setFieldValue(
      'smtp',
      setCacheAlertSmtpPasswordDraft(valuesRef.current, value).smtp,
    )
  }

  return {
    form,
    values,
    loaded,
    loading,
    loadError,
    unavailable,
    unavailableMessage,
    dirty,
    saveFailed,
    refresh,
    refreshBlocked,
    setField,
    setSecretDraft,
    setSmtpField,
    setSmtpPasswordDraft,
    pending: asyncAction.isPending(CACHE_ALERT_KEY),
    pendingOrigin: asyncAction.pendingOrigin(CACHE_ALERT_KEY),
  }
}
