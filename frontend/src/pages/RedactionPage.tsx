import { useEffect, useMemo, useRef, useState } from 'react'
import { useForm, useStore } from '@tanstack/react-form'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useLocation } from 'react-router'
import { LoaderCircle, RefreshCw, Save } from 'lucide-react'
import type {
  RedactionConfigSchema,
  RedactionPreviewSchema,
  RedactionScopeSchema,
} from '@/generated/admin-api'
import {
  getRedactionSettingOptions,
  getRedactionSettingQueryKey,
  listRedactionCustomStringsOptions,
  listUserOptionsOptions,
  previewRedactionMutation,
  setRedactionSettingMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import {
  RedactionPreviewPanel,
  type RedactionPreviewForm,
} from '@/components/redaction/RedactionPreviewPanel'
import {
  RedactionRulesPanel,
  type RedactionRulesForm,
} from '@/components/redaction/RedactionRulesPanel'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { Checkbox } from '@/components/ui/checkbox'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useStoredPageSize } from '@/components/shared/TablePagination'
import { createRedactionDefaults } from '@/admin-mappers/redaction'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import {
  clearRetainedRedactionDraft,
  cloneRedactionConfig,
  customStringRuleWarnings,
  defaultRedactionPreviewForm,
  emptyRedactionDraft,
  localRedactionPage,
  normalizeRedactionConfig,
  readRetainedRedactionDraft,
  REDACTION_PAGE_SIZE_KEY,
  REDACTION_PREVIEW_KEY,
  REDACTION_RULES_KEY,
  redactionCustomStringsQuery,
  redactionIdentityKey,
  redactionPageRows,
  redactionPageTotal,
  redactionPreviewBody,
  redactionSaveBody,
  redactionTargetQuery,
  refreshRedactionCustomStrings,
  refreshRedactionSetting,
  retainRedactionDraft,
  sameRedactionConfig,
  type RedactionPreviewFormValues,
  type RedactionRetainedDraft,
} from '@/lib/redaction'
import { useAsyncAction } from '@/lib/use-async-action'
import { usePageTitle } from '@/lib/router'

export function RedactionPage() {
  const location = useLocation()
  const isPreview = location.pathname === '/redaction/preview'
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const { notifyApiError, notifySuccess } = useNotifier()
  const asyncAction = useAsyncAction()
  const me = useSessionStore((state) => state.me)
  const epoch = useSessionStore((state) => state.epoch)
  const identityKey = redactionIdentityKey(me)
  const userId = me?.user_id ?? null
  const isAdmin = me?.is_admin === true
  const identityReady = identityKey !== null
  const initialDraft = useRef(readRetainedRedactionDraft(identityKey)).current
  const [suppressInitialLoad, setSuppressInitialLoad] = useState(
    initialDraft?.loaded === true,
  )
  const [scope, setScope] = useState<RedactionScopeSchema>(
    () => initialDraft?.scope ?? (isAdmin ? 'global' : 'user'),
  )
  const [targetUserId, setTargetUserId] = useState<number | null>(() =>
    initialDraft ? initialDraft.targetUserId : isAdmin ? null : userId,
  )
  const [first, setFirst] = useState(initialDraft?.first ?? 0)
  const [rows, setRows] = useStoredPageSize(REDACTION_PAGE_SIZE_KEY, 10)
  const [search, setSearch] = useState(initialDraft?.search ?? '')
  const [dirty, setDirty] = useState(initialDraft?.customStringDirty ?? false)
  const [revealAll, setRevealAll] = useState(
    initialDraft?.customStringRevealAll ?? false,
  )
  const [validationError, setValidationError] = useState(
    initialDraft?.validationError ?? false,
  )
  const [pageFailed, setPageFailed] = useState(false)
  const [previewResult, setPreviewResult] =
    useState<RedactionPreviewSchema | null>(initialDraft?.previewResult ?? null)
  const initializedIdentity = useRef<string | null>(
    initialDraft ? identityKey : null,
  )
  const skipSettingReset = useRef<unknown>(null)
  const draftSnapshot = useRef<RedactionRetainedDraft | null>(null)
  const mountEpochRef = useRef(epoch)
  const notifiedErrors = useRef(new WeakSet<object>())
  const scopeRef = useRef(scope)
  const targetUserIdRef = useRef(targetUserId)
  const firstRef = useRef(first)
  const rowsRef = useRef(rows)
  const searchRef = useRef(search)

  scopeRef.current = scope
  targetUserIdRef.current = targetUserId
  firstRef.current = first
  rowsRef.current = rows
  searchRef.current = search

  /**
   * Whether the session that owns this page is still the live one.
   *
   * The epoch this page was mounted under identifies the owner of its work;
   * the store's current epoch decides whether that owner is still in charge.
   * A page that a logout unmounted never re-renders, so the epoch it captured
   * is stale by the time its continuation runs.
   */
  function isRetired(): boolean {
    return useSessionStore.getState().epoch !== mountEpochRef.current
  }

  const rulesForm = useForm({
    defaultValues: initialDraft
      ? cloneRedactionConfig(initialDraft.config)
      : emptyRedactionDraft(),
    onSubmit: async ({ value }) => {
      await save(value)
    },
  })
  const previewForm = useForm({
    defaultValues: initialDraft
      ? {
          input_kind: initialDraft.previewInputKind,
          text: initialDraft.previewText,
        }
      : defaultRedactionPreviewForm(),
    onSubmit: async ({ value }) => {
      await runPreview(value)
    },
  })
  const config = useStore(rulesForm.store, (state) => state.values)
  const previewValues = useStore(previewForm.store, (state) => state.values)

  useEffect(
    () => () => {
      // The unmount cleanup may run after logout, when its session is already
      // retired; the retention helper keeps only the live epoch's draft.
      if (draftSnapshot.current) {
        retainRedactionDraft(draftSnapshot.current)
      }
    },
    [],
  )

  function setRulesFormValues(next: RedactionConfigSchema): void {
    const options = {
      dontRunListeners: true,
      dontUpdateMeta: true,
      dontValidate: true,
    }
    rulesForm.setFieldValue('enabled', next.enabled, options)
    rulesForm.setFieldValue('rules', { ...next.rules }, options)
    rulesForm.setFieldValue(
      'custom_strings',
      next.custom_strings.map((rule) => ({ ...rule })),
      options,
    )
  }

  function setPreviewFormValues(next: RedactionPreviewFormValues): void {
    const options = {
      dontRunListeners: true,
      dontUpdateMeta: true,
      dontValidate: true,
    }
    previewForm.setFieldValue('input_kind', next.input_kind, options)
    previewForm.setFieldValue('text', next.text, options)
  }

  const targetQuery = redactionTargetQuery(scope, targetUserId)
  const settingQuery = useQuery({
    ...getRedactionSettingOptions({ query: targetQuery }),
    enabled: identityReady && !suppressInitialLoad,
  })
  const pageQueryParams = redactionCustomStringsQuery(
    targetQuery,
    first,
    rows,
    search,
  )
  const pageQuery = useQuery({
    ...listRedactionCustomStringsOptions({ query: pageQueryParams }),
    enabled:
      identityReady &&
      !dirty &&
      !suppressInitialLoad &&
      settingQuery.data !== undefined,
  })
  const userOptionsQuery = useQuery({
    ...listUserOptionsOptions(),
    enabled: identityReady && isAdmin,
  })
  const saveMutation = useMutation(setRedactionSettingMutation())
  const previewMutation = useMutation(previewRedactionMutation())

  usePageTitle(isPreview ? 'redactionPreview' : 'redactionRules')

  const saving = asyncAction.isPending(REDACTION_RULES_KEY)
  const previewing = asyncAction.isPending(REDACTION_PREVIEW_KEY)
  const loading = settingQuery.isFetching
  const ready =
    identityReady && (suppressInitialLoad || settingQuery.data !== undefined)
  const loadError =
    !suppressInitialLoad &&
    settingQuery.isError &&
    settingQuery.data === undefined
  const pageError = dirty ? pageFailed : pageFailed || pageQuery.isError

  draftSnapshot.current =
    identityKey === null || isRetired()
      ? null
      : {
          config: cloneRedactionConfig(config),
          customStringDirty: dirty,
          customStringRevealAll: revealAll,
          first,
          identityKey,
          epoch: mountEpochRef.current,
          loaded: ready,
          previewInputKind: previewValues.input_kind,
          previewResult,
          previewText: previewValues.text,
          scope,
          search,
          targetUserId,
          validationError,
        }

  const rowsData = useMemo(
    () =>
      redactionPageRows({ config, dirty, first, rows, search }, pageQuery.data),
    [config, dirty, first, pageQuery.data, rows, search],
  )
  const total = useMemo(
    () => redactionPageTotal({ config, dirty, search }, pageQuery.data),
    [config, dirty, pageQuery.data, search],
  )

  function reportApiError(cause: unknown): void {
    if (cause && typeof cause === 'object') {
      if (notifiedErrors.current.has(cause)) return
      notifiedErrors.current.add(cause)
    }
    notifyApiError(cause)
  }

  useEffect(() => {
    if (identityKey === null) {
      initializedIdentity.current = null
      clearRetainedRedactionDraft()
      return
    }
    if (initializedIdentity.current === identityKey) return
    initializedIdentity.current = identityKey
    setScope(isAdmin ? 'global' : 'user')
    setTargetUserId(isAdmin ? null : userId)
    setFirst(0)
    setSearch('')
    setDirty(false)
    setRevealAll(false)
    setValidationError(false)
    setPageFailed(false)
    setPreviewResult(null)
    setSuppressInitialLoad(false)
    setRulesFormValues(emptyRedactionDraft())
    setPreviewFormValues(defaultRedactionPreviewForm())
  }, [epoch, identityKey, isAdmin, userId])

  useEffect(() => {
    if (suppressInitialLoad) return
    if (!settingQuery.data) return
    if (skipSettingReset.current === settingQuery.data) {
      skipSettingReset.current = null
      return
    }
    setRulesFormValues(
      normalizeRedactionConfig(
        createRedactionDefaults(settingQuery.data.config),
      ),
    )
    setDirty(false)
    setRevealAll(false)
    setValidationError(false)
    setPageFailed(false)
  }, [settingQuery.data, suppressInitialLoad])

  useEffect(() => {
    if (settingQuery.error) reportApiError(settingQuery.error)
  }, [settingQuery.error])

  useEffect(() => {
    if (pageQuery.error) reportApiError(pageQuery.error)
  }, [pageQuery.error])

  useEffect(() => {
    if (pageQuery.isSuccess) setPageFailed(false)
  }, [pageQuery.isSuccess, pageQuery.dataUpdatedAt])

  useEffect(() => {
    if (userOptionsQuery.error) reportApiError(userOptionsQuery.error)
  }, [userOptionsQuery.error])

  useEffect(() => {
    if (dirty || pageQuery.isFetching) return
    const page = pageQuery.data
    if (!page) return
    if (
      page.items.length === 0 &&
      page.total > 0 &&
      firstRef.current >= page.total
    ) {
      const corrected =
        Math.floor((page.total - 1) / (page.rows || rowsRef.current)) *
        (page.rows || rowsRef.current)
      if (corrected !== firstRef.current) setFirst(corrected)
      return
    }
    if (page.rows > 0 && page.rows !== rowsRef.current) {
      setRows(page.rows)
    }
  }, [dirty, pageQuery.data, pageQuery.isFetching, setRows])

  useEffect(() => {
    if (!isAdmin || scope !== 'user') return
    const users = userOptionsQuery.data?.users
    if (!users?.length) return
    if (
      targetUserIdRef.current !== null &&
      users.some((user) => user.user_id === targetUserIdRef.current)
    ) {
      return
    }
    changeTarget('user', users[0].user_id)
  }, [isAdmin, scope, targetUserId, userOptionsQuery.data])

  function changeTarget(
    nextScope: RedactionScopeSchema,
    nextUserId: number | null,
  ): void {
    if (nextScope === 'user' && nextUserId === null) return
    if (scope === nextScope && targetUserId === nextUserId) return
    setScope(nextScope)
    setTargetUserId(nextUserId)
    setFirst(0)
    setDirty(false)
    setRevealAll(false)
    setValidationError(false)
    setPageFailed(false)
    setPreviewResult(null)
    setSuppressInitialLoad(false)
    setRulesFormValues(emptyRedactionDraft())
  }

  function changeScope(next: string): void {
    if (next !== 'global' && next !== 'user') return
    const nextUserId =
      next === 'user'
        ? (targetUserId ?? userOptionsQuery.data?.users[0]?.user_id ?? userId)
        : null
    changeTarget(next, nextUserId)
  }

  function changeTargetUser(value: string): void {
    const nextUserId = Number(value)
    if (!Number.isSafeInteger(nextUserId) || nextUserId <= 0) return
    changeTarget('user', nextUserId)
  }

  async function refresh(): Promise<void> {
    if (!identityReady) return
    if (isRetired()) return
    const target = redactionTargetQuery(
      scopeRef.current,
      targetUserIdRef.current,
    )
    try {
      const setting = await refreshRedactionSetting(queryClient, target)
      if (isRetired()) return
      setRulesFormValues(
        normalizeRedactionConfig(createRedactionDefaults(setting.config)),
      )
      setDirty(false)
      setRevealAll(false)
      setValidationError(false)
      setPageFailed(false)
      await refreshRedactionCustomStrings(
        queryClient,
        target,
        firstRef.current,
        rowsRef.current,
        searchRef.current,
      )
      if (isRetired()) return
      setPageFailed(false)
    } catch (cause) {
      if (isRetired()) return
      setPageFailed(true)
      reportApiError(cause)
    }
  }

  async function save(value: RedactionConfigSchema): Promise<void> {
    if (!ready || saving) return
    if (isRetired()) return
    const warnings = customStringRuleWarnings(value)
    if (warnings.some((warning) => warning.invalidRegex)) {
      setValidationError(true)
      return
    }
    const submitted = redactionSaveBody(value)
    const normalizedChanged =
      JSON.stringify(value.custom_strings) !==
      JSON.stringify(submitted.custom_strings)
    const target = redactionTargetQuery(scope, targetUserId)
    setValidationError(false)
    setRulesFormValues(submitted)
    setDirty((current) => current || normalizedChanged)
    try {
      const outcome = await asyncAction.run(
        REDACTION_RULES_KEY,
        async () => {
          const response = await saveMutation.mutateAsync({
            body: submitted,
            query: target,
          })
          if (isRetired()) return normalizeRedactionConfig(response.config)
          skipSettingReset.current = response
          queryClient.setQueryData(
            getRedactionSettingQueryKey({ query: target }),
            response,
          )
          const applied = sameRedactionConfig(rulesForm.state.values, submitted)
          setRevealAll(false)
          if (applied) {
            setRulesFormValues(normalizeRedactionConfig(response.config))
            setDirty(false)
            await refreshRedactionCustomStrings(
              queryClient,
              target,
              firstRef.current,
              rowsRef.current,
              searchRef.current,
            )
          }
          return normalizeRedactionConfig(response.config)
        },
        'save',
      )
      // A retired session's save must not notify the account that is logged in
      // now, nor rearm its own work.
      if (isRetired()) return
      if (outcome.executed) notifySuccess(t('saved'))
    } catch (cause) {
      if (isRetired()) return
      reportApiError(cause)
    }
  }

  async function runPreview(value: RedactionPreviewFormValues): Promise<void> {
    if (isRetired()) return
    const capturedConfig = cloneRedactionConfig(rulesForm.state.values)
    const capturedScope = scopeRef.current
    const capturedTarget = targetUserIdRef.current
    const capturedText = value.text
    const capturedKind = value.input_kind
    try {
      await asyncAction.run(
        REDACTION_PREVIEW_KEY,
        async () => {
          setPreviewResult(null)
          const response = await previewMutation.mutateAsync({
            body: redactionPreviewBody(capturedConfig, {
              input_kind: capturedKind,
              text: capturedText,
            }),
          })
          if (isRetired()) return response.preview
          if (
            scopeRef.current === capturedScope &&
            targetUserIdRef.current === capturedTarget &&
            JSON.stringify(rulesForm.state.values) ===
              JSON.stringify(capturedConfig) &&
            previewForm.state.values.text === capturedText &&
            previewForm.state.values.input_kind === capturedKind
          ) {
            setPreviewResult(response.preview)
          }
          return response.preview
        },
        'preview',
      )
    } catch (cause) {
      if (isRetired()) return
      reportApiError(cause)
    }
  }

  function changeSearch(next: string): void {
    setSearch(next)
    setFirst(0)
  }

  function changePage(nextFirst: number, nextRows: number): void {
    setFirst(nextFirst)
    setRows(nextRows)
  }

  function addRule(): void {
    const next: RedactionConfigSchema['custom_strings'] = [
      ...rulesForm.state.values.custom_strings,
      { match_type: 'contains', pattern: '', scope: 'text' },
    ]
    rulesForm.setFieldValue('custom_strings', next)
    setDirty(true)
    setValidationError(false)
    setFirst(0)
  }

  function updateRule(
    arrayIndex: number,
    patch: Partial<RedactionConfigSchema['custom_strings'][number]>,
  ): void {
    const current = rulesForm.state.values
    if (!current.custom_strings[arrayIndex]) return
    const customStrings = current.custom_strings.map((rule, index) =>
      index === arrayIndex ? { ...rule, ...patch } : rule,
    )
    rulesForm.setFieldValue('custom_strings', customStrings)
    const localized = localRedactionPage({
      config: { ...current, custom_strings: customStrings },
      first: firstRef.current,
      rows: rowsRef.current,
      search: searchRef.current,
    })
    setFirst(localized.first)
    setDirty(true)
    setValidationError(false)
  }

  return (
    <div
      className="grid min-w-0 gap-5"
      data-foundation-route-id={
        isPreview ? 'R-REDACTION-PREVIEW' : 'R-REDACTION-RULES'
      }
    >
      <HeaderActions>
        <div className="flex min-w-0 flex-wrap items-end gap-2">
          <div className="flex min-h-9 items-center gap-2">
            <span className="text-sm text-muted-foreground">
              {t('redactionRuleScope')}
            </span>
            {isAdmin ? (
              <Select
                disabled={!identityReady || loading || saving}
                onValueChange={changeScope}
                value={scope}
              >
                <SelectTrigger
                  aria-label={t('redactionRuleScope')}
                  className="w-32"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value="global">
                    {t('redactionPublicRules')}
                  </SelectItem>
                  <SelectItem value="user">
                    {t('redactionPrivateRules')}
                  </SelectItem>
                </SelectContent>
              </Select>
            ) : (
              <span aria-label={t('redactionRuleScope')} className="text-sm">
                {t('redactionMyPrivateRules')}
              </span>
            )}
          </div>

          {isAdmin && scope === 'user' && (
            <div className="flex min-h-9 items-center gap-2">
              <span className="text-sm text-muted-foreground">
                {t('redactionTargetUser')}
              </span>
              <Select
                disabled={
                  !identityReady ||
                  loading ||
                  saving ||
                  userOptionsQuery.isPending ||
                  (userOptionsQuery.data?.users.length ?? 0) === 0
                }
                onValueChange={changeTargetUser}
                value={targetUserId === null ? undefined : String(targetUserId)}
              >
                <SelectTrigger
                  aria-label={t('redactionTargetUser')}
                  className="w-52 max-w-[calc(100vw-12rem)]"
                >
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {(userOptionsQuery.data?.users ?? []).map((user) => (
                    <SelectItem key={user.user_id} value={String(user.user_id)}>
                      {`${user.display_name || user.login_name} · ${user.login_name}`}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}

          <Button
            disabled={!ready || loading || saving}
            onClick={() => void rulesForm.handleSubmit()}
            type="button"
          >
            {saving ? (
              <LoaderCircle aria-hidden="true" className="animate-spin" />
            ) : (
              <Save aria-hidden="true" />
            )}
            {t('save')}
          </Button>
          <Button
            disabled={saving}
            onClick={() => void refresh()}
            type="button"
            variant="outline"
          >
            {loading ? (
              <LoaderCircle aria-hidden="true" className="animate-spin" />
            ) : (
              <RefreshCw aria-hidden="true" />
            )}
            {t('refresh')}
          </Button>
          <label className="flex min-h-9 items-center gap-2 rounded-md border px-3 text-sm">
            <rulesForm.Field name="enabled">
              {(field) => (
                <Checkbox
                  aria-label={t('redaction')}
                  checked={field.state.value}
                  disabled={!ready}
                  onCheckedChange={(checked) =>
                    field.handleChange(checked === true)
                  }
                />
              )}
            </rulesForm.Field>
            <span>{t('redaction')}</span>
          </label>
        </div>
      </HeaderActions>

      {!ready ? (
        loadError ? (
          <div className="flex flex-wrap items-center gap-3" role="alert">
            <span>{t('failed')}</span>
            <Button
              onClick={() => void refresh()}
              type="button"
              variant="outline"
            >
              {t('refresh')}
            </Button>
          </div>
        ) : (
          <div
            aria-label={t('loading')}
            className="grid min-h-32 place-items-center"
            role="status"
          >
            <LoaderCircle aria-hidden="true" className="size-6 animate-spin" />
          </div>
        )
      ) : isPreview ? (
        <RedactionPreviewPanel
          config={config}
          form={previewForm as RedactionPreviewForm}
          previewResult={previewResult}
          previewing={previewing}
        />
      ) : (
        <RedactionRulesPanel
          customStringDirty={dirty}
          customStringFirst={first}
          customStringRevealAll={revealAll}
          customStringRows={rows}
          customStringSearch={search}
          form={rulesForm as RedactionRulesForm}
          onAddRule={addRule}
          onEditRule={() => {
            setDirty(true)
            setValidationError(false)
          }}
          onPage={changePage}
          onRemoveRule={(arrayIndex) => {
            const current = rulesForm.state.values
            const customStrings = current.custom_strings.filter(
              (_rule, index) => index !== arrayIndex,
            )
            rulesForm.setFieldValue('custom_strings', customStrings)
            const localized = localRedactionPage({
              config: { ...current, custom_strings: customStrings },
              first: firstRef.current,
              rows: rowsRef.current,
              search: searchRef.current,
            })
            setFirst(localized.first)
            setDirty(true)
            setValidationError(false)
          }}
          onSearch={changeSearch}
          onToggleReveal={() => setRevealAll((current) => !current)}
          pageError={pageError}
          rows={rowsData}
          total={total}
          validationError={validationError}
        />
      )}
    </div>
  )
}
