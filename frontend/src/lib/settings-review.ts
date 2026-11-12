/**
 * The admin Review settings tab: the LLM review settings, read by TanStack
 * Query and edited through one native form.
 *
 * The stored settings include two secrets that the read echoes back verbatim,
 * so saving is refused until the read has succeeded: the empty default form
 * would otherwise clear the stored key and webhook token. The tab is saved by
 * hand, and the accepted response is merged leaf by leaf so an edit made during
 * the write survives it.
 */

import { useEffect, useRef, useState } from 'react'
import { useForm, useStore } from '@tanstack/react-form'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type { LlmReviewSettings, MeResponse } from '@/generated/admin-api'
import {
  getLlmReviewSettingOptions,
  getLlmReviewSettingQueryKey,
  setLlmReviewSettingMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { capturePayload } from '@/stores/record-action-types'
import { useLocale } from './i18n'
import { runAsyncAction, type AsyncActionOutcome } from './async-action'
import { useSessionStore } from './session'
import { useAsyncAction } from './use-async-action'
import {
  LLM_REVIEW_FIELDS,
  LLM_REVIEW_KEY,
  createLlmReviewForm,
  formFromSettings,
  llmReviewDirtyFields,
  llmReviewFormToRequest,
  llmReviewRefreshBlocked,
  mergeLlmReviewResponse,
  parseLlmReviewHeaders,
  type LlmReviewForm,
} from './settings-review-model'

type SettingsIdentity = Pick<MeResponse, 'user_id' | 'login_name' | 'is_admin'>

type SettingsReviewOptions = {
  onSaved?: () => void
}

export function reviewIdentityKey(identity: SettingsIdentity | null): string {
  if (!identity) return ''
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin}`
}

export function useSettingsReview(options: SettingsReviewOptions = {}) {
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const { t } = useLocale()
  const me = useSessionStore((state) => state.me)
  const identityKey = reviewIdentityKey(me)
  const onSavedRef = useRef(options.onSaved)
  const parsedHeadersRef = useRef<Record<string, string>>({})
  onSavedRef.current = options.onSaved

  const form = useForm({
    defaultValues: createLlmReviewForm(),
    validators: {
      onSubmit: ({ value }) => {
        const parsed = parseLlmReviewHeaders(value.webhook_headers_text)
        if (!parsed.ok) {
          return t('invalidReviewHeadersFormat')
        }
        parsedHeadersRef.current = parsed.headers
        return undefined
      },
    },
    onSubmit: async ({ value }) => {
      const outcome = await saveValues(value, parsedHeadersRef.current)
      if (outcome.executed) onSavedRef.current?.()
    },
  })
  const values = useStore(form.store, (state) => state.values)

  function applyForm(next: LlmReviewForm): void {
    for (const field of LLM_REVIEW_FIELDS) {
      form.setFieldValue(field as never, next[field] as never, {
        dontRunListeners: true,
        dontUpdateMeta: true,
        dontValidate: true,
      })
    }
  }
  const [baseline, setBaseline] = useState<LlmReviewForm>(() =>
    createLlmReviewForm(),
  )
  const [saveFailed, setSaveFailed] = useState(false)
  const loadedRef = useRef(false)
  const valuesRef = useRef(values)
  const baselineRef = useRef(baseline)
  const queryDataRef = useRef<LlmReviewSettings | undefined>(undefined)

  valuesRef.current = values
  baselineRef.current = baseline

  const query = useQuery({
    ...getLlmReviewSettingOptions(),
    enabled: identityKey !== '',
    staleTime: Number.POSITIVE_INFINITY,
  })
  const saveMutation = useMutation(setLlmReviewSettingMutation())

  const loaded = query.data !== undefined
  const loading = query.isFetching && !loaded
  const loadError = query.isError
  const dirty = llmReviewDirtyFields(values, baseline).length > 0
  const refreshBlocked = llmReviewRefreshBlocked({
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
    const mapped = formFromSettings(data)
    const pristine =
      llmReviewDirtyFields(valuesRef.current, baselineRef.current).length === 0
    if (loadedRef.current && !pristine) return
    loadedRef.current = true
    applyForm(mapped)
    setBaseline(mapped)
  }, [form, query.data])

  async function refresh(): Promise<void> {
    if (identityKey === '') return
    await queryClient.fetchQuery({
      ...getLlmReviewSettingOptions(),
      staleTime: 0,
    })
  }

  async function saveValues(
    value: LlmReviewForm,
    headers: Record<string, string>,
  ): Promise<AsyncActionOutcome<void>> {
    if (identityKey === '' || !loaded) return { executed: false }
    const submitted = capturePayload(value)
    try {
      const outcome = await runAsyncAction(
        LLM_REVIEW_KEY,
        async () => {
          const body = llmReviewFormToRequest(submitted, headers)
          const response = await saveMutation.mutateAsync({ body })
          const latest = valuesRef.current
          applyForm(mergeLlmReviewResponse(latest, submitted, response))
          setBaseline(formFromSettings(response))
          loadedRef.current = true
          queryClient.setQueryData(getLlmReviewSettingQueryKey(), response)
        },
        'settings',
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
    loaded,
    loading,
    loadError,
    dirty,
    saveFailed,
    refresh,
    refreshBlocked,
    pending: asyncAction.isPending(LLM_REVIEW_KEY),
    pendingOrigin: asyncAction.pendingOrigin(LLM_REVIEW_KEY),
  }
}
