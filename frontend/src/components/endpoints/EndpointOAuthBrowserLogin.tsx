import { Copy, LoaderCircle } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import type { OAuthBrowserStartResponse } from '@/generated/admin-api'
import { formatApiError } from '@/api'
import {
  completeEndpointBrowserLogin,
  startEndpointBrowserLogin,
} from '@/api/endpoint-oauth'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { useLocale } from '@/lib/i18n'
import { copyText } from '@/lib/clipboard'
import {
  sanitizeOAuthErrorMessage,
  type OAuthStatusSnapshot,
  type RunOAuthAction,
} from '@/models/endpoints/oauth'

type EndpointOAuthBrowserLoginProps = {
  actionPending: boolean
  captureStatusSnapshot: () => OAuthStatusSnapshot
  dialogGeneration: number
  endpointId: string
  isSnapshotCurrent: (snapshot: OAuthStatusSnapshot) => boolean
  refreshStatus: () => Promise<void>
  runAction: RunOAuthAction
}

export function EndpointOAuthBrowserLogin({
  actionPending,
  captureStatusSnapshot,
  dialogGeneration,
  endpointId,
  isSnapshotCurrent,
  refreshStatus,
  runAction,
}: EndpointOAuthBrowserLoginProps) {
  const { t } = useLocale()
  const [browser, setBrowser] = useState<OAuthBrowserStartResponse | null>(null)
  const [redirectUrl, setRedirectUrl] = useState('')
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('')
  const browserRef = useRef<OAuthBrowserStartResponse | null>(null)
  const redirectUrlRef = useRef('')
  const busyRef = useRef(false)
  const flowGenerationRef = useRef(0)
  const flowEndpointIdRef = useRef('')
  const captureRef = useRef(captureStatusSnapshot)
  const isCurrentRef = useRef(isSnapshotCurrent)
  const refreshRef = useRef(refreshStatus)
  const runActionRef = useRef(runAction)

  captureRef.current = captureStatusSnapshot
  isCurrentRef.current = isSnapshotCurrent
  refreshRef.current = refreshStatus
  runActionRef.current = runAction

  function clearFlow(): void {
    flowGenerationRef.current += 1
    flowEndpointIdRef.current = ''
    browserRef.current = null
    redirectUrlRef.current = ''
    busyRef.current = false
  }

  useEffect(() => {
    clearFlow()
    setBrowser(null)
    setRedirectUrl('')
    setBusy(false)
    setMessage('')
    return clearFlow
  }, [dialogGeneration, endpointId])

  function currentFlowIsValid(
    snapshot: OAuthStatusSnapshot,
    flowGeneration: number,
    flowId?: string,
  ): boolean {
    return (
      snapshot.endpointId === endpointId &&
      snapshot.dialogGeneration === dialogGeneration &&
      isCurrentRef.current(snapshot) &&
      flowGenerationRef.current === flowGeneration &&
      flowEndpointIdRef.current === endpointId &&
      (flowId === undefined || browserRef.current?.flow_id === flowId)
    )
  }

  function updateRedirectUrl(value: string): void {
    redirectUrlRef.current = value
    setRedirectUrl(value)
  }

  async function startBrowser(): Promise<void> {
    if (busyRef.current) return
    const snapshot = captureRef.current()
    if (snapshot.endpointId !== endpointId) return
    let claimed = false
    let flowGeneration = -1

    try {
      const outcome = await runActionRef.current(
        snapshot.endpointId,
        async () => {
          claimed = true
          flowGeneration = ++flowGenerationRef.current
          flowEndpointIdRef.current = snapshot.endpointId
          browserRef.current = null
          redirectUrlRef.current = ''
          setBrowser(null)
          setRedirectUrl('')
          setMessage('')
          busyRef.current = true
          setBusy(true)
          return startEndpointBrowserLogin(snapshot.endpointId)
        },
        'oauth-browser-start',
      )
      if (!outcome.executed || !currentFlowIsValid(snapshot, flowGeneration)) {
        return
      }
      browserRef.current = outcome.value
      setBrowser(outcome.value)
    } catch (cause) {
      if (claimed && currentFlowIsValid(snapshot, flowGeneration)) {
        setMessage(
          sanitizeOAuthErrorMessage(formatApiError(cause), [], t('error')),
        )
      }
    } finally {
      if (claimed && flowGenerationRef.current === flowGeneration) {
        busyRef.current = false
        setBusy(false)
      }
    }
  }

  async function completeBrowser(): Promise<void> {
    if (busyRef.current) return
    const snapshot = captureRef.current()
    const flow = browserRef.current
    const capturedRedirectUrl = redirectUrlRef.current.trim()
    const flowGeneration = flowGenerationRef.current
    if (
      !flow ||
      !capturedRedirectUrl ||
      flowEndpointIdRef.current !== endpointId ||
      !isCurrentRef.current(snapshot)
    ) {
      return
    }
    let claimed = false

    try {
      const outcome = await runActionRef.current(
        snapshot.endpointId,
        async () => {
          claimed = true
          busyRef.current = true
          setBusy(true)
          setMessage('')
          const result = await completeEndpointBrowserLogin(
            snapshot.endpointId,
            flow.flow_id,
            capturedRedirectUrl,
          )
          if (
            result.status === 'complete' &&
            currentFlowIsValid(snapshot, flowGeneration, flow.flow_id)
          ) {
            await refreshRef.current()
          }
          return result
        },
        'oauth-browser-complete',
      )
      if (
        !outcome.executed ||
        !currentFlowIsValid(snapshot, flowGeneration, flow.flow_id)
      ) {
        return
      }
      if (outcome.value.status === 'complete') {
        clearFlow()
        setBrowser(null)
        setRedirectUrl('')
        setBusy(false)
        setMessage('')
      }
    } catch (cause) {
      if (
        claimed &&
        currentFlowIsValid(snapshot, flowGeneration, flow.flow_id)
      ) {
        setMessage(
          sanitizeOAuthErrorMessage(
            formatApiError(cause),
            [capturedRedirectUrl],
            t('error'),
          ),
        )
      }
    } finally {
      if (claimed && flowGenerationRef.current === flowGeneration) {
        busyRef.current = false
        setBusy(false)
      }
    }
  }

  async function copy(value: string): Promise<void> {
    try {
      await copyText(value)
    } catch {
      // The value remains selectable when clipboard access is unavailable.
    }
  }

  return (
    <section
      aria-label={t('endpointOAuthBrowserLogin')}
      className="grid gap-2 border-t pt-3"
    >
      <div className="flex items-center justify-between gap-2">
        <span className="text-sm font-medium">
          {t('endpointOAuthBrowserLogin')}
        </span>
        <Button
          disabled={actionPending || busy}
          onClick={() => void startBrowser()}
          size="sm"
          type="button"
          variant="outline"
        >
          {busy && !browser ? (
            <LoaderCircle aria-hidden="true" className="animate-spin" />
          ) : null}
          {t('endpointOAuthBrowserStart')}
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        {t('endpointOAuthBrowserHint')}
      </p>
      {browser ? (
        <div className="grid min-w-0 gap-3">
          <div className="grid min-w-0 gap-1">
            <label
              className="text-xs text-muted-foreground"
              htmlFor="endpoint-oauth-authorize"
            >
              {t('endpointOAuthAuthorizeUrl')}
            </label>
            <div className="flex min-w-0 items-center gap-2">
              <Input
                aria-label={t('endpointOAuthAuthorizeUrl')}
                autoComplete="off"
                className="min-w-0"
                id="endpoint-oauth-authorize"
                readOnly
                value={browser.authorize_url}
              />
              <Button
                aria-label={t('endpointOAuthCopy')}
                disabled={actionPending}
                onClick={() => void copy(browser.authorize_url)}
                size="icon-sm"
                type="button"
                variant="ghost"
              >
                <Copy aria-hidden="true" />
              </Button>
            </div>
          </div>
          <div className="grid min-w-0 gap-1">
            <label
              className="text-xs text-muted-foreground"
              htmlFor="endpoint-oauth-redirect"
            >
              {t('endpointOAuthRedirectUrl')}
            </label>
            <Input
              autoComplete="off"
              disabled={actionPending || busy}
              id="endpoint-oauth-redirect"
              onChange={(event) => updateRedirectUrl(event.currentTarget.value)}
              placeholder={t('endpointOAuthRedirectPlaceholder')}
              type="password"
              value={redirectUrl}
            />
          </div>
          <Button
            className="justify-self-end"
            disabled={actionPending || busy || redirectUrl.trim() === ''}
            onClick={() => void completeBrowser()}
            type="button"
          >
            {busy && redirectUrl.trim() !== '' ? (
              <LoaderCircle aria-hidden="true" className="animate-spin" />
            ) : null}
            {t('endpointOAuthComplete')}
          </Button>
        </div>
      ) : null}
      {message ? (
        <p className="break-words text-sm text-destructive">{message}</p>
      ) : null}
    </section>
  )
}
