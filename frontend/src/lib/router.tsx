import { useEffect, type ReactNode } from 'react'
import { Navigate, Outlet, useLocation } from 'react-router'
import type { MessageKey } from '@/i18n'
import type { AppRoute } from './nav'
import { defaultNavPath } from './nav'
import { useLocale } from './i18n'
import { useSessionStore } from './session'
import { WorkerQueryProvider } from './query'
import { Skeleton } from '@/components/ui/skeleton'
import { FoundationPlaceholder } from '@/pages/FoundationPlaceholder'

export function WorkerAuthGate() {
  const bootstrapped = useSessionStore((state) => state.bootstrapped)
  const bootstrapping = useSessionStore((state) => state.bootstrapping)
  const me = useSessionStore((state) => state.me)
  const epoch = useSessionStore((state) => state.epoch)
  const bootstrapAuth = useSessionStore((state) => state.bootstrapAuth)
  const location = useLocation()
  const { t } = useLocale()

  useEffect(() => {
    if (!bootstrapped) void bootstrapAuth()
  }, [bootstrapped, bootstrapAuth])

  if (!bootstrapped || bootstrapping) {
    return (
      <div
        aria-label={t('loading')}
        className="grid min-h-[50vh] place-items-center"
        role="status"
      >
        <Skeleton className="h-8 w-40" />
      </div>
    )
  }

  if (!me) {
    const redirect = `${location.pathname}${location.search}${location.hash}`
    const search = new URLSearchParams({ redirect }).toString()
    return <Navigate replace to={`/login?${search}`} />
  }

  // The authorized subtree reads through the query client of the current
  // session epoch, so no identity ever sees another identity's cached reads.
  return (
    <WorkerQueryProvider epoch={epoch}>
      <Outlet />
    </WorkerQueryProvider>
  )
}

export function AdminGuard({ children }: { children: ReactNode }) {
  const isAdmin = useSessionStore((state) => state.me?.is_admin ?? false)
  if (!isAdmin) return <Navigate replace to={defaultNavPath(false)} />
  return children
}

export function GuardedFoundationRoute({ route }: { route: AppRoute }) {
  const isAdmin = useSessionStore((state) => state.me?.is_admin ?? false)
  if (route.adminOnly && !isAdmin) {
    return <Navigate replace to={defaultNavPath(false)} />
  }
  return <FoundationPlaceholder route={route} />
}

export function usePageTitle(titleKey: MessageKey) {
  const { locale, t } = useLocale()
  useEffect(() => {
    document.title = `${t(titleKey)} · Prompt Ferry`
  }, [locale, t, titleKey])
}
