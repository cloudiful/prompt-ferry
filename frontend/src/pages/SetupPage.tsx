import { useState } from 'react'
import { useForm } from '@tanstack/react-form'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { Navigate, useLocation } from 'react-router'
import { Eye, EyeOff, UserPlus } from 'lucide-react'
import { Alert, AlertDescription } from '@/components/ui/alert'
import { Button } from '@/components/ui/button'
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
import { LocaleToggle, ThemeToggle } from '@/components/auth/DisplayControls'
import {
  setupStatusOptions,
  setupStatusQueryKey,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { formatApiError } from '@/api'
import { LOGIN_NAME_STORAGE_KEY, readStorage, writeStorage } from '@/storage'
import { defaultNavPath } from '@/lib/nav'
import { useAsyncAction } from '@/lib/use-async-action'
import { useLocale } from '@/lib/i18n'
import { usePageTitle } from '@/lib/router'
import { SESSION_AUTH_KEY, useSessionStore } from '@/lib/session'

function initialLoginName(): string {
  return readStorage(LOGIN_NAME_STORAGE_KEY) ?? 'admin'
}

export function SetupPage() {
  const { t } = useLocale()
  const location = useLocation()
  const queryClient = useQueryClient()
  const me = useSessionStore((state) => state.me)
  const setupError = useSessionStore((state) => state.setupError)
  const setup = useSessionStore((state) => state.setup)
  const { isPending, pendingOrigin } = useAsyncAction()
  const [showPassword, setShowPassword] = useState(false)
  const [showMasterSecret, setShowMasterSecret] = useState(false)
  const busy = isPending(SESSION_AUTH_KEY)
  const setupQuery = useQuery({
    ...setupStatusOptions(),
    enabled: me === null,
  })

  usePageTitle('setupTitle')

  const form = useForm({
    defaultValues: {
      master_secret: '',
      login_name: initialLoginName(),
      display_name: '',
      password: '',
    },
    onSubmit: async ({ value }) => {
      const loginName = value.login_name.trim()
      const displayName = value.display_name.trim()
      try {
        const outcome = await setup(
          {
            master_secret: value.master_secret,
            login_name: loginName,
            password: value.password,
            display_name: displayName || null,
          },
          async (status) => {
            if (status) {
              queryClient.setQueryData(setupStatusQueryKey(), status)
            }
            await queryClient.invalidateQueries({
              queryKey: setupStatusQueryKey(),
            })
          },
        )
        if (!outcome.executed) return
        writeStorage(LOGIN_NAME_STORAGE_KEY, loginName)
      } catch {
        // Keep the draft for the retry, but never keep the proof.
        form.setFieldValue('master_secret', '')
        return
      }
    },
  })

  if (me) {
    return <Navigate replace to={defaultNavPath(me.is_admin)} />
  }

  const status = setupQuery.data

  return (
    <form.Subscribe selector={(state) => state.isSubmitting}>
      {(isSubmitting) => {
        // A pending shared action or the form's own submission keeps the setup
        // route mounted, so a cached initialized status cannot unmount it
        // before the guarded action and its mandatory refresh settle.
        if (!busy && !isSubmitting) {
          if (setupQuery.isPending) {
            return (
              <main
                aria-label={t('loading')}
                className="grid min-h-screen place-items-center p-4"
                data-foundation-route-id="R-SETUP"
                role="status"
              >
                <Skeleton className="h-8 w-40" />
              </main>
            )
          }

          if (status?.initialized) {
            return <Navigate replace to={`/login${location.search}`} />
          }

          if (setupQuery.isError) {
            return (
              <SetupStatusCard
                description={t('setupStatusFailedDesc')}
                detail={formatApiError(setupQuery.error)}
                onRetry={() => void setupQuery.refetch()}
                title={t('setupStatusFailed')}
              />
            )
          }

          if (status && !status.setup_available) {
            return (
              <SetupStatusCard
                description={t('setupUnavailableDesc')}
                onRetry={() => void setupQuery.refetch()}
                title={t('setupUnavailable')}
              />
            )
          }
        }

        return (
          <main
            className="grid min-h-screen place-items-center p-4 sm:p-8"
            data-foundation-route-id="R-SETUP"
          >
            <Card className="w-full max-w-md">
              <CardHeader>
                <CardTitle>{t('setupTitle')}</CardTitle>
              </CardHeader>
              <CardContent>
                <form
                  className="grid gap-4"
                  noValidate
                  onSubmit={(event) => {
                    event.preventDefault()
                    event.stopPropagation()
                    void form.handleSubmit()
                  }}
                >
                  <p className="text-sm text-muted-foreground">
                    {t('setupIntro')}
                  </p>
                  <Alert>
                    <AlertDescription>
                      {t('setupTransportWarning')}
                    </AlertDescription>
                  </Alert>

                  <div className="grid gap-2">
                    <Label htmlFor="setup-master-secret">
                      {t('setupMasterSecret')}
                    </Label>
                    <div className="flex gap-2">
                      <form.Field
                        name="master_secret"
                        validators={{
                          onChange: ({ value }) =>
                            value ? undefined : t('setupMasterSecretRequired'),
                        }}
                      >
                        {(field) => (
                          <>
                            <Input
                              autoComplete="off"
                              className="min-w-0 flex-1"
                              disabled={busy}
                              id="setup-master-secret"
                              onBlur={field.handleBlur}
                              onChange={(event) =>
                                field.handleChange(event.currentTarget.value)
                              }
                              type={showMasterSecret ? 'text' : 'password'}
                              value={field.state.value}
                            />
                            <form.Subscribe
                              selector={(state) => state.submissionAttempts}
                            >
                              {(attempts) => (
                                <FieldError
                                  attempts={attempts}
                                  errors={field.state.meta.errors}
                                />
                              )}
                            </form.Subscribe>
                          </>
                        )}
                      </form.Field>
                      <Button
                        aria-label={t(
                          showMasterSecret ? 'hidePassword' : 'showPassword',
                        )}
                        disabled={busy}
                        onClick={() =>
                          setShowMasterSecret((visible) => !visible)
                        }
                        size="icon"
                        type="button"
                        variant="outline"
                      >
                        {showMasterSecret ? (
                          <EyeOff aria-hidden="true" />
                        ) : (
                          <Eye aria-hidden="true" />
                        )}
                      </Button>
                    </div>
                  </div>

                  <div className="grid gap-2">
                    <Label htmlFor="setup-login-name">{t('loginName')}</Label>
                    <form.Field
                      name="login_name"
                      validators={{
                        onChange: ({ value }) =>
                          value.trim() ? undefined : t('loginNameRequired'),
                      }}
                    >
                      {(field) => (
                        <>
                          <Input
                            autoComplete="username"
                            disabled={busy}
                            id="setup-login-name"
                            onBlur={field.handleBlur}
                            onChange={(event) =>
                              field.handleChange(event.currentTarget.value)
                            }
                            value={field.state.value}
                          />
                          <form.Subscribe
                            selector={(state) => state.submissionAttempts}
                          >
                            {(attempts) => (
                              <FieldError
                                attempts={attempts}
                                errors={field.state.meta.errors}
                              />
                            )}
                          </form.Subscribe>
                        </>
                      )}
                    </form.Field>
                  </div>

                  <div className="grid gap-2">
                    <Label htmlFor="setup-display-name">
                      {t('displayName')}
                    </Label>
                    <form.Field name="display_name">
                      {(field) => (
                        <>
                          <Input
                            aria-describedby="setup-display-name-help"
                            autoComplete="name"
                            disabled={busy}
                            id="setup-display-name"
                            onBlur={field.handleBlur}
                            onChange={(event) =>
                              field.handleChange(event.currentTarget.value)
                            }
                            value={field.state.value}
                          />
                          <p
                            className="text-sm text-muted-foreground"
                            id="setup-display-name-help"
                          >
                            {t('setupDisplayNameHelp')}
                          </p>
                        </>
                      )}
                    </form.Field>
                  </div>

                  <div className="grid gap-2">
                    <Label htmlFor="setup-password">{t('password')}</Label>
                    <div className="flex gap-2">
                      <form.Field
                        name="password"
                        validators={{
                          onChange: ({ value }) =>
                            value ? undefined : t('passwordRequired'),
                        }}
                      >
                        {(field) => (
                          <>
                            <Input
                              autoComplete="new-password"
                              className="min-w-0 flex-1"
                              disabled={busy}
                              id="setup-password"
                              onBlur={field.handleBlur}
                              onChange={(event) =>
                                field.handleChange(event.currentTarget.value)
                              }
                              type={showPassword ? 'text' : 'password'}
                              value={field.state.value}
                            />
                            <form.Subscribe
                              selector={(state) => state.submissionAttempts}
                            >
                              {(attempts) => (
                                <FieldError
                                  attempts={attempts}
                                  errors={field.state.meta.errors}
                                />
                              )}
                            </form.Subscribe>
                          </>
                        )}
                      </form.Field>
                      <Button
                        aria-label={t(
                          showPassword ? 'hidePassword' : 'showPassword',
                        )}
                        disabled={busy}
                        onClick={() => setShowPassword((visible) => !visible)}
                        size="icon"
                        type="button"
                        variant="outline"
                      >
                        {showPassword ? (
                          <EyeOff aria-hidden="true" />
                        ) : (
                          <Eye aria-hidden="true" />
                        )}
                      </Button>
                    </div>
                  </div>

                  <p className="text-sm text-muted-foreground">
                    {t('setupSharedDatabaseNote')}
                  </p>

                  {setupError && (
                    <Alert variant="destructive">
                      <AlertDescription>{setupError}</AlertDescription>
                    </Alert>
                  )}

                  <Button className="w-full" disabled={busy} type="submit">
                    {pendingOrigin(SESSION_AUTH_KEY) === 'setup' ? (
                      <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
                    ) : (
                      <UserPlus aria-hidden="true" />
                    )}
                    <span>{t('setupSubmit')}</span>
                  </Button>
                </form>
              </CardContent>
              <CardFooter className="justify-center gap-2">
                <LocaleToggle />
                <ThemeToggle />
              </CardFooter>
            </Card>
          </main>
        )
      }}
    </form.Subscribe>
  )
}

type SetupStatusCardProps = {
  title: string
  description: string
  detail?: string
  onRetry: () => void
}

function SetupStatusCard({
  title,
  description,
  detail,
  onRetry,
}: SetupStatusCardProps) {
  const { t } = useLocale()
  return (
    <main
      className="grid min-h-screen place-items-center p-4 sm:p-8"
      data-foundation-route-id="R-SETUP"
    >
      <Card className="w-full max-w-md">
        <CardHeader>
          <CardTitle>{title}</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-3">
          <p className="text-sm text-muted-foreground">{description}</p>
          {detail && (
            <Alert variant="destructive">
              <AlertDescription>{detail}</AlertDescription>
            </Alert>
          )}
          <Button onClick={onRetry} type="button" variant="outline">
            {t('retry')}
          </Button>
        </CardContent>
        <CardFooter className="justify-center gap-2">
          <LocaleToggle />
          <ThemeToggle />
        </CardFooter>
      </Card>
    </main>
  )
}

type FieldErrorProps = {
  attempts: number
  errors: Array<unknown>
}

function FieldError({ attempts, errors }: FieldErrorProps) {
  if (attempts === 0 || errors.length === 0) return null
  return (
    <p className="text-sm text-destructive" role="alert">
      {String(errors[0])}
    </p>
  )
}
