import { watch } from 'vue'
import {
  createMemoryHistory,
  createRouter,
  createWebHistory,
  type RouteLocationNormalizedLoaded,
} from 'vue-router'
import { messages, type Locale, type MessageKey } from './i18n'
import { i18n } from './i18n/plugin'
import { pinia } from './pinia'
import { useRelaySessionStore } from './stores/relay-session'
import { useSessionStore } from './stores/session'
import { defaultNavSection, isRelayHostPath, navItems } from './nav'

export { isRelayHostPath }

declare module 'vue-router' {
  interface RouteMeta {
    adminOnly?: boolean
    public?: boolean
    titleKey?: MessageKey
  }
}

const APP_NAME = 'Prompt Ferry'

export function formatPageTitle(
  localeValue: Locale,
  titleKey: MessageKey,
): string {
  return `${messages[localeValue][titleKey]} · ${APP_NAME}`
}

export function routeTitleKey(
  route: Pick<RouteLocationNormalizedLoaded, 'meta'>,
): MessageKey {
  return route.meta.titleKey ?? 'workspaceTitle'
}

function applyDocumentTitle(
  route: Pick<RouteLocationNormalizedLoaded, 'meta'>,
): void {
  if (typeof document === 'undefined') return
  document.title = formatPageTitle(
    i18n.global.locale.value as Locale,
    routeTitleKey(route),
  )
}

const appRoutes = navItems.flatMap((item) => {
  if (!item.children?.length) {
    return [
      {
        path: item.section,
        name: item.section,
        component: item.loader,
        meta: {
          adminOnly: item.adminOnly ?? false,
          titleKey: item.labelKey,
        },
      },
    ]
  }

  return [
    {
      path: item.section,
      redirect: item.children[0]!.path,
      meta: {
        adminOnly: item.adminOnly ?? false,
        titleKey: item.labelKey,
      },
    },
    ...item.children.map((child) => ({
      path: child.path.slice(1),
      name: child.path.slice(1).replaceAll('/', '-'),
      component: item.loader,
      meta: {
        adminOnly: Boolean(item.adminOnly || child.adminOnly),
        titleKey: child.labelKey,
      },
    })),
  ]
})

export const router = createRouter({
  history:
    typeof window === 'undefined' ? createMemoryHistory() : createWebHistory(),
  routes: [
    {
      path: '/login',
      name: 'login',
      component: () => import('./pages/LoginPage.vue'),
      meta: { public: true, titleKey: 'login' },
    },
    {
      path: '/',
      component: () => import('./layouts/AppShell.vue'),
      children: [{ path: '', redirect: '/api-keys' }, ...appRoutes],
    },
    {
      path: '/:pathMatch(.*)*',
      redirect: '/api-keys',
      meta: { titleKey: 'workspaceTitle' },
    },
  ],
})

let titleLocaleWatchStarted = false

router.afterEach((to) => {
  applyDocumentTitle(to)
  if (titleLocaleWatchStarted || typeof document === 'undefined') return
  titleLocaleWatchStarted = true
  watch(i18n.global.locale, () => applyDocumentTitle(router.currentRoute.value))
})

router.beforeEach(async (to) => {
  // The Relay host page owns a separate relay-token session and must open
  // without a Worker: it never reads `session.me`/`isAdmin` and never
  // redirects to the Worker login when the Worker is absent.
  if (isRelayHostPath(to.path)) {
    const relaySession = useRelaySessionStore(pinia)
    if (!relaySession.bootstrapped) {
      try {
        await relaySession.bootstrap()
      } catch {
        // The host page renders its own token form on failure.
      }
    }
    return true
  }

  const session = useSessionStore(pinia)
  const isPublic = to.matched.some((record) => record.meta.public)
  if (!session.bootstrapped) {
    await session.bootstrapAuth()
  }

  if (isPublic) {
    if (session.me) {
      const redirect =
        typeof to.query.redirect === 'string'
          ? to.query.redirect
          : `/${defaultNavSection(session.isAdmin)}`
      return redirect
    }
    return true
  }

  if (!session.me) {
    return {
      name: 'login',
      query: { redirect: to.fullPath },
    }
  }

  if (to.matched.some((record) => record.meta.adminOnly) && !session.isAdmin) {
    return `/${defaultNavSection(false)}`
  }

  return true
})
