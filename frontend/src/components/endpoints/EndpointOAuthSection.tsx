import { RefreshCw, Trash2 } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import type { EndpointOAuthStatusResponse } from '@/generated/admin-api'
import type { AsyncActionOutcome } from '@/lib/async-action'
import { formatApiError } from '@/api'
import {
  clearEndpointOAuthToken,
  getEndpointOAuthStatus,
  refreshEndpointOAuthToken,
} from '@/api/endpoint-oauth'
import { EndpointOAuthBrowserLogin } from '@/components/endpoints/EndpointOAuthBrowserLogin'
import { EndpointOAuthDeviceLogin } from '@/components/endpoints/EndpointOAuthDeviceLogin'
import { Button } from '@/components/ui/button'
import { useLocale } from '@/lib/i18n'
import { copyText } from '@/lib/clipboard'
import {
  sanitizeOAuthErrorMessage,
  type OAuthStatusSnapshot,
  type RunOAuthAction,
} from '@/models/endpoints/oauth'

type EndpointOAuthSectionProps = {
  captureStatusSnapshot: () => OAuthStatusSnapshot
  actionPending: boolean
  dialogGeneration: number
  endpointId: string
  isSnapshotCurrent: (snapshot: OAuthStatusSnapshot) => boolean
  onStatus: (
    snapshot: OAuthStatusSnapshot,
    status: Pick<EndpointOAuthStatusResponse, 'has_oauth_token' | 'plan'>,
  ) => void
  runAction?: RunOAuthAction
}

function formatExpiresAt(value: string, locale: string): string {
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return value
  return new Intl.DateTimeFormat(locale, {
    day: 'numeric',
    hour: 'numeric',
    minute: 'numeric',
    month: 'numeric',
    second: 'numeric',
    year: 'numeric',
  }).format(date)
}

export function EndpointOAuthSection({
  actionPending,
  captureStatusSnapshot,
  dialogGeneration,
  endpointId,
  isSnapshotCurrent,
  onStatus,
  runAction,
}: EndpointOAuthSectionProps) {
  const { locale, t } = useLocale()
  const [status, setStatus] = useState<EndpointOAuthStatusResponse | null>(null)
  const [statusError, setStatusError] = useState('')
  const [refreshing, setRefreshing] = useState(false)
  const [clearing, setClearing] = useState(false)
  const readGenerationRef = useRef(0)
  const activeRef = useRef(true)
  const identityRef = useRef({ endpointId, dialogGeneration })
  const captureRef = useRef(captureStatusSnapshot)
  const isCurrentRef = useRef(isSnapshotCurrent)
  const onStatusRef = useRef(onStatus)
  const runActionRef = useRef(runAction)

  identityRef.current = { endpointId, dialogGeneration }
  captureRef.current = captureStatusSnapshot
  isCurrentRef.current = isSnapshotCurrent
  onStatusRef.current = onStatus
  runActionRef.current = runAction

  function isCurrent(snapshot: OAuthStatusSnapshot): boolean {
    return (
      activeRef.current &&
      snapshot.endpointId === identityRef.current.endpointId &&
      snapshot.dialogGeneration === identityRef.current.dialogGeneration &&
      isCurrentRef.current(snapshot)
    )
  }

  function applyStatus(
    snapshot: OAuthStatusSnapshot,
    next: EndpointOAuthStatusResponse,
  ): void {
    if (!isCurrent(snapshot)) return
    setStatus(next)
    setStatusError('')
    onStatusRef.current(snapshot, {
      has_oauth_token: next.has_oauth_token,
      plan: next.plan,
    })
  }

  async function refreshStatus(): Promise<void> {
    const snapshot = captureRef.current()
    if (!isCurrent(snapshot)) return
    const requestGeneration = ++readGenerationRef.current
    setStatusError('')
    try {
      const next = await getEndpointOAuthStatus(snapshot.endpointId)
      if (
        requestGeneration !== readGenerationRef.current ||
        !isCurrent(snapshot)
      ) {
        return
      }
      applyStatus(snapshot, next)
    } catch (cause) {
      if (
        requestGeneration === readGenerationRef.current &&
        isCurrent(snapshot)
      ) {
        setStatusError(
          sanitizeOAuthErrorMessage(formatApiError(cause), [], t('error')),
        )
      }
    }
  }

  function invalidateReads(endpointIdToInvalidate: string): void {
    if (endpointIdToInvalidate === identityRef.current.endpointId) {
      readGenerationRef.current += 1
    }
  }

  async function runGuarded<T>(
    actionEndpointId: string,
    action: () => Promise<T>,
    label: string,
  ): Promise<AsyncActionOutcome<T>> {
    const snapshot = captureRef.current()
    const run = runActionRef.current
    if (
      !run ||
      actionEndpointId !== snapshot.endpointId ||
      !isCurrent(snapshot)
    ) {
      return { executed: false }
    }
    return run(
      actionEndpointId,
      async () => {
        if (!isCurrent(snapshot)) return undefined as T
        invalidateReads(actionEndpointId)
        return action()
      },
      label,
    )
  }

  useEffect(() => {
    activeRef.current = true
    setStatus(null)
    setStatusError('')
    setRefreshing(false)
    setClearing(false)
    void refreshStatus()
    return () => {
      activeRef.current = false
      readGenerationRef.current += 1
    }
  }, [dialogGeneration, endpointId])

  async function refreshToken(): Promise<void> {
    const snapshot = captureRef.current()
    let claimed = false
    try {
      await runGuarded(
        snapshot.endpointId,
        async () => {
          claimed = true
          setRefreshing(true)
          setStatusError('')
          const next = await refreshEndpointOAuthToken(snapshot.endpointId)
          applyStatus(snapshot, next)
        },
        'oauth-refresh',
      )
    } catch (cause) {
      if (claimed && isCurrent(snapshot)) {
        setStatusError(
          sanitizeOAuthErrorMessage(formatApiError(cause), [], t('error')),
        )
      }
    } finally {
      if (claimed && isCurrent(snapshot)) {
        setRefreshing(false)
      }
    }
  }

  async function clearToken(): Promise<void> {
    const snapshot = captureRef.current()
    let claimed = false
    try {
      await runGuarded(
        snapshot.endpointId,
        async () => {
          claimed = true
          setClearing(true)
          setStatusError('')
          await clearEndpointOAuthToken(snapshot.endpointId)
          if (isCurrent(snapshot)) await refreshStatus()
        },
        'oauth-clear',
      )
    } catch (cause) {
      if (claimed && isCurrent(snapshot)) {
        setStatusError(
          sanitizeOAuthErrorMessage(formatApiError(cause), [], t('error')),
        )
      }
    } finally {
      if (claimed && isCurrent(snapshot)) {
        setClearing(false)
      }
    }
  }

  async function copy(value: string): Promise<void> {
    try {
      await copyText(value)
    } catch {
      // Clipboard access is optional; copy remains an explicit action.
    }
  }

  const hasToken = status?.has_oauth_token ?? false
  const plan = status?.plan ?? 'platform_api_key'
  const expiresAt = status?.expires_at
  const statusText = !hasToken
    ? t('endpointOAuthNotLoggedIn')
    : status?.expired
      ? t('endpointOAuthExpired')
      : t('endpointOAuthLoggedIn')
  const statusTone = !hasToken
    ? 'border-muted text-muted-foreground'
    : status?.expired
      ? 'border-amber-500/40 text-amber-700 dark:text-amber-300'
      : 'border-emerald-600/40 text-emerald-700 dark:text-emerald-300'

  return (
    <section
      aria-label={t('endpointOAuth')}
      className="grid min-w-0 gap-3 rounded-lg border p-3"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-sm font-medium">{t('endpointOAuth')}</span>
          <span
            className={`rounded-full border px-2 py-0.5 text-xs ${statusTone}`}
          >
            {statusText}
          </span>
          {hasToken ? (
            <span className="rounded-full border px-2 py-0.5 text-xs text-muted-foreground">
              {plan === 'chatgpt_subscription'
                ? t('endpointPlanChatgptSubscription')
                : t('endpointPlanPlatformApiKey')}
            </span>
          ) : null}
        </div>
        {hasToken ? (
          <div className="flex items-center gap-2">
            <Button
              disabled={actionPending || refreshing || clearing}
              onClick={() => void refreshToken()}
              size="sm"
              type="button"
              variant="outline"
            >
              {refreshing ? (
                <RefreshCw aria-hidden="true" className="animate-spin" />
              ) : (
                <RefreshCw aria-hidden="true" />
              )}
              {t('endpointOAuthRefresh')}
            </Button>
            <Button
              disabled={actionPending || refreshing || clearing}
              onClick={() => void clearToken()}
              size="sm"
              type="button"
              variant="outline"
            >
              {clearing ? (
                <Trash2 aria-hidden="true" className="animate-pulse" />
              ) : (
                <Trash2 aria-hidden="true" />
              )}
              {t('endpointOAuthClear')}
            </Button>
          </div>
        ) : null}
      </div>
      {statusError ? (
        <p className="break-words text-sm text-destructive">{statusError}</p>
      ) : null}
      {hasToken && expiresAt ? (
        <p className="text-xs text-muted-foreground">
          {t('endpointOAuthExpiresAt')}: {formatExpiresAt(expiresAt, locale)}
        </p>
      ) : null}
      {!hasToken ? (
        <div className="grid gap-4 border-t pt-3">
          <EndpointOAuthDeviceLogin
            actionPending={actionPending}
            captureStatusSnapshot={captureRef.current}
            dialogGeneration={dialogGeneration}
            endpointId={endpointId}
            isSnapshotCurrent={isCurrentRef.current}
            refreshStatus={refreshStatus}
            runAction={(id, action, label) => runGuarded(id, action, label)}
          />
          <EndpointOAuthBrowserLogin
            actionPending={actionPending}
            captureStatusSnapshot={captureRef.current}
            dialogGeneration={dialogGeneration}
            endpointId={endpointId}
            isSnapshotCurrent={isCurrentRef.current}
            refreshStatus={refreshStatus}
            runAction={(id, action, label) => runGuarded(id, action, label)}
          />
        </div>
      ) : null}
    </section>
  )
}
