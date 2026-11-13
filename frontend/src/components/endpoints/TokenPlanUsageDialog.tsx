import { useEffect, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { LoaderCircle } from 'lucide-react'
import { Badge } from '@/components/ui/badge'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { KeyQuotaObservationHistory } from '@/components/endpoints/KeyQuotaObservationHistory'
import { QuotaInferencePanel } from '@/components/endpoints/QuotaInferencePanel'
import { TokenPlanKeyQuotaStatus } from '@/components/endpoints/TokenPlanKeyQuotaStatus'
import { TokenPlanQuotaWindowRow } from '@/components/endpoints/TokenPlanQuotaWindowRow'
import {
  quotaWindowLabel,
  TokenPlanQuotaWindows,
} from '@/components/endpoints/TokenPlanQuotaWindows'
import {
  toCanonicalQuotaRows,
  type CanonicalQuotaRow,
} from '@/composables/useCanonicalQuotaWindows'
import {
  effectiveQuotaStatus,
  isFeedbackState,
} from '@/composables/tokenPlanEffectiveKeys'
import type {
  EndpointPlan,
  EndpointProvider,
  TokenPlanKeyUsage,
  TokenPlanQuotaState,
  TokenPlanQuotaStatus,
  TokenPlanUsageResponse,
} from '@/generated/admin-api'
import { tokenPlanUsageOptions } from '@/generated/admin-api/@tanstack/react-query.gen'
import {
  TOKEN_PLAN_CACHE_TTL_MS,
  tokenPlanUsageSnapshot,
} from '@/lib/quota-cache'
import { useLocale, type Translate } from '@/lib/i18n'
import {
  deepseekBalanceAmounts,
  type TokenPlanFetchState,
} from '@/models/endpoints/quota'
import {
  formatMoney,
  formatOpenRouterCredits,
} from '@/models/endpoints/token-plan-window'

export type TokenPlanUsageDialogProps = {
  endpointId: string
  endpointName: string
  onError: (cause: unknown) => void
  onOpenChange: (open: boolean) => void
  open: boolean
  provider: EndpointProvider
  plan?: EndpointPlan
}

const MUTED_QUOTA_STATES: ReadonlySet<TokenPlanQuotaState> = new Set([
  'exhausted',
  'cooling',
  'rate_limited',
  'upstream_unavailable',
])

function isMutedQuotaStatus(status: TokenPlanQuotaStatus | null): boolean {
  return status !== null && MUTED_QUOTA_STATES.has(status.state)
}

function quotaStatusForKey(
  key: TokenPlanKeyUsage,
  fetchState: TokenPlanFetchState,
  nowMs: number,
): TokenPlanQuotaStatus | null {
  const effective = effectiveQuotaStatus(key, nowMs)
  if (effective !== key.quota_status) return effective
  if (fetchState !== 'stale') return effective
  if (isFeedbackState(effective?.state)) return effective
  return {
    state: 'unknown',
    observed_at: key.quota_status?.observed_at ?? null,
    cooldown_until: null,
    refresh_failed: key.quota_status?.refresh_failed ?? false,
    scope: 'text_inference',
  }
}

function factRows(key: TokenPlanKeyUsage, nowMs: number): CanonicalQuotaRow[] {
  return toCanonicalQuotaRows(key, nowMs).filter(
    (row) =>
      row.sourceModel === null &&
      row.sourceId !== 'command_code:balances' &&
      (row.kind === 'balance' || row.kind === 'unlimited'),
  )
}

function topLevelLabel(row: CanonicalQuotaRow, t: Translate): string {
  if (row.sourceId.startsWith('glm:')) {
    return row.kind === 'weekly' ? t('tokenPlanWeekly') : t('tokenPlanInterval')
  }
  if (row.sourceId.startsWith('opencode_go:')) {
    if (row.kind === 'rolling') return t('tokenPlanRolling')
    if (row.kind === 'weekly') return t('tokenPlanWeekly')
    return t('tokenPlanMonthly')
  }
  return quotaWindowLabel(row, t)
}

function amountOrUnknown(
  value: number | null | undefined,
  t: TranslateFn,
  currency?: string,
): string {
  if (typeof value !== 'number' || !Number.isFinite(value)) {
    return t('tokenPlanBalanceUnknown')
  }
  return formatMoney(currency, value)
}

function openRouterCredit(
  value: number | null | undefined,
  t: TranslateFn,
): string {
  return formatOpenRouterCredits(value, t)
}

function keyedValue(label: string, value: string) {
  return (
    <span className="break-words" key={label}>
      {label}: {value}
    </span>
  )
}

export function TokenPlanUsageDialog({
  endpointId,
  endpointName,
  onError,
  onOpenChange,
  open,
  provider,
  plan,
}: TokenPlanUsageDialogProps) {
  const { t } = useLocale()
  const onErrorRef = useRef(onError)
  const [nowMs, setNowMs] = useState(() => Date.now())
  onErrorRef.current = onError

  const usageQuery = useQuery({
    ...tokenPlanUsageOptions({ path: { endpoint_id: endpointId } }),
    enabled: open && endpointId !== '',
    staleTime: TOKEN_PLAN_CACHE_TTL_MS,
    refetchOnWindowFocus: true,
    refetchInterval: TOKEN_PLAN_CACHE_TTL_MS,
  })

  useEffect(() => {
    if (!open || !endpointId || !usageQuery.error) return
    onErrorRef.current(usageQuery.error)
  }, [endpointId, open, usageQuery.error])

  useEffect(() => {
    if (!open) return
    setNowMs(Date.now())
    const ticker = window.setInterval(() => setNowMs(Date.now()), 1000)
    return () => window.clearInterval(ticker)
  }, [open])

  const snapshot = tokenPlanUsageSnapshot(usageQuery, nowMs)
  const usage: TokenPlanUsageResponse | null = snapshot.usage
  const translate = t as unknown as TranslateFn
  const configuredKeys = (usage?.keys ?? []).map((key) => ({
    keyId: key.key_id,
    label: key.key_label,
  }))

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent className="flex max-h-[calc(100dvh-2rem)] flex-col gap-3 overflow-hidden p-4 sm:max-w-[min(calc(100vw_-_2rem),72rem)] sm:p-6">
        <DialogHeader className="min-w-0 shrink-0 pr-8">
          <DialogTitle className="break-words">
            {t('tokenPlanUsage')} / {endpointName}
          </DialogTitle>
        </DialogHeader>
        <div
          className="grid min-h-0 min-w-0 flex-1 grid-cols-[minmax(0,1fr)] gap-4 overflow-y-auto overscroll-contain pr-1 text-xs"
          data-quota-dialog={endpointId}
          data-quota-provider={provider}
        >
          {usage === null && snapshot.state === 'loading' ? (
            <div className="flex items-center gap-2 text-muted-foreground">
              <LoaderCircle
                aria-hidden="true"
                className="size-4 animate-spin"
              />
              <span>{t('loading')}</span>
            </div>
          ) : usage === null ? (
            <p className="text-muted-foreground">
              {snapshot.state === 'error'
                ? t('tokenPlanUsageFailed')
                : t('tokenPlanNoUsage')}
            </p>
          ) : (
            <>
              {snapshot.state === 'stale' ? (
                <Badge
                  className="w-fit text-amber-700 dark:text-amber-300"
                  variant="outline"
                >
                  {t('quotaCachedObservation')}
                </Badge>
              ) : null}
              {usage.keys.map((key) => {
                const status = quotaStatusForKey(key, snapshot.state, nowMs)
                const muted = isMutedQuotaStatus(status)
                const facts = factRows(key, nowMs)
                const deepseekAmounts = deepseekBalanceAmounts(
                  key.deepseek_balance,
                )
                const deepseekEntries = key.deepseek_balance?.balances ?? []
                return (
                  <details
                    className="min-w-0 border-b last:border-b-0"
                    data-quota-key={key.key_id}
                    data-quota-muted={muted ? 'true' : 'false'}
                    key={key.key_id}
                    open
                  >
                    <summary className="flex min-w-0 cursor-pointer list-none flex-wrap items-center justify-between gap-x-3 gap-y-1 px-1 py-2 [&::-webkit-details-marker]:hidden">
                      <span className="flex min-w-0 flex-wrap items-center gap-2">
                        <span
                          className="min-w-0 break-all font-semibold text-foreground"
                          title={key.key_label}
                        >
                          {key.key_label}
                        </span>
                        <TokenPlanKeyQuotaStatus status={status} />
                        {key.openrouter_balance?.is_free_tier ? (
                          <Badge
                            title={t('tokenPlanFreeTierHint')}
                            variant="secondary"
                          >
                            {t('tokenPlanFreeTier')}
                          </Badge>
                        ) : null}
                      </span>
                    </summary>
                    <div
                      className={
                        muted
                          ? 'grid min-w-0 gap-3 px-1 pb-3 text-muted-foreground'
                          : 'grid min-w-0 gap-3 px-1 pb-3'
                      }
                    >
                      {!key.ok ? (
                        <p className="break-words text-red-700 dark:text-red-300">
                          {key.error_message ?? t('tokenPlanUsageFailed')}
                        </p>
                      ) : (
                        <>
                          {key.openrouter_balance ? (
                            <section className="grid min-w-0 gap-1 rounded-md border p-3">
                              <h3
                                className={
                                  muted
                                    ? 'break-words font-medium text-muted-foreground'
                                    : 'break-words font-medium text-foreground'
                                }
                              >
                                {t('tokenPlanOpenRouterBalance')}
                              </h3>
                              <div className="flex min-w-0 flex-wrap gap-x-4 gap-y-1">
                                {keyedValue(
                                  t('tokenPlanLimit'),
                                  openRouterCredit(
                                    key.openrouter_balance.limit,
                                    translate,
                                  ),
                                )}
                                {keyedValue(
                                  t('tokenPlanLimitRemaining'),
                                  openRouterCredit(
                                    key.openrouter_balance.limit_remaining,
                                    translate,
                                  ),
                                )}
                                {key.openrouter_balance.limit_reset ? (
                                  <span className="break-words">
                                    {t('tokenPlanLimitReset')}:{' '}
                                    {key.openrouter_balance.limit_reset}
                                  </span>
                                ) : null}
                                {keyedValue(
                                  t('tokenPlanTotalCredits'),
                                  openRouterCredit(
                                    key.openrouter_balance.total_credits,
                                    translate,
                                  ),
                                )}
                                {keyedValue(
                                  t('tokenPlanTotalUsage'),
                                  openRouterCredit(
                                    key.openrouter_balance.total_usage,
                                    translate,
                                  ),
                                )}
                              </div>
                              {key.openrouter_balance.is_free_tier ? (
                                <p className="text-muted-foreground">
                                  {t('tokenPlanFreeTierHint')}
                                </p>
                              ) : null}
                            </section>
                          ) : null}
                          {key.deepseek_balance ? (
                            <section className="grid min-w-0 gap-1 rounded-md border p-3">
                              <h3
                                className={
                                  muted
                                    ? 'break-words font-medium text-muted-foreground'
                                    : 'break-words font-medium text-foreground'
                                }
                              >
                                {t('tokenPlanDeepSeekBalance')}
                              </h3>
                              <span className="break-words">
                                {t('status')}:{' '}
                                {key.deepseek_balance.is_available
                                  ? t('tokenPlanAvailable')
                                  : t('tokenPlanUnavailable')}
                              </span>
                              {deepseekEntries.map((entry, index) => {
                                const amount = deepseekAmounts[index]
                                return (
                                  <div
                                    className="flex min-w-0 flex-wrap gap-x-4 gap-y-1"
                                    key={`${entry.currency}:${index}`}
                                  >
                                    <span className="font-semibold">
                                      {entry.currency}
                                    </span>
                                    {keyedValue(
                                      t('tokenPlanTotalBalance'),
                                      amount
                                        ? amountOrUnknown(
                                            amount.total,
                                            translate,
                                            amount.currency,
                                          )
                                        : t('tokenPlanBalanceUnknown'),
                                    )}
                                    {keyedValue(
                                      t('tokenPlanGrantedBalance'),
                                      amountOrUnknown(
                                        entry.granted_balance,
                                        translate,
                                        entry.currency,
                                      ),
                                    )}
                                    {keyedValue(
                                      t('tokenPlanToppedUpBalance'),
                                      amountOrUnknown(
                                        entry.topped_up_balance,
                                        translate,
                                        entry.currency,
                                      ),
                                    )}
                                  </div>
                                )
                              })}
                            </section>
                          ) : null}
                          {key.openrouter_spend ? (
                            <section className="grid min-w-0 gap-1 rounded-md border p-3">
                              <h3
                                className={
                                  muted
                                    ? 'break-words font-medium text-muted-foreground'
                                    : 'break-words font-medium text-foreground'
                                }
                              >
                                {t('tokenPlanOpenRouterSpend')}
                              </h3>
                              <div className="flex min-w-0 flex-wrap gap-x-4 gap-y-1">
                                {keyedValue(
                                  t('tokenPlanSpendUsage'),
                                  key.openrouter_spend.usage.toFixed(2),
                                )}
                                {keyedValue(
                                  t('tokenPlanSpendDaily'),
                                  key.openrouter_spend.daily.toFixed(2),
                                )}
                                {keyedValue(
                                  t('tokenPlanSpendWeekly'),
                                  key.openrouter_spend.weekly.toFixed(2),
                                )}
                                {keyedValue(
                                  t('tokenPlanSpendMonthly'),
                                  key.openrouter_spend.monthly.toFixed(2),
                                )}
                              </div>
                            </section>
                          ) : null}
                          {facts.map((row) => (
                            <TokenPlanQuotaWindowRow
                              key={row.sourceId}
                              label={topLevelLabel(row, t)}
                              muted={muted}
                              nowMs={nowMs}
                              quantitySuffix={
                                row.unit === 'currency' ? ' USD' : undefined
                              }
                              row={row}
                            />
                          ))}
                          <TokenPlanQuotaWindows
                            keyUsage={key}
                            muted={muted}
                            nowMs={nowMs}
                          />
                        </>
                      )}
                    </div>
                  </details>
                )
              })}
              {usage.keys.length === 0 ? (
                <p className="text-muted-foreground">{t('tokenPlanNoUsage')}</p>
              ) : null}
            </>
          )}
          {open &&
          provider === 'openai' &&
          plan === 'chatgpt_subscription' &&
          usage?.quota_inference_available === true ? (
            <QuotaInferencePanel endpointId={endpointId} enabled />
          ) : null}
          {open && endpointId && provider !== 'generic' ? (
            <KeyQuotaObservationHistory
              configuredKeys={configuredKeys}
              endpointId={endpointId}
              key={`observations-${endpointId}`}
            />
          ) : null}
        </div>
      </DialogContent>
    </Dialog>
  )
}
