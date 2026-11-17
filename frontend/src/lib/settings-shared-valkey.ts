/**
 * The admin shared Valkey setting: a database-owned, encrypted URL plus the
 * shared ephemeral TTL.
 *
 * One save owns the `setting:sharedValkey` key, so a double click sends one
 * PATCH. The URL is write-only: an empty draft keeps the stored value and the
 * explicit clear switch sends the `clear` patch. Every save reports the
 * desired revision while this worker keeps its applied revision until restart.
 */

import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type {
  SharedValkeySecretPatch,
  SharedValkeySettingsResponse,
} from '@/generated/admin-api'
import {
  getSharedValkeyOptions,
  getSharedValkeyQueryKey,
  setSharedValkeyMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { capturePayload } from '@/stores/record-action-types'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import { settingKey } from './settings-keys'
import { useAsyncAction } from './use-async-action'

export const SHARED_VALKEY_KEY = settingKey('sharedValkey')

export type SharedValkeyForm = {
  url: string
  clear: boolean
  ttl_seconds: number
}

export function sharedValkeyFormFromResponse(
  response: SharedValkeySettingsResponse | undefined,
): SharedValkeyForm {
  return {
    url: '',
    clear: false,
    ttl_seconds: response?.ttl_seconds ?? 86400,
  }
}

/** The PATCH body: an empty draft keeps the stored URL, the switch clears it. */
export function sharedValkeyFormToPatch(
  form: SharedValkeyForm,
): SharedValkeySecretPatch | undefined {
  if (form.clear) return { mode: 'clear' }
  const url = form.url.trim()
  return url === '' ? undefined : { mode: 'replace', value: url }
}

export function useSettingsSharedValkey() {
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const query = useQuery({
    ...getSharedValkeyOptions(),
    staleTime: Number.POSITIVE_INFINITY,
  })
  const saveMutation = useMutation(setSharedValkeyMutation())
  const [form, setForm] = useState<SharedValkeyForm>(() =>
    sharedValkeyFormFromResponse(undefined),
  )

  useEffect(() => {
    if (query.data === undefined) return
    setForm(sharedValkeyFormFromResponse(query.data))
  }, [query.data])

  const loaded = query.data !== undefined
  const loading = query.isFetching && !loaded
  const loadError = query.isError

  function setField<K extends keyof SharedValkeyForm>(
    field: K,
    value: SharedValkeyForm[K],
  ): void {
    setForm((current) => ({ ...current, [field]: value }))
  }

  async function save(
    form: SharedValkeyForm,
  ): Promise<AsyncActionOutcome<SharedValkeySettingsResponse>> {
    if (!loaded) return { executed: false }
    const submitted = capturePayload(form)
    const body = {
      ttl_seconds: submitted.ttl_seconds,
      url: sharedValkeyFormToPatch(submitted),
    }
    const outcome = await runAsyncAction(
      SHARED_VALKEY_KEY,
      async () => {
        const response = await saveMutation.mutateAsync({ body })
        queryClient.setQueryData(getSharedValkeyQueryKey(), response)
        setForm(sharedValkeyFormFromResponse(response))
        return response
      },
      'settings',
    )
    return outcome
  }

  return {
    response: query.data,
    form,
    setField,
    loaded,
    loading,
    loadError,
    refresh: () => query.refetch(),
    save,
    pending: asyncAction.isPending(SHARED_VALKEY_KEY),
    pendingOrigin: asyncAction.pendingOrigin(SHARED_VALKEY_KEY),
  }
}
