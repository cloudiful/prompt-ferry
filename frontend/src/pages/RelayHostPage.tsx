import { useEffect, useRef, useState } from 'react'
import { useForm } from '@tanstack/react-form'
import { Eye, EyeOff, RefreshCw } from 'lucide-react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import type {
  HostRole,
  RelayHostResponse,
  RelaySettingsResponse,
  RelayStatusResponse,
} from '@/generated/admin-api'
import { formatApiError } from '@/api'
import {
  relayGetSettingsOptions,
  relayGetSettingsQueryKey,
  relayHostOptions,
  relayHostQueryKey,
  relayRequestRestartMutation,
  relaySetHostRoleMutation,
  relaySetSettingsMutation,
  relayStatusOptions,
  relayStatusQueryKey,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import {
  createEmptyRelayHostForm,
  createRelayHostRoleRequest,
  createRelaySettingsUpdate,
  hasHostRoleChange,
  relayHostToForm,
  type RelayHostForm,
} from '@/admin-mappers/forms/relay-host'
import {
  canManageWorker,
  isHostRestartRequired,
  isRelayReady,
} from '@/models/endpoints/capability'
import { Alert, AlertDescription, AlertTitle } from '@/components/ui/alert'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Skeleton } from '@/components/ui/skeleton'
import { useAsyncAction } from '@/lib/use-async-action'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { usePageTitle } from '@/lib/router'
import { RELAY_SESSION_AUTH_KEY, useRelaySessionStore } from '@/lib/session'

const HOST_ACTION_KEY = 'host:local'

const roles: Array<{
  role: HostRole
  labelKey: 'relayRoleIntegrated' | 'relayRoleWorker' | 'relayRoleRelay'
  descriptionKey:
    'relayRoleIntegratedDesc' | 'relayRoleWorkerDesc' | 'relayRoleRelayDesc'
}> = [
  {
    role: 'integrated',
    labelKey: 'relayRoleIntegrated',
    descriptionKey: 'relayRoleIntegratedDesc',
  },
  {
    role: 'worker',
    labelKey: 'relayRoleWorker',
    descriptionKey: 'relayRoleWorkerDesc',
  },
  {
    role: 'relay',
    labelKey: 'relayRoleRelay',
    descriptionKey: 'relayRoleRelayDesc',
  },
]

type RelayLoginForm = { admin_token: string }

type RefreshResult = {
  host: RelayHostResponse
  status: RelayStatusResponse
  settings: RelaySettingsResponse
}

export function RelayHostPage() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const { notifySuccess } = useNotifier()
  const authenticated = useRelaySessionStore((state) => state.authenticated)
  const bootstrapped = useRelaySessionStore((state) => state.bootstrapped)
  const bootstrapping = useRelaySessionStore((state) => state.bootstrapping)
  const authError = useRelaySessionStore((state) => state.error)
  const bootstrap = useRelaySessionStore((state) => state.bootstrap)
  const login = useRelaySessionStore((state) => state.login)
  const logout = useRelaySessionStore((state) => state.logout)
  const [showToken, setShowToken] = useState(false)
  const [actionError, setActionError] = useState('')
  const [refreshing, setRefreshing] = useState(false)
  const bindEditGeneration = useRef(0)
  const formInitialized = useRef(false)
  const hostAction = asyncAction.pendingOrigin(HOST_ACTION_KEY)
  const authBusy = asyncAction.isPending(RELAY_SESSION_AUTH_KEY)

  usePageTitle('relayHost')

  const tokenForm = useForm({
    defaultValues: { admin_token: '' },
    onSubmit: async ({ value }) => {
      await onLogin(value)
    },
  })

  const hostForm = useForm({
    defaultValues: createEmptyRelayHostForm(),
    onSubmit: async ({ value }) => {
      await saveSettings(value)
    },
  })

  useEffect(() => {
    if (!bootstrapped) void bootstrap()
  }, [bootstrapped, bootstrap])

  const hostQuery = useQuery({
    ...relayHostOptions(),
    enabled: authenticated,
    refetchOnWindowFocus: false,
  })
  const statusQuery = useQuery({
    ...relayStatusOptions(),
    enabled: authenticated,
    refetchOnWindowFocus: false,
  })
  const settingsQuery = useQuery({
    ...relayGetSettingsOptions(),
    enabled: authenticated,
    refetchOnWindowFocus: false,
  })

  const setHostRole = useMutation(relaySetHostRoleMutation())
  const setSettings = useMutation(relaySetSettingsMutation())
  const requestRestartMutation = useMutation(relayRequestRestartMutation())

  useEffect(() => {
    if (!authenticated) {
      formInitialized.current = false
      return
    }
    if (!formInitialized.current && hostQuery.data && settingsQuery.data) {
      const initial = relayHostToForm(hostQuery.data, settingsQuery.data)
      hostForm.reset(initial)
      formInitialized.current = true
    }
  }, [authenticated, hostQuery.data, settingsQuery.data])

  function syncFormAfterWrite(
    confirmed: RelayHostForm,
    submitted: RelayHostForm,
    submittedBindGeneration: number,
  ): void {
    const current = hostForm.state.values
    hostForm.setFieldValue(
      'role',
      current.role === submitted.role ? confirmed.role : current.role,
    )
    if (bindEditGeneration.current === submittedBindGeneration) {
      hostForm.setFieldValue('admin_bind', confirmed.admin_bind)
    }
  }

  function fetchRefreshResult(): Promise<RefreshResult> {
    return Promise.all([
      queryClient.fetchQuery({ ...relayHostOptions(), staleTime: 0 }),
      queryClient.fetchQuery({ ...relayStatusOptions(), staleTime: 0 }),
      queryClient.fetchQuery({ ...relayGetSettingsOptions(), staleTime: 0 }),
    ]).then(([host, status, settings]) => ({ host, status, settings }))
  }

  async function onLogin(form: RelayLoginForm): Promise<void> {
    const capturedToken = form.admin_token.trim()
    try {
      const outcome = await login(capturedToken, async () => {
        await fetchRefreshResult()
      })
      if (!outcome.executed) return
      if (tokenForm.state.values.admin_token.trim() === capturedToken) {
        tokenForm.reset({ admin_token: '' })
      }
      notifySuccess(t('saved'))
    } catch {
      return
    }
  }

  async function refreshHost(): Promise<void> {
    setRefreshing(true)
    setActionError('')
    try {
      const refreshed = await fetchRefreshResult()
      formInitialized.current = true
      hostForm.reset(relayHostToForm(refreshed.host, refreshed.settings))
    } catch (cause) {
      setActionError(formatApiError(cause))
    } finally {
      setRefreshing(false)
    }
  }

  async function saveRole(role: HostRole): Promise<void> {
    const host = hostQuery.data
    const submitted = { ...hostForm.state.values }
    if (!host || !hasHostRoleChange({ ...submitted, role }, host)) {
      return
    }

    const submittedBindGeneration = bindEditGeneration.current
    try {
      const outcome = await asyncAction.run(
        HOST_ACTION_KEY,
        async () => {
          setActionError('')
          const saved = await setHostRole.mutateAsync({
            body: createRelayHostRoleRequest({ ...submitted, role }),
          })
          queryClient.setQueryData(relayHostQueryKey(), saved)
          return saved
        },
        `role:${role}`,
      )
      if (!outcome.executed) return
      syncFormAfterWrite(
        relayHostToForm(outcome.value, settingsQuery.data),
        submitted,
        submittedBindGeneration,
      )
      notifySuccess(t('relayRoleSaved'))
    } catch (cause) {
      setActionError(formatApiError(cause))
    }
  }

  async function saveSettings(submitted: RelayHostForm): Promise<void> {
    if (!settingsQuery.data) return
    const submittedBindGeneration = bindEditGeneration.current
    const body = createRelaySettingsUpdate(submitted)
    try {
      const outcome = await asyncAction.run(
        HOST_ACTION_KEY,
        async () => {
          setActionError('')
          const saved = await setSettings.mutateAsync({ body })
          queryClient.setQueryData(relayGetSettingsQueryKey(), saved)
          const [host, status] = await Promise.all([
            queryClient.fetchQuery({ ...relayHostOptions(), staleTime: 0 }),
            queryClient.fetchQuery({ ...relayStatusOptions(), staleTime: 0 }),
          ])
          queryClient.setQueryData(relayHostQueryKey(), host)
          queryClient.setQueryData(relayStatusQueryKey(), status)
          return { host, saved, status }
        },
        'settings',
      )
      if (!outcome.executed) return
      syncFormAfterWrite(
        relayHostToForm(outcome.value.host, outcome.value.saved),
        submitted,
        submittedBindGeneration,
      )
      notifySuccess(t('saved'))
    } catch (cause) {
      setActionError(formatApiError(cause))
    }
  }

  async function requestRestart(origin: 'banner' | 'settings'): Promise<void> {
    try {
      const outcome = await asyncAction.run(
        HOST_ACTION_KEY,
        async () => {
          setActionError('')
          await requestRestartMutation.mutateAsync({})
        },
        `restart:${origin}`,
      )
      if (outcome.executed) notifySuccess(t('relayRestartRequested'))
    } catch (cause) {
      setActionError(formatApiError(cause))
    }
  }

  async function handleLogout(): Promise<void> {
    if (hostAction !== null) return
    try {
      const outcome = await logout()
      if (!outcome.executed) return
      await Promise.all([
        queryClient.removeQueries({
          queryKey: relayHostQueryKey(),
          exact: true,
        }),
        queryClient.removeQueries({
          queryKey: relayStatusQueryKey(),
          exact: true,
        }),
        queryClient.removeQueries({
          queryKey: relayGetSettingsQueryKey(),
          exact: true,
        }),
      ])
      formInitialized.current = false
      bindEditGeneration.current += 1
      hostForm.reset(createEmptyRelayHostForm())
      tokenForm.reset({ admin_token: '' })
      setActionError('')
      setShowToken(false)
    } catch (cause) {
      setActionError(formatApiError(cause))
    }
  }

  if (!bootstrapped || bootstrapping) {
    return (
      <div
        aria-label={t('loading')}
        className="grid gap-3"
        data-foundation-route-id="R-RELAY-HOST"
        role="status"
      >
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-28 w-full" />
      </div>
    )
  }

  if (!authenticated) {
    return (
      <Card
        className="mx-auto w-full max-w-lg"
        data-foundation-route-id="R-RELAY-HOST"
      >
        <CardHeader>
          <CardTitle>{t('relayHostLogin')}</CardTitle>
        </CardHeader>
        <CardContent>
          <form
            className="grid gap-4"
            noValidate
            onSubmit={(event) => {
              event.preventDefault()
              event.stopPropagation()
              void tokenForm.handleSubmit()
            }}
          >
            <div className="grid gap-2">
              <Label htmlFor="relay-admin-token">
                {t('relayHostTokenPlaceholder')}
              </Label>
              <div className="flex gap-2">
                <tokenForm.Field name="admin_token">
                  {(field) => (
                    <Input
                      autoComplete="off"
                      className="min-w-0 flex-1"
                      disabled={authBusy}
                      id="relay-admin-token"
                      onBlur={field.handleBlur}
                      onChange={(event) =>
                        field.handleChange(event.currentTarget.value)
                      }
                      type={showToken ? 'text' : 'password'}
                      value={field.state.value}
                    />
                  )}
                </tokenForm.Field>
                <Button
                  aria-label={t(showToken ? 'hidePassword' : 'showPassword')}
                  disabled={authBusy}
                  onClick={() => setShowToken((visible) => !visible)}
                  size="icon"
                  type="button"
                  variant="outline"
                >
                  {showToken ? (
                    <EyeOff aria-hidden="true" />
                  ) : (
                    <Eye aria-hidden="true" />
                  )}
                </Button>
              </div>
            </div>
            {authError && (
              <Alert variant="destructive">
                <AlertDescription>{authError}</AlertDescription>
              </Alert>
            )}
            <tokenForm.Subscribe
              selector={(state) => state.values.admin_token.trim()}
            >
              {(token) => (
                <Button disabled={authBusy || !token} type="submit">
                  {asyncAction.pendingOrigin(RELAY_SESSION_AUTH_KEY) ===
                    'login' && (
                    <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
                  )}
                  <span>{t('login')}</span>
                </Button>
              )}
            </tokenForm.Subscribe>
          </form>
        </CardContent>
      </Card>
    )
  }

  if (hostQuery.isPending || statusQuery.isPending || settingsQuery.isPending) {
    return (
      <div
        aria-label={t('loading')}
        className="grid gap-3"
        data-foundation-route-id="R-RELAY-HOST"
        role="status"
      >
        <Skeleton className="h-8 w-48" />
        <Skeleton className="h-28 w-full" />
      </div>
    )
  }

  const host = hostQuery.data
  const status = statusQuery.data
  const settings = settingsQuery.data
  const restartRequired =
    isHostRestartRequired(host, status, settings) ||
    (host?.pending_role != null && host.pending_role !== host.role)
  const workerConnected = canManageWorker(status)
  const roleCardsDisabled = hostAction !== null || !host || !settings
  const settingsDisabled = hostAction !== null || !settings
  const queryError = hostQuery.error ?? statusQuery.error ?? settingsQuery.error
  const errorMessage =
    actionError || (queryError ? formatApiError(queryError) : '')

  return (
    <div className="grid min-w-0 gap-4" data-foundation-route-id="R-RELAY-HOST">
      <HeaderActions>
        <div className="flex items-center gap-2">
          {host && <Badge>{host.role}</Badge>}
          {status && (
            <Badge
              aria-label={`${t('relayHostWorker')}: ${status.worker.connected_workers}`}
              variant="secondary"
            >
              {status.worker.connected_workers}
            </Badge>
          )}
        </div>
        <div className="flex items-center gap-2">
          <Button
            disabled={refreshing}
            onClick={() => void refreshHost()}
            type="button"
            variant="outline"
          >
            {refreshing ? (
              <span className="size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
            ) : (
              <RefreshCw aria-hidden="true" />
            )}
            <span>{t('refresh')}</span>
          </Button>
          <Button
            disabled={authBusy || hostAction !== null}
            onClick={() => void handleLogout()}
            type="button"
            variant="ghost"
          >
            <span>{t('logout')}</span>
            {asyncAction.pendingOrigin(RELAY_SESSION_AUTH_KEY) === 'logout' && (
              <span className="size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
            )}
          </Button>
        </div>
      </HeaderActions>

      {errorMessage && (
        <Alert variant="destructive">
          <AlertDescription>{errorMessage}</AlertDescription>
        </Alert>
      )}

      {restartRequired && (
        <Alert>
          <AlertTitle>{t('relayRestartRequired')}</AlertTitle>
          <AlertDescription className="flex flex-wrap items-center justify-between gap-3">
            <span>{t('relayRestartRequiredDesc')}</span>
            <Button
              disabled={hostAction !== null}
              onClick={() => void requestRestart('banner')}
              type="button"
              variant="outline"
            >
              {hostAction === 'restart:banner' && (
                <span className="size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
              )}
              <span>{t('relayRestartNow')}</span>
            </Button>
          </AlertDescription>
        </Alert>
      )}

      {status && !workerConnected && (
        <Alert>
          <AlertTitle>{t('relayWorkerDisconnected')}</AlertTitle>
          <AlertDescription>
            {t('relayWorkerDisconnectedDesc')}
          </AlertDescription>
        </Alert>
      )}

      <section
        aria-labelledby="relay-host-roles"
        className="grid min-w-0 gap-3"
      >
        <h2 className="text-sm font-semibold" id="relay-host-roles">
          {t('relayHostRoleTitle')}
        </h2>
        <div className="grid gap-3 md:grid-cols-3">
          {roles.map(({ role, labelKey, descriptionKey }) => {
            const isCurrent = host?.role === role
            const isPending = host?.pending_role === role
            const isActionOrigin = hostAction === `role:${role}`
            return (
              <Card
                className={isCurrent ? 'ring-2 ring-primary' : undefined}
                key={role}
              >
                <CardHeader>
                  <div className="flex items-center justify-between gap-2">
                    <CardTitle className="text-base">{t(labelKey)}</CardTitle>
                    <div className="flex flex-wrap items-center gap-1">
                      {isCurrent && <Badge>{t('relayRoleCurrent')}</Badge>}
                      {isPending && (
                        <Badge variant="secondary">
                          {t('relayRolePending')}
                        </Badge>
                      )}
                    </div>
                  </div>
                </CardHeader>
                <CardContent className="grid gap-3">
                  <p className="min-h-10 text-sm text-muted-foreground">
                    {t(descriptionKey)}
                  </p>
                  <Button
                    disabled={roleCardsDisabled || isCurrent || isPending}
                    onClick={() => void saveRole(role)}
                    type="button"
                    variant={isCurrent ? 'secondary' : 'outline'}
                  >
                    {isActionOrigin && (
                      <span className="size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
                    )}
                    <span>
                      {isCurrent ? t('relayRoleSelected') : t('relayUseRole')}
                    </span>
                  </Button>
                </CardContent>
              </Card>
            )
          })}
        </div>
      </section>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">
            {t('relayHostSettingsTitle')}
          </CardTitle>
        </CardHeader>
        <form
          className="grid gap-0"
          noValidate
          onSubmit={(event) => {
            event.preventDefault()
            event.stopPropagation()
            void hostForm.handleSubmit()
          }}
        >
          <CardContent className="grid gap-2">
            <Label htmlFor="relay-host-admin-bind">
              {t('relayHostAdminBind')}
            </Label>
            <hostForm.Field name="admin_bind">
              {(field) => (
                <Input
                  aria-describedby="relay-host-admin-bind-help"
                  aria-label={t('relayHostAdminBind')}
                  autoComplete="off"
                  disabled={settingsDisabled}
                  id="relay-host-admin-bind"
                  onChange={(event) => {
                    bindEditGeneration.current += 1
                    field.handleChange(event.currentTarget.value)
                  }}
                  placeholder="127.0.0.1:8790"
                  value={field.state.value}
                />
              )}
            </hostForm.Field>
            <p
              className="text-sm text-muted-foreground"
              id="relay-host-admin-bind-help"
            >
              {t('relayHostAdminBindHelp')}
            </p>
          </CardContent>
          <CardFooter className="flex flex-wrap justify-end gap-2">
            <Button disabled={settingsDisabled} type="submit">
              {hostAction === 'settings' && (
                <span className="size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
              )}
              <span>{t('save')}</span>
            </Button>
            <Button
              disabled={settingsDisabled}
              onClick={() => void requestRestart('settings')}
              type="button"
              variant="outline"
            >
              {hostAction === 'restart:settings' && (
                <span className="size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
              )}
              <span>{t('relayRestartNow')}</span>
            </Button>
          </CardFooter>
        </form>
      </Card>

      {status && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">
              {t('relayHostStatusTitle')}
            </CardTitle>
          </CardHeader>
          <CardContent className="grid gap-1 text-sm">
            <span>
              {t('relayHostRelayReady')}:{' '}
              {t(
                isRelayReady(status)
                  ? 'relayStatusConnected'
                  : 'relayStatusDisconnected',
              )}
            </span>
            <span>
              {t('relayHostWorker')}:{' '}
              {t(
                workerConnected
                  ? 'relayStatusConnected'
                  : 'relayWorkerDisconnected',
              )}
            </span>
          </CardContent>
        </Card>
      )}
    </div>
  )
}
