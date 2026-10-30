import { useState } from 'react'
import { Link, NavLink, Outlet, useLocation, useNavigate } from 'react-router'
import { LogOut, Menu, Server, X } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Separator } from '@/components/ui/separator'
import {
  Sheet,
  SheetClose,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from '@/components/ui/sheet'
import { LocaleToggle, ThemeToggle } from '@/components/auth/DisplayControls'
import { cn } from '@/lib/utils'
import {
  defaultNavPath,
  routeForPath,
  visibleNavSections,
  type NavSection,
} from '@/lib/nav'
import { useAsyncAction } from '@/lib/use-async-action'
import { useLocale } from '@/lib/i18n'
import { usePageTitle } from '@/lib/router'
import {
  RELAY_SESSION_AUTH_KEY,
  SESSION_AUTH_KEY,
  useRelaySessionStore,
  useSessionStore,
} from '@/lib/session'
import { useNotifier } from '@/lib/notifier'

function SectionLinks({
  section,
  onNavigate,
}: {
  section: NavSection
  onNavigate?: () => void
}) {
  const { t } = useLocale()
  const nested = [
    'endpoints',
    'redaction',
    'request-records',
    'settings',
  ].includes(section.id)
  const Icon = section.icon

  if (!nested) {
    const route = section.routes[0]!
    return (
      <NavLink
        end
        onClick={onNavigate}
        to={route.path}
        className={({ isActive }) =>
          cn(
            'flex min-h-10 items-center gap-3 rounded-md px-3 text-sm font-medium transition-colors outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50',
            isActive
              ? 'bg-accent text-accent-foreground'
              : 'text-muted-foreground hover:bg-accent/70 hover:text-foreground',
          )
        }
      >
        <Icon aria-hidden="true" className="size-4 shrink-0" />
        <span className="truncate">{t(route.titleKey)}</span>
      </NavLink>
    )
  }

  return (
    <div className="grid gap-0.5">
      <div className="flex min-h-8 items-center gap-3 px-3 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        <Icon aria-hidden="true" className="size-4 shrink-0" />
        <span className="truncate">{t(section.labelKey)}</span>
      </div>
      <div className="grid gap-0.5 pl-3">
        {section.routes.map((route) => (
          <NavLink
            key={route.id}
            onClick={onNavigate}
            to={route.path}
            className={({ isActive }) =>
              cn(
                'flex min-h-9 items-center rounded-md px-2.5 text-sm transition-colors outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50',
                isActive
                  ? 'bg-accent text-accent-foreground'
                  : 'text-muted-foreground hover:bg-accent/70 hover:text-foreground',
              )
            }
          >
            <span className="truncate">{t(route.titleKey)}</span>
          </NavLink>
        ))}
      </div>
    </div>
  )
}

function Navigation({
  isAdmin,
  onNavigate,
}: {
  isAdmin: boolean
  onNavigate?: () => void
}) {
  const { t } = useLocale()
  return (
    <nav aria-label={t('workspaceTitle')} className="grid content-start gap-2">
      {visibleNavSections(isAdmin).map((section) => (
        <SectionLinks
          key={section.id}
          onNavigate={onNavigate}
          section={section}
        />
      ))}
    </nav>
  )
}

function WorkerLogoutButton({
  disabled,
  pending,
  onLogout,
}: {
  disabled: boolean
  pending: boolean
  onLogout: () => void
}) {
  const { t } = useLocale()

  return (
    <Button
      aria-label={t('logout')}
      className="w-full justify-start"
      disabled={disabled}
      onClick={onLogout}
      type="button"
      variant="ghost"
    >
      <LogOut aria-hidden="true" />
      <span>{t('logout')}</span>
      {pending && (
        <span className="ml-auto size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
      )}
    </Button>
  )
}

export function AppShell() {
  const { t } = useLocale()
  const location = useLocation()
  const navigate = useNavigate()
  const { isPending, pendingOrigin } = useAsyncAction()
  const { notifyApiError } = useNotifier()
  const me = useSessionStore((state) => state.me)
  const logout = useSessionStore((state) => state.logout)
  const relayAuthenticated = useRelaySessionStore(
    (state) => state.authenticated,
  )
  const relayLogout = useRelaySessionStore((state) => state.logout)
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false)
  const isAdmin = me?.is_admin ?? false
  const currentRoute = routeForPath(location.pathname)
  const titleKey = currentRoute?.titleKey ?? 'workspaceTitle'
  const workerBusy = isPending(SESSION_AUTH_KEY)
  const relayBusy = isPending(RELAY_SESSION_AUTH_KEY)

  usePageTitle(titleKey)

  async function handleWorkerLogout(): Promise<void> {
    try {
      const outcome = await logout()
      if (outcome.executed) navigate('/login', { replace: true })
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  async function handleRelayLogout(): Promise<void> {
    try {
      await relayLogout()
    } catch (cause) {
      notifyApiError(cause)
    }
  }

  return (
    <div className="min-h-screen bg-background text-foreground">
      <aside className="fixed inset-y-0 left-0 z-30 hidden w-64 flex-col border-r bg-card lg:flex">
        <div className="flex h-16 items-center px-5">
          <Link className="font-semibold tracking-tight" to="/api-keys">
            Prompt Ferry
          </Link>
        </div>
        <Separator />
        <div className="min-h-0 flex-1 overflow-y-auto p-3">
          <Navigation isAdmin={isAdmin} />
        </div>
        <Separator />
        <div className="grid gap-2 p-3">
          {me && (
            <span className="truncate px-2 text-xs text-muted-foreground">
              {me.display_name || me.login_name}
            </span>
          )}
          {me && (
            <WorkerLogoutButton
              disabled={workerBusy}
              onLogout={() => void handleWorkerLogout()}
              pending={pendingOrigin(SESSION_AUTH_KEY) === 'logout'}
            />
          )}
          {relayAuthenticated && (
            <Button
              className="w-full justify-start"
              disabled={relayBusy}
              onClick={() => void handleRelayLogout()}
              type="button"
              variant="ghost"
            >
              <Server aria-hidden="true" />
              <span>{t('relayHost')}</span>
              {pendingOrigin(RELAY_SESSION_AUTH_KEY) === 'logout' && (
                <span className="ml-auto size-3 animate-spin rounded-full border-2 border-current border-r-transparent" />
              )}
            </Button>
          )}
        </div>
      </aside>

      <div className="min-w-0 lg:pl-64">
        <header className="sticky top-0 z-20 flex min-h-16 items-center gap-2 border-b bg-background/95 px-3 backdrop-blur sm:px-5">
          <Sheet
            open={mobileMenuOpen}
            onOpenChange={(open) => {
              if (!open && workerBusy) return
              setMobileMenuOpen(open)
            }}
          >
            <SheetTrigger asChild>
              <Button
                aria-label={t('expandNav')}
                className="lg:hidden"
                disabled={workerBusy}
                size="icon"
                type="button"
                variant="ghost"
              >
                <Menu aria-hidden="true" />
              </Button>
            </SheetTrigger>
            <SheetContent
              className="w-[min(20rem,85vw)] overflow-hidden p-0"
              showCloseButton={false}
              side="left"
            >
              <SheetHeader className="flex-row items-center justify-between border-b px-4 py-3">
                <SheetTitle>Prompt Ferry</SheetTitle>
                <SheetClose asChild>
                  <Button
                    aria-label={t('cancel')}
                    disabled={workerBusy}
                    size="icon"
                    type="button"
                    variant="ghost"
                  >
                    <X aria-hidden="true" />
                  </Button>
                </SheetClose>
              </SheetHeader>
              <div className="min-h-0 flex-1 overflow-y-auto p-3">
                <Navigation
                  isAdmin={isAdmin}
                  onNavigate={() => {
                    if (!workerBusy) setMobileMenuOpen(false)
                  }}
                />
              </div>
              {me && (
                <>
                  <Separator />
                  <div className="grid gap-2 p-3">
                    <span className="truncate px-2 text-xs text-muted-foreground">
                      {me.display_name || me.login_name}
                    </span>
                    <WorkerLogoutButton
                      disabled={workerBusy}
                      onLogout={() => void handleWorkerLogout()}
                      pending={pendingOrigin(SESSION_AUTH_KEY) === 'logout'}
                    />
                  </div>
                </>
              )}
            </SheetContent>
          </Sheet>
          <div className="min-w-0 flex-1">
            <h1 className="truncate text-sm font-semibold sm:text-base">
              {t(titleKey)}
            </h1>
          </div>
          <div className="flex shrink-0 items-center gap-1.5">
            <LocaleToggle />
            <ThemeToggle />
          </div>
        </header>

        <main className="min-w-0 p-4 sm:p-6">
          <Outlet />
        </main>
      </div>
    </div>
  )
}
