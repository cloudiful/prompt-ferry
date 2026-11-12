import { useEffect, useRef, useState } from 'react'
import { useLocation } from 'react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Calculator, Download, RefreshCw } from 'lucide-react'
import type {
  BillingChargeResponse,
  BillingPriceRuleResponse,
} from '@/generated/admin-api'
import {
  billingChargeDetailOptions,
  billingSummaryOptions,
  createBillingPriceRuleMutation,
  deleteBillingPriceRuleMutation,
  listBillingChargesOptions,
  listBillingPriceRulesOptions,
  listEndpointsOptions,
  listUserOptionsOptions,
  patchBillingPriceRuleMutation,
  repriceBillingMutation,
  updateBillingPriceRuleMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { fetchBillingCsv } from '@/api/billing'
import { BillingChargeDetailDialog } from '@/components/billing/BillingChargeDetailDialog'
import { BillingCharges } from '@/components/billing/BillingCharges'
import { BillingFilters } from '@/components/billing/BillingFilters'
import { BillingPriceRuleDialog } from '@/components/billing/BillingPriceRuleDialog'
import { BillingPriceRules } from '@/components/billing/BillingPriceRules'
import { BillingSummary } from '@/components/billing/BillingSummary'
import { useStoredPageSize } from '@/components/shared/TablePagination'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { usePageTitle } from '@/lib/router'
import { useSessionStore } from '@/lib/session'
import { useAsyncAction } from '@/lib/use-async-action'
import {
  BILLING_REPRICE_KEY,
  billingEndpointsSelect,
  billingIdentityKey,
  billingPriceRuleActionKey,
  billingUserOptionsSelect,
  refreshBillingCharges,
  refreshBillingPriceRules,
  refreshBillingSummary,
} from '@/lib/billing'
import {
  BILLING_CHARGE_PAGE_SIZE_FALLBACK,
  BILLING_PAGE_SIZE_OPTIONS,
  BILLING_PRICE_RULE_PAGE_SIZE_FALLBACK,
  BILLING_PRICE_RULE_PAGE_SIZE_OPTIONS,
  billingFilterQuery,
  billingPriceRuleRequest,
  currentMonthDate,
  endOfDate,
  newBillingPriceRuleForm,
  startOfDate,
  type BillingChargeFilters,
  type BillingPriceRuleForm,
} from '@/lib/billing-model'
import type { TablePageChange } from '@/models/pagination'

type PriceRuleDialogState = {
  generation: number
  initialForm: BillingPriceRuleForm
}

export function BillingPage() {
  const { t } = useLocale()
  const location = useLocation()
  const queryClient = useQueryClient()
  const { notifyApiError, notifySuccess } = useNotifier()
  const asyncAction = useAsyncAction()
  const me = useSessionStore((state) => state.me)
  const isAdmin = me?.is_admin === true
  const identity = {
    user_id: me?.user_id ?? null,
    is_admin: me?.is_admin === true,
  }
  const identityKey = billingIdentityKey(identity)
  const initializedFor = useRef<string | null>(null)
  const notifiedErrors = useRef(new WeakSet<object>())
  const [draftFilters, setDraftFilters] = useState<BillingChargeFilters>({})
  const [appliedFilters, setAppliedFilters] = useState<BillingChargeFilters>({})
  const [periodStart, setPeriodStart] = useState(currentMonthDate)
  const [periodEnd, setPeriodEnd] = useState('')
  const [chargesFirst, setChargesFirst] = useState(0)
  const [chargeRows, setChargeRows] = useStoredPageSize(
    'billing',
    BILLING_CHARGE_PAGE_SIZE_FALLBACK,
    BILLING_PAGE_SIZE_OPTIONS,
  )
  const [priceRuleFirst, setPriceRuleFirst] = useState(0)
  const [priceRuleRows, setPriceRuleRows] = useStoredPageSize(
    'billing-price-rules',
    BILLING_PRICE_RULE_PAGE_SIZE_FALLBACK,
    BILLING_PRICE_RULE_PAGE_SIZE_OPTIONS,
  )
  const [detailChargeId, setDetailChargeId] = useState<number | null>(null)
  const [priceRuleDialog, setPriceRuleDialog] =
    useState<PriceRuleDialogState | null>(null)
  const chargesFirstRef = useRef(chargesFirst)
  const chargeRowsRef = useRef(chargeRows)
  const priceRuleFirstRef = useRef(priceRuleFirst)
  const priceRuleRowsRef = useRef(priceRuleRows)
  const appliedFiltersRef = useRef(appliedFilters)

  chargesFirstRef.current = chargesFirst
  chargeRowsRef.current = chargeRows
  priceRuleFirstRef.current = priceRuleFirst
  priceRuleRowsRef.current = priceRuleRows
  appliedFiltersRef.current = appliedFilters

  const summaryQuery = useQuery({
    ...billingSummaryOptions({ query: billingFilterQuery(appliedFilters) }),
    enabled: me !== null,
  })
  const chargesQuery = useQuery({
    ...listBillingChargesOptions({
      query: {
        ...billingFilterQuery(appliedFilters),
        first: chargesFirst,
        rows: chargeRows,
      },
    }),
    enabled: me !== null,
  })
  const priceRulesQuery = useQuery({
    ...listBillingPriceRulesOptions({
      query: { first: priceRuleFirst, rows: priceRuleRows },
    }),
    enabled: me !== null && isAdmin,
  })
  const endpointsQuery = useQuery({
    ...listEndpointsOptions({ query: { first: 0, rows: 500 } }),
    select: billingEndpointsSelect,
    enabled: me !== null && isAdmin,
  })
  const userOptionsQuery = useQuery({
    ...listUserOptionsOptions(),
    select: billingUserOptionsSelect,
    enabled: me !== null && isAdmin,
  })
  const detailQuery = useQuery({
    ...billingChargeDetailOptions({
      path: { charge_id: detailChargeId ?? 0 },
    }),
    enabled: detailChargeId !== null,
  })

  const createPriceRule = useMutation(createBillingPriceRuleMutation())
  const updatePriceRule = useMutation(updateBillingPriceRuleMutation())
  const removePriceRule = useMutation(deleteBillingPriceRuleMutation())
  const patchPriceRule = useMutation(patchBillingPriceRuleMutation())
  const repriceBilling = useMutation(repriceBillingMutation())

  usePageTitle('billing')

  function reportApiError(cause: unknown): void {
    if (cause && typeof cause === 'object') {
      if (notifiedErrors.current.has(cause)) return
      notifiedErrors.current.add(cause)
    }
    notifyApiError(cause)
  }

  useEffect(() => {
    if (summaryQuery.error) reportApiError(summaryQuery.error)
  }, [summaryQuery.error])
  useEffect(() => {
    if (chargesQuery.error) reportApiError(chargesQuery.error)
  }, [chargesQuery.error])
  useEffect(() => {
    if (priceRulesQuery.error) reportApiError(priceRulesQuery.error)
  }, [priceRulesQuery.error])
  useEffect(() => {
    if (endpointsQuery.error) reportApiError(endpointsQuery.error)
  }, [endpointsQuery.error])
  useEffect(() => {
    if (userOptionsQuery.error) reportApiError(userOptionsQuery.error)
  }, [userOptionsQuery.error])
  useEffect(() => {
    if (detailQuery.error) reportApiError(detailQuery.error)
  }, [detailQuery.error])

  useEffect(() => {
    if (!me) return
    if (initializedFor.current !== identityKey) {
      initializedFor.current = identityKey
      setDraftFilters({})
      setAppliedFilters({})
      setPeriodStart(currentMonthDate())
      setPeriodEnd('')
      setChargesFirst(0)
      setPriceRuleFirst(0)
      setDetailChargeId(null)
      setPriceRuleDialog(null)
    }
    const requestId =
      new URLSearchParams(location.search).get('request_id') ?? undefined
    setDraftFilters((current) =>
      current.request_id === requestId
        ? current
        : { ...current, request_id: requestId },
    )
    setAppliedFilters((current) =>
      current.request_id === requestId
        ? current
        : { ...current, request_id: requestId },
    )
  }, [identityKey, location.search, me])

  useEffect(() => {
    const page = chargesQuery.data
    if (!page || page.rows <= 0) return
    if (page.charges.length === 0 && page.first >= page.total) {
      const correctedFirst =
        page.total > 0
          ? Math.floor((page.total - 1) / page.rows) * page.rows
          : 0
      if (correctedFirst !== chargesFirstRef.current) {
        chargesFirstRef.current = correctedFirst
        setChargesFirst(correctedFirst)
      }
      return
    }
    if (page.first !== chargesFirstRef.current) {
      chargesFirstRef.current = page.first
      setChargesFirst(page.first)
    }
    if (page.rows !== chargeRowsRef.current) {
      chargeRowsRef.current = page.rows
      setChargeRows(page.rows)
    }
  }, [chargesQuery.data, setChargeRows])

  useEffect(() => {
    const page = priceRulesQuery.data
    if (!page || page.rows <= 0) return
    if (page.rules.length === 0 && page.first >= page.total) {
      const correctedFirst =
        page.total > 0
          ? Math.floor((page.total - 1) / page.rows) * page.rows
          : 0
      if (correctedFirst !== priceRuleFirstRef.current) {
        priceRuleFirstRef.current = correctedFirst
        setPriceRuleFirst(correctedFirst)
      }
      return
    }
    if (page.first !== priceRuleFirstRef.current) {
      priceRuleFirstRef.current = page.first
      setPriceRuleFirst(page.first)
    }
    if (page.rows !== priceRuleRowsRef.current) {
      priceRuleRowsRef.current = page.rows
      setPriceRuleRows(page.rows)
    }
  }, [priceRulesQuery.data, setPriceRuleRows])

  async function refresh(): Promise<void> {
    await Promise.all([
      summaryQuery.refetch(),
      chargesQuery.refetch(),
      ...(isAdmin
        ? [
            priceRulesQuery.refetch(),
            endpointsQuery.refetch(),
            userOptionsQuery.refetch(),
          ]
        : []),
    ])
  }

  function changeFilters(filters: BillingChargeFilters): void {
    setDraftFilters(filters)
  }

  function applyFilters(): void {
    setAppliedFilters({ ...draftFilters })
    setChargesFirst(0)
  }

  function applyPeriod(start: string, end: string): void {
    const nextStart = start || currentMonthDate()
    const nextFilters: BillingChargeFilters = {
      ...draftFilters,
      start_at: startOfDate(nextStart),
      end_at: endOfDate(end),
    }
    setPeriodStart(nextStart)
    setPeriodEnd(end)
    setDraftFilters(nextFilters)
    setAppliedFilters(nextFilters)
    setChargesFirst(0)
  }

  function changeChargePage(change: TablePageChange): void {
    chargesFirstRef.current = change.first
    chargeRowsRef.current = change.rows
    setChargesFirst(change.first)
    setChargeRows(change.rows)
  }

  function changePriceRulePage(change: TablePageChange): void {
    priceRuleFirstRef.current = change.first
    priceRuleRowsRef.current = change.rows
    setPriceRuleFirst(change.first)
    setPriceRuleRows(change.rows)
  }

  function openDetail(charge: BillingChargeResponse): void {
    setDetailChargeId(charge.charge_id)
  }

  function openPriceRuleDialog(rule?: BillingPriceRuleResponse): void {
    setPriceRuleDialog({
      generation: (priceRuleDialog?.generation ?? 0) + 1,
      initialForm: newBillingPriceRuleForm(rule),
    })
  }

  function changePriceRuleDialog(open: boolean): void {
    if (open) return
    const priceRuleId = priceRuleDialog?.initialForm.price_rule_id ?? null
    if (asyncAction.isPending(billingPriceRuleActionKey(priceRuleId))) return
    setPriceRuleDialog(null)
  }

  async function savePriceRule(form: BillingPriceRuleForm): Promise<void> {
    const priceRuleId = form.price_rule_id ?? null
    const body = billingPriceRuleRequest(form)
    const generation = priceRuleDialog?.generation ?? 0
    const capturedFirst = priceRuleFirstRef.current
    const capturedRows = priceRuleRowsRef.current
    const capturedFilters = { ...appliedFiltersRef.current }
    try {
      const outcome = await asyncAction.run(
        billingPriceRuleActionKey(priceRuleId),
        async () => {
          if (priceRuleId) {
            await updatePriceRule.mutateAsync({
              path: { price_rule_id: priceRuleId },
              body,
            })
          } else {
            await createPriceRule.mutateAsync({ body })
          }
          await Promise.all([
            refreshBillingPriceRules(queryClient, capturedFirst, capturedRows),
            refreshBillingSummary(queryClient, capturedFilters),
          ])
        },
        'save',
      )
      if (!outcome.executed) return
      setPriceRuleDialog((current) =>
        current && current.generation === generation ? null : current,
      )
      notifySuccess(t(priceRuleId ? 'priceRuleUpdated' : 'priceRuleCreated'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function deletePriceRule(
    rule: BillingPriceRuleResponse,
  ): Promise<void> {
    if (!window.confirm(t('priceRuleDeleteConfirm'))) return
    const priceRuleId = rule.price_rule_id
    const capturedFirst = priceRuleFirstRef.current
    const capturedRows = priceRuleRowsRef.current
    const capturedFilters = { ...appliedFiltersRef.current }
    const capturedChargeFirst = chargesFirstRef.current
    const capturedChargeRows = chargeRowsRef.current
    try {
      const outcome = await asyncAction.run(
        billingPriceRuleActionKey(priceRuleId),
        async () => {
          await removePriceRule.mutateAsync({
            path: { price_rule_id: priceRuleId },
          })
          await Promise.all([
            refreshBillingPriceRules(queryClient, capturedFirst, capturedRows),
            refreshBillingSummary(queryClient, capturedFilters),
            refreshBillingCharges(
              queryClient,
              capturedFilters,
              capturedChargeFirst,
              capturedChargeRows,
            ),
          ])
        },
        'delete',
      )
      if (outcome.executed) notifySuccess(t('priceRuleDeleted'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function togglePriceRule(
    rule: BillingPriceRuleResponse,
  ): Promise<void> {
    const priceRuleId = rule.price_rule_id
    const enabled = !rule.enabled
    const capturedFirst = priceRuleFirstRef.current
    const capturedRows = priceRuleRowsRef.current
    try {
      const outcome = await asyncAction.run(
        billingPriceRuleActionKey(priceRuleId),
        async () => {
          await patchPriceRule.mutateAsync({
            path: { price_rule_id: priceRuleId },
            body: { enabled },
          })
          await refreshBillingPriceRules(
            queryClient,
            capturedFirst,
            capturedRows,
          )
        },
        'toggle',
      )
      if (!outcome.executed) return
      notifySuccess(t(enabled ? 'priceRuleEnabled' : 'priceRuleDisabled'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function reprice(): Promise<void> {
    const capturedFilters = { ...appliedFiltersRef.current }
    const capturedChargeFirst = chargesFirstRef.current
    const capturedChargeRows = chargeRowsRef.current
    const capturedPriceRuleFirst = priceRuleFirstRef.current
    const capturedPriceRuleRows = priceRuleRowsRef.current
    try {
      const outcome = await asyncAction.run(
        BILLING_REPRICE_KEY,
        async () => {
          const result = await repriceBilling.mutateAsync({
            body: { limit: 10_000 },
          })
          await Promise.all([
            refreshBillingSummary(queryClient, capturedFilters),
            refreshBillingCharges(
              queryClient,
              capturedFilters,
              capturedChargeFirst,
              capturedChargeRows,
            ),
            refreshBillingPriceRules(
              queryClient,
              capturedPriceRuleFirst,
              capturedPriceRuleRows,
            ),
          ])
          return result.repriced
        },
        'reprice',
      )
      if (outcome.executed) {
        notifySuccess(t('repriced', { count: outcome.value }))
      }
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function downloadCsv(kind: 'details' | 'monthly'): Promise<void> {
    try {
      const csv = await fetchBillingCsv(kind, appliedFiltersRef.current)
      const url = URL.createObjectURL(
        new Blob([`\ufeff${csv}`], { type: 'text/csv;charset=utf-8' }),
      )
      const link = document.createElement('a')
      link.href = url
      link.download = `prompt-ferry-billing-${kind}.csv`
      document.body.appendChild(link)
      link.click()
      link.remove()
      URL.revokeObjectURL(url)
      notifySuccess(t('billingCsvDownloaded'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  const draftKey = billingPriceRuleActionKey(
    priceRuleDialog?.initialForm.price_rule_id ?? null,
  )
  const draftPendingAction = asyncAction.pendingOrigin(draftKey)
  const draftPending = draftPendingAction !== null
  const summary = summaryQuery.data ?? null
  const charges = chargesQuery.data
  const priceRules = priceRulesQuery.data
  const detail = detailChargeId !== null ? (detailQuery.data ?? null) : null

  return (
    <section
      className="grid min-w-0 max-w-full gap-3"
      data-foundation-route-id="R-BILLING"
    >
      <HeaderActions>
        <Button
          aria-label={t('billingRefresh')}
          disabled={summaryQuery.isFetching}
          onClick={() => void refresh()}
          size="sm"
          type="button"
          variant="outline"
        >
          {summaryQuery.isFetching ? (
            <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
          ) : (
            <RefreshCw aria-hidden="true" />
          )}
          {t('billingRefresh')}
        </Button>
        <Button
          onClick={() => void downloadCsv('details')}
          size="sm"
          type="button"
          variant="outline"
        >
          <Download aria-hidden="true" />
          {t('billingExportDetails')}
        </Button>
        <Button
          onClick={() => void downloadCsv('monthly')}
          size="sm"
          type="button"
          variant="outline"
        >
          <Download aria-hidden="true" />
          {t('billingExportMonthly')}
        </Button>
        {isAdmin && (
          <Button
            disabled={asyncAction.isPending(BILLING_REPRICE_KEY)}
            onClick={() => void reprice()}
            size="sm"
            type="button"
            variant="outline"
          >
            {asyncAction.isPending(BILLING_REPRICE_KEY) ? (
              <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
            ) : (
              <Calculator aria-hidden="true" />
            )}
            {t('reprice')}
          </Button>
        )}
      </HeaderActions>

      <BillingFilters
        endpoints={endpointsQuery.data ?? []}
        filters={draftFilters}
        isAdmin={isAdmin}
        onApply={applyFilters}
        onChange={changeFilters}
        onPeriod={applyPeriod}
        periodEnd={periodEnd}
        periodStart={periodStart}
        users={userOptionsQuery.data ?? []}
      />

      <BillingSummary summary={summary} />

      <BillingCharges
        charges={charges?.charges ?? []}
        first={charges?.first ?? chargesFirst}
        isAdmin={isAdmin}
        loading={chargesQuery.isPending}
        onOpenDetail={openDetail}
        onPage={changeChargePage}
        rows={charges?.rows ?? chargeRows}
        total={charges?.total ?? 0}
      />

      {isAdmin && (
        <BillingPriceRules
          first={priceRules?.first ?? priceRuleFirst}
          loading={priceRulesQuery.isPending}
          onCreate={() => openPriceRuleDialog()}
          onDelete={(rule) => void deletePriceRule(rule)}
          onEdit={(rule) => openPriceRuleDialog(rule)}
          onPage={changePriceRulePage}
          onToggle={(rule) => void togglePriceRule(rule)}
          rows={priceRules?.rows ?? priceRuleRows}
          rules={priceRules?.rules ?? []}
          total={priceRules?.total ?? 0}
        />
      )}

      <BillingPriceRuleDialog
        initialForm={priceRuleDialog?.initialForm ?? newBillingPriceRuleForm()}
        key={`${priceRuleDialog?.initialForm.price_rule_id ?? 'new'}:${priceRuleDialog?.generation ?? 0}`}
        onOpenChange={changePriceRuleDialog}
        onSave={(form) => void savePriceRule(form)}
        open={priceRuleDialog !== null}
        pending={draftPending}
        savePending={draftPendingAction === 'save'}
      />

      <BillingChargeDetailDialog
        detail={detail}
        isAdmin={isAdmin}
        loading={detailChargeId !== null && detailQuery.isPending}
        onOpenChange={(open) => {
          if (!open) setDetailChargeId(null)
        }}
        open={detailChargeId !== null}
      />
    </section>
  )
}
