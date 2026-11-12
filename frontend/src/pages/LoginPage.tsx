import { useEffect, useState } from 'react'
import { useForm } from '@tanstack/react-form'
import { useLocation, useNavigate } from 'react-router'
import { Eye, EyeOff, LogIn } from 'lucide-react'
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
import { LocaleToggle, ThemeToggle } from '@/components/auth/DisplayControls'
import { LOGIN_NAME_STORAGE_KEY, readStorage, writeStorage } from '@/storage'
import { defaultNavPath } from '@/lib/nav'
import { useAsyncAction } from '@/lib/use-async-action'
import { useLocale } from '@/lib/i18n'
import { usePageTitle } from '@/lib/router'
import { SESSION_AUTH_KEY, useSessionStore } from '@/lib/session'

function initialLoginName(): string {
  return readStorage(LOGIN_NAME_STORAGE_KEY) ?? 'admin'
}

function safeRedirectTarget(value: string | null): string | null {
  if (!value || !value.startsWith('/') || value.startsWith('//')) return null
  return value
}

export function LoginPage() {
  const { t } = useLocale()
  const location = useLocation()
  const navigate = useNavigate()
  const me = useSessionStore((state) => state.me)
  const bootstrapped = useSessionStore((state) => state.bootstrapped)
  const bootstrapping = useSessionStore((state) => state.bootstrapping)
  const error = useSessionStore((state) => state.error)
  const bootstrapAuth = useSessionStore((state) => state.bootstrapAuth)
  const login = useSessionStore((state) => state.login)
  const { isPending, pendingOrigin } = useAsyncAction()
  const [showPassword, setShowPassword] = useState(false)
  const busy = bootstrapping || isPending(SESSION_AUTH_KEY)
  const params = new URLSearchParams(location.search)
  const redirect = safeRedirectTarget(params.get('redirect'))

  usePageTitle('login')

  const form = useForm({
    defaultValues: { login_name: initialLoginName(), password: '' },
    onSubmit: async ({ value }) => {
      const loginName = value.login_name.trim()
      writeStorage(LOGIN_NAME_STORAGE_KEY, loginName)
      try {
        const outcome = await login({
          login_name: loginName,
          password: value.password,
        })
        if (outcome.executed) {
          const currentUser = useSessionStore.getState().me
          if (currentUser) {
            navigate(redirect ?? defaultNavPath(currentUser.is_admin), {
              replace: true,
            })
          }
        }
      } catch {
        return
      }
    },
  })

  useEffect(() => {
    if (!bootstrapped) void bootstrapAuth()
  }, [bootstrapped, bootstrapAuth])

  useEffect(() => {
    if (me) navigate(redirect ?? defaultNavPath(me.is_admin), { replace: true })
  }, [me, navigate, redirect])

  return (
    <main
      className="grid min-h-screen place-items-center p-4 sm:p-8"
      data-foundation-route-id="R-LOGIN"
    >
      <Card className="w-full max-w-md">
        <CardHeader>
          <CardTitle>{t('workspaceTitle')}</CardTitle>
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
            <form.Subscribe selector={(state) => state.submissionAttempts}>
              {(attempts) => (
                <>
                  <div className="grid gap-2">
                    <Label htmlFor="login-name">{t('loginName')}</Label>
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
                            autoFocus
                            disabled={busy}
                            id="login-name"
                            onBlur={field.handleBlur}
                            onChange={(event) =>
                              field.handleChange(event.currentTarget.value)
                            }
                            value={field.state.value}
                          />
                          <FieldError
                            attempts={attempts}
                            errors={field.state.meta.errors}
                          />
                        </>
                      )}
                    </form.Field>
                  </div>

                  <div className="grid gap-2">
                    <Label htmlFor="login-password">{t('password')}</Label>
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
                              autoComplete="current-password"
                              className="min-w-0 flex-1"
                              disabled={busy}
                              id="login-password"
                              onBlur={field.handleBlur}
                              onChange={(event) =>
                                field.handleChange(event.currentTarget.value)
                              }
                              type={showPassword ? 'text' : 'password'}
                              value={field.state.value}
                            />
                            <FieldError
                              attempts={attempts}
                              errors={field.state.meta.errors}
                            />
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
                </>
              )}
            </form.Subscribe>

            {error && (
              <Alert variant="destructive">
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            )}

            <Button className="w-full" disabled={busy} type="submit">
              {pendingOrigin(SESSION_AUTH_KEY) === 'login' ? (
                <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
              ) : (
                <LogIn aria-hidden="true" />
              )}
              <span>{t('login')}</span>
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
}

type FieldErrorProps = {
  attempts: number
  errors: Array<unknown>
}

/**
 * The message of a failed field.
 *
 * The message appears with the first submission attempt and then follows the
 * field, so an untouched field does not report an error before the user
 * tried to submit.
 */
function FieldError({ attempts, errors }: FieldErrorProps) {
  if (attempts === 0 || errors.length === 0) return null
  return (
    <p className="text-sm text-destructive" role="alert">
      {String(errors[0])}
    </p>
  )
}
