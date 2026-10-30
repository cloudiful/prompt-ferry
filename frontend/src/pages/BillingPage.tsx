import { useEffect, useRef, useState } from 'react'
import { useLocation } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { Calculator, Download, RefreshCw } from 'lucide-react'
import type {
  BillingChargeResponse,
  BillingPriceRuleResponse,
} from '@/generated/admin-api'
import { BillingChargeDetailDialog } from '@/components/billing/BillingChargeDetailDialog'
import { BillingCharges } from '@/components/billing/BillingCharges'
import { BillingFilters } from '@/components/billing/BillingFilters'
import { BillingPriceRuleDialog } from '@/components/billing/BillingPriceRuleDialog'
import { BillingPriceRules } from '@/components/billing/BillingPriceRules'
import { BillingSummary } from '@/components/billing/BillingSummary'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { usePageTitle } from '@/lib/router'
import { useSessionStore } from '@/lib/session'
import { useAsyncAction } from '@/lib/use-async-action'
import { billingPriceRuleActionKey, useBillingStore } from '@/lib/billing'
import { newBillingPriceRuleForm } from '@/lib/billing-model'
import type { TablePageChange } from '@/models/pagination'

export function BillingPage() {
  const { t } = useLocale()
  const location = useLocation()
  const queryClient = useQueryClient()
  const { notifyApiError, notifySuccess } = useNotifier()
  const asyncAction = useAsyncAction()
  const me = useSessionStore((state) => state.me)
  const isAdmin = me?.is_admin === true
  const initializedFor = useRef<string | null>(null)
  const [priceRuleDialogOpen, setPriceRuleDialogOpen] = useState(false)

  const filters = useBillingStore((state) => state.filters)
  const periodStart = useBillingStore((state) => state.periodStart)
  const periodEnd = useBillingStore((state) => state.periodEnd)
  const summary = useBillingStore((state) => state.summary)
  const summaryLoading = useBillingStore((state) => state.summaryLoading)
  const charges = useBillingStore((state) => state.charges)
  const chargesTotal = useBillingStore((state) => state.chargesTotal)
  const chargesFirst = useBillingStore((state) => state.chargesFirst)
  const chargesRows = useBillingStore((state) => state.chargesRows)
  const chargesLoading = useBillingStore((state) => state.chargesLoading)
  const priceRules = useBillingStore((state) => state.priceRules)
  const priceRuleTotal = useBillingStore((state) => state.priceRuleTotal)
  const priceRuleFirst = useBillingStore((state) => state.priceRuleFirst)
  const priceRuleRows = useBillingStore((state) => state.priceRuleRows)
  const priceRulesLoading = useBillingStore((state) => state.priceRulesLoading)
  const endpoints = useBillingStore((state) => state.endpoints)
  const userOptions = useBillingStore((state) => state.userOptions)
  const detail = useBillingStore((state) => state.detail)
  const detailChargeId = useBillingStore((state) => state.detailChargeId)
  const detailLoading = useBillingStore((state) => state.detailLoading)
  const priceRuleDraft = useBillingStore((state) => state.priceRuleDraft)

  usePageTitle('billing')

  useEffect(() => {
    if (!me) return
    const store = useBillingStore.getState()
    store.initializeIdentity({
      user_id: me.user_id,
      is_admin: me.is_admin === true,
    })
    const current = useBillingStore.getState()
    if (initializedFor.current === current.identityKey) return
    initializedFor.current = current.identityKey
    const requestId =
      new URLSearchParams(location.search).get('request_id') ?? undefined
    current.configureFilters({ ...current.filters, request_id: requestId })
    current.configurePeriod(current.periodStart, current.periodEnd)
    void refresh()
  }, [me, location.search])

  async function refresh(): Promise<void> {
    try {
      await useBillingStore.getState().refresh(queryClient)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function applyFilters(): Promise<void> {
    try {
      await useBillingStore
        .getState()
        .applyFilters(queryClient, useBillingStore.getState().filters)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function applyPeriod(start: string, end: string): Promise<void> {
    try {
      await useBillingStore.getState().applyPeriod(queryClient, start, end)
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  function changeChargePage(change: TablePageChange): void {
    void useBillingStore
      .getState()
      .setChargePage(queryClient, change.first, change.rows)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  function changePriceRulePage(change: TablePageChange): void {
    void useBillingStore
      .getState()
      .setPriceRulePage(queryClient, change.first, change.rows)
      .catch((cause: unknown) => notifyApiError(cause))
  }

  async function openDetail(charge: BillingChargeResponse): Promise<void> {
    const chargeId = charge.charge_id
    try {
      await useBillingStore.getState().openDetail(chargeId)
    } catch (cause) {
      if (useBillingStore.getState().detailChargeId === chargeId) {
        useBillingStore.getState().closeDetail()
      }
      notifyApiError(cause)
    }
  }

  function openPriceRuleDialog(rule?: BillingPriceRuleResponse): void {
    setPriceRuleDialogOpen(true)
    useBillingStore.getState().setPriceRuleDraft(newBillingPriceRuleForm(rule))
  }

  function changePriceRuleDialog(open: boolean): void {
    if (!open && draftPending) return
    setPriceRuleDialogOpen(open)
    if (!open) useBillingStore.getState().setPriceRuleDraft(null)
  }

  async function savePriceRule(): Promise<void> {
    const draft = useBillingStore.getState().priceRuleDraft
    if (!draft) return
    const priceRuleId = draft.price_rule_id ?? null
    try {
      const outcome = await useBillingStore
        .getState()
        .savePriceRule(queryClient, draft)
      if (!outcome.executed) return
      notifySuccess(t(priceRuleId ? 'priceRuleUpdated' : 'priceRuleCreated'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function deletePriceRule(
    rule: BillingPriceRuleResponse,
  ): Promise<void> {
    if (!window.confirm(t('priceRuleDeleteConfirm'))) return
    try {
      const outcome = await useBillingStore
        .getState()
        .removePriceRule(queryClient, rule.price_rule_id)
      if (outcome.executed) notifySuccess(t('priceRuleDeleted'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function togglePriceRule(
    rule: BillingPriceRuleResponse,
  ): Promise<void> {
    try {
      const outcome = await useBillingStore
        .getState()
        .togglePriceRule(queryClient, rule)
      if (!outcome.executed) return
      notifySuccess(t(rule.enabled ? 'priceRuleDisabled' : 'priceRuleEnabled'))
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function reprice(): Promise<void> {
    try {
      const outcome = await useBillingStore.getState().reprice(queryClient)
      if (outcome.executed) {
        notifySuccess(t('repriced', { count: outcome.value }))
      }
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function downloadCsv(kind: 'details' | 'monthly'): Promise<void> {
    try {
      const csv = await useBillingStore.getState().fetchCsvText(kind)
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
      notifyApiError(cause)
    }
  }

  const draftKey = billingPriceRuleActionKey(
    priceRuleDraft?.price_rule_id ?? null,
  )
  const draftPendingAction = asyncAction.pendingOrigin(draftKey)
  const draftPending = draftPendingAction !== null
  const priceRuleDialogVisible = priceRuleDialogOpen && priceRuleDraft

  return (
    <section
      className="grid min-w-0 max-w-full gap-3"
      data-foundation-route-id="R-BILLING"
    >
      <HeaderActions>
        <Button
          aria-label={t('billingRefresh')}
          disabled={summaryLoading}
          onClick={() => void refresh()}
          size="sm"
          type="button"
          variant="outline"
        >
          {summaryLoading ? (
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
            disabled={asyncAction.isPending('billing:reprice')}
            onClick={() => void reprice()}
            size="sm"
            type="button"
            variant="outline"
          >
            {asyncAction.isPending('billing:reprice') ? (
              <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
            ) : (
              <Calculator aria-hidden="true" />
            )}
            {t('reprice')}
          </Button>
        )}
      </HeaderActions>

      <BillingFilters
        endpoints={endpoints}
        filters={filters}
        isAdmin={isAdmin}
        onApply={() => void applyFilters()}
        onChange={(next) => useBillingStore.getState().configureFilters(next)}
        onPeriod={(start, end) => void applyPeriod(start, end)}
        periodEnd={periodEnd}
        periodStart={periodStart}
        users={userOptions}
      />

      <BillingSummary summary={summary} />

      <BillingCharges
        charges={charges}
        first={chargesFirst}
        isAdmin={isAdmin}
        loading={chargesLoading}
        onOpenDetail={(charge) => void openDetail(charge)}
        onPage={changeChargePage}
        rows={chargesRows}
        total={chargesTotal}
      />

      {isAdmin && (
        <BillingPriceRules
          first={priceRuleFirst}
          loading={priceRulesLoading}
          onCreate={() => openPriceRuleDialog()}
          onDelete={(rule) => void deletePriceRule(rule)}
          onEdit={(rule) => openPriceRuleDialog(rule)}
          onPage={changePriceRulePage}
          onToggle={(rule) => void togglePriceRule(rule)}
          rows={priceRuleRows}
          rules={priceRules}
          total={priceRuleTotal}
        />
      )}

      <BillingPriceRuleDialog
        form={priceRuleDraft ?? newBillingPriceRuleForm()}
        onChange={(patch) => {
          const draft = useBillingStore.getState().priceRuleDraft
          if (!draft) return
          useBillingStore.getState().setPriceRuleDraft({ ...draft, ...patch })
        }}
        onOpenChange={changePriceRuleDialog}
        onSave={() => void savePriceRule()}
        open={Boolean(priceRuleDialogVisible)}
        pending={draftPending}
        savePending={draftPendingAction === 'save'}
      />

      <BillingChargeDetailDialog
        detail={detail}
        isAdmin={isAdmin}
        loading={detailLoading}
        onOpenChange={(open) => {
          if (!open) useBillingStore.getState().closeDetail()
        }}
        open={detailChargeId !== null}
      />
    </section>
  )
}
