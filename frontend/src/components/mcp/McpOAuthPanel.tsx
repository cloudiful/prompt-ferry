import { useQuery } from '@tanstack/react-query'
import {
  KeyRound,
  LoaderCircle,
  PlugZap,
  RefreshCw,
  Unplug,
} from 'lucide-react'
import type { McpServer } from '@/generated/admin-api'
import { Button } from '@/components/ui/button'
import { fetchMcpOAuthStatus, mcpOAuthStatusQueryKey } from '@/lib/mcp'
import { useLocale } from '@/lib/i18n'
import { useSessionStore } from '@/lib/session'

type McpOAuthPanelProps = {
  server: McpServer
  pending: boolean
  onConnect: (server: McpServer) => void
  onDisconnect: (server: McpServer) => void
}

const READ_SCOPE = 'read-mcp'

export function McpOAuthPanel({
  server,
  pending,
  onConnect,
  onDisconnect,
}: McpOAuthPanelProps) {
  const { t } = useLocale()
  const me = useSessionStore((state) => state.me)
  const ownedByCurrentUser =
    server.scope === 'user' && me != null && server.owner_user_id === me.user_id
  const queryEnabled = server.auth_mode === 'oauth' && server.server_id !== ''
  const statusQuery = useQuery({
    enabled: queryEnabled,
    queryKey: mcpOAuthStatusQueryKey(server.server_id),
    queryFn: () => fetchMcpOAuthStatus(server.server_id),
    retry: false,
    refetchOnWindowFocus: true,
    refetchInterval: (query) =>
      query.state.data?.status === 'authorizing' ? 3000 : false,
  })
  const status = statusQuery.isError ? undefined : statusQuery.data
  const reason = status?.reason?.trim() ?? ''
  const missing = status != null && reason !== ''
  const value = status == null ? null : missing ? 'disconnected' : status.status
  const isLoading =
    queryEnabled &&
    (statusQuery.isPending || (statusQuery.isError && statusQuery.isFetching))
  const isUnknown =
    queryEnabled &&
    (statusQuery.isError || (!statusQuery.isPending && status == null))
  const label = isLoading
    ? t('oauthStatusLoading')
    : isUnknown
      ? t('oauthStatusUnknown')
      : value === 'connected'
        ? t('oauthStatusConnected')
        : value === 'authorizing'
          ? t('oauthStatusAuthorizing')
          : value === 'reauth_required'
            ? t('oauthStatusReauthRequired')
            : t('oauthStatusDisconnected')
  const canConnect =
    queryEnabled &&
    ownedByCurrentUser &&
    !pending &&
    !missing &&
    !isLoading &&
    !isUnknown &&
    value !== 'authorizing'
  const canDisconnect =
    queryEnabled &&
    ownedByCurrentUser &&
    !pending &&
    !missing &&
    (statusQuery.isError || (status != null && value !== 'disconnected'))

  return (
    <section
      aria-label={t('authModeOAuth')}
      className="grid min-w-0 gap-2 rounded-md border p-3"
    >
      <div className="flex items-center gap-2">
        <KeyRound aria-hidden="true" className="size-4" />
        <span className="font-medium">{t('authModeOAuth')}</span>
        {queryEnabled ? (
          <span className="text-sm text-muted-foreground">{label}</span>
        ) : null}
        {statusQuery.isFetching ? (
          <LoaderCircle
            aria-label={t('loading')}
            className="size-4 animate-spin text-muted-foreground"
          />
        ) : null}
      </div>
      <p className="text-xs text-muted-foreground">{t('oauthHint')}</p>
      {!ownedByCurrentUser ? (
        <p className="text-xs text-muted-foreground">{t('oauthOwnerOnly')}</p>
      ) : null}
      {missing ? (
        <p className="text-xs text-destructive" role="alert">
          {t('oauthConfigMissing')}: {reason}
        </p>
      ) : null}
      {statusQuery.isError ? (
        <div className="grid gap-1" role="alert">
          <p className="text-xs text-destructive">
            {t('oauthStatusUnavailable')}
          </p>
          <Button
            className="justify-self-start"
            disabled={statusQuery.isFetching}
            onClick={() => statusQuery.refetch()}
            size="sm"
            type="button"
            variant="outline"
          >
            {statusQuery.isFetching ? (
              <LoaderCircle aria-hidden="true" className="animate-spin" />
            ) : (
              <RefreshCw aria-hidden="true" />
            )}
            {t('oauthRetry')}
          </Button>
        </div>
      ) : null}
      {status?.expires_at && !isUnknown ? (
        <p className="text-xs text-muted-foreground">
          {t('oauthExpires')}: {new Date(status.expires_at).toLocaleString()}
        </p>
      ) : null}
      <p className="text-xs text-muted-foreground">
        {t('oauthScopes')}: {READ_SCOPE}
      </p>
      <div className="flex flex-wrap gap-2">
        <Button
          disabled={!canConnect}
          onClick={() => onConnect(server)}
          size="sm"
          type="button"
          variant="outline"
        >
          <PlugZap aria-hidden="true" />
          {value === 'connected' || value === 'reauth_required'
            ? t('oauthReconnect')
            : t('oauthConnect')}
        </Button>
        <Button
          disabled={!canDisconnect}
          onClick={() => onDisconnect(server)}
          size="sm"
          type="button"
          variant="outline"
        >
          <Unplug aria-hidden="true" />
          {t('oauthDisconnect')}
        </Button>
      </div>
    </section>
  )
}
