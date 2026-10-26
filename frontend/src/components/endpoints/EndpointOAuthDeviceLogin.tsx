import { Copy, ExternalLink, LoaderCircle } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import type { OAuthDeviceStartResponse } from '@/generated/admin-api'
import { formatApiError } from '@/api'
import {
  pollEndpointDeviceLogin,
  startEndpointDeviceLogin,
} from '@/api/endpoint-oauth'
import { Button } from '@/components/ui/button'
import { useLocale } from '@/lib/i18n'
import { copyText } from '@/lib/clipboard'
import {
  sanitizeOAuthErrorMessage,
  type OAuthStatusSnapshot,
  type RunOAuthAction,
} from '@/models/endpoints/oauth'

type EndpointOAuthDeviceLoginProps = {
  actionPending: boolean
  captureStatusSnapshot: () => OAuthStatusSnapshot
  dialogGeneration: number
  endpointId: string
  isSnapshotCurrent: (snapshot: OAuthStatusSnapshot) => boolean
  refreshStatus: () => Promise<void>
  runAction: RunOAuthAction
}

function safeVerificationUrl(value: string): string | null {
  try {
    const url = new URL(value)
    return url.protocol === 'http:' || url.protocol === 'https:'
      ? url.toString()
      : null
  } catch {
    return null
  }
}

export function EndpointOAuthDeviceLogin({
  actionPending,
  captureStatusSnapshot,
  dialogGeneration,
  endpointId,
  isSnapshotCurrent,
  refreshStatus,
  runAction,
}: EndpointOAuthDeviceLoginProps) {
  const { t } = useLocale()
  const [device, setDevice] = useState<OAuthDeviceStartResponse | null>(null)
  const [starting, setStarting] = useState(false)
  const [polling, setPolling] = useState(false)
  const [message, setMessage] = useState('')
  const deviceRef = useRef<OAuthDeviceStartResponse | null>(null)
  const startingRef = useRef(false)
  const pollingRef = useRef(false)
  const flowGenerationRef = useRef(0)
  const flowEndpointIdRef = useRef('')
  const pollTimerRef = useRef<number | null>(null)
  const captureRef = useRef(captureStatusSnapshot)
  const isCurrentRef = useRef(isSnapshotCurrent)
  const refreshRef = useRef(refreshStatus)
  const runActionRef = useRef(runAction)

  captureRef.current = captureStatusSnapshot
  isCurrentRef.current = isSnapshotCurrent
  refreshRef.current = refreshStatus
  runActionRef.current = runAction

  function stopPolling(): void {
    if (pollTimerRef.current !== null) {
      window.clearInterval(pollTimerRef.current)
      pollTimerRef.current = null
    }
  }

  function releaseFlow(): void {
    flowGenerationRef.current += 1
    stopPolling()
    flowEndpointIdRef.current = ''
    deviceRef.current = null
    startingRef.current = false
    pollingRef.current = false
  }

  useEffect(() => {
    releaseFlow()
    setDevice(null)
    setStarting(false)
    setPolling(false)
    setMessage('')
    return () => releaseFlow()
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
      (flowId === undefined || deviceRef.current?.flow_id === flowId)
    )
  }

  async function pollDevice(
    snapshot: OAuthStatusSnapshot,
    flowGeneration: number,
    flowId: string,
  ): Promise<void> {
    if (
      pollingRef.current ||
      !currentFlowIsValid(snapshot, flowGeneration, flowId)
    ) {
      return
    }

    let claimed = false
    try {
      const outcome = await runActionRef.current(
        snapshot.endpointId,
        async () => {
          claimed = true
          pollingRef.current = true
          setPolling(true)
          const result = await pollEndpointDeviceLogin(
            snapshot.endpointId,
            flowId,
          )
          if (
            result.status === 'complete' &&
            currentFlowIsValid(snapshot, flowGeneration, flowId)
          ) {
            await refreshRef.current()
          }
          return result
        },
        'oauth-device-poll',
      )
      if (
        !outcome.executed ||
        !currentFlowIsValid(snapshot, flowGeneration, flowId)
      ) {
        return
      }
      if (outcome.value.status === 'complete') {
        stopPolling()
        flowEndpointIdRef.current = ''
        deviceRef.current = null
        setDevice(null)
        setMessage('')
      }
    } catch (cause) {
      if (claimed && currentFlowIsValid(snapshot, flowGeneration, flowId)) {
        stopPolling()
        flowEndpointIdRef.current = ''
        deviceRef.current = null
        setDevice(null)
        setMessage(
          sanitizeOAuthErrorMessage(formatApiError(cause), [], t('error')),
        )
      }
    } finally {
      if (claimed && flowGenerationRef.current === flowGeneration) {
        pollingRef.current = false
        setPolling(false)
      }
    }
  }

  async function startDevice(): Promise<void> {
    if (startingRef.current) return
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
          stopPolling()
          deviceRef.current = null
          setDevice(null)
          flowEndpointIdRef.current = snapshot.endpointId
          startingRef.current = true
          setStarting(true)
          setMessage('')
          return startEndpointDeviceLogin(snapshot.endpointId)
        },
        'oauth-device-start',
      )
      if (!outcome.executed || !currentFlowIsValid(snapshot, flowGeneration)) {
        return
      }
      const started = outcome.value
      deviceRef.current = started
      setDevice(started)
      const intervalSeconds = Math.max(1, started.interval_seconds)
      pollTimerRef.current = window.setInterval(() => {
        void pollDevice(snapshot, flowGeneration, started.flow_id)
      }, intervalSeconds * 1000)
    } catch (cause) {
      if (claimed && currentFlowIsValid(snapshot, flowGeneration)) {
        setMessage(
          sanitizeOAuthErrorMessage(formatApiError(cause), [], t('error')),
        )
      }
    } finally {
      if (claimed && flowGenerationRef.current === flowGeneration) {
        startingRef.current = false
        setStarting(false)
      }
    }
  }

  const verificationUrl = device
    ? safeVerificationUrl(device.verification_uri)
    : null

  async function copy(value: string): Promise<void> {
    try {
      await copyText(value)
    } catch {
      // The value remains selectable when clipboard access is unavailable.
    }
  }

  return (
    <section aria-label={t('endpointOAuthDeviceLogin')} className="grid gap-2">
      <div className="flex items-center justify-between gap-2">
        <span className="text-sm font-medium">
          {t('endpointOAuthDeviceLogin')}
        </span>
        <Button
          disabled={actionPending || starting || polling}
          onClick={() => void startDevice()}
          size="sm"
          type="button"
          variant="outline"
        >
          {starting ? (
            <LoaderCircle aria-hidden="true" className="animate-spin" />
          ) : null}
          {t('endpointOAuthDeviceStart')}
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        {t('endpointOAuthDeviceHint')}
      </p>
      {device ? (
        <div className="grid min-w-0 gap-2">
          <div className="flex items-center gap-2">
            <code className="rounded bg-muted px-2 py-1 font-mono text-sm">
              {device.user_code}
            </code>
            <Button
              aria-label={t('endpointOAuthCopy')}
              disabled={actionPending}
              onClick={() => void copy(device.user_code)}
              size="icon-xs"
              type="button"
              variant="ghost"
            >
              <Copy aria-hidden="true" />
            </Button>
          </div>
          <p
            aria-busy={polling}
            className="flex flex-wrap items-center gap-1 text-xs text-muted-foreground"
          >
            {polling ? (
              <LoaderCircle
                aria-hidden="true"
                className="size-3 animate-spin"
              />
            ) : null}
            <span>{t('endpointOAuthDeviceWaiting')}</span>
            {verificationUrl ? (
              <a href={verificationUrl} rel="noreferrer" target="_blank">
                <ExternalLink
                  aria-hidden="true"
                  className="mr-1 inline size-3"
                />
                {verificationUrl}
              </a>
            ) : (
              <span className="break-all">{device.verification_uri}</span>
            )}
          </p>
        </div>
      ) : null}
      {message ? (
        <p className="break-words text-sm text-destructive">{message}</p>
      ) : null}
    </section>
  )
}
