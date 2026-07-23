import { createRouter, createWebHistory } from 'vue-router'
import { pinia } from './pinia'
import { useSessionStore } from './stores/session'
import { defaultNavSection, navItems } from './nav'

const appRoutes = navItems.flatMap((item) => {
  if (!item.children?.length) {
    return [
      {
        path: item.section,
        name: item.section,
        component: item.loader,
        meta: { adminOnly: item.adminOnly ?? false },
      },
    ]
  }

  return [
    {
      path: item.section,
      redirect: item.children[0]!.path,
      meta: { adminOnly: item.adminOnly ?? false },
    },
    ...item.children.map((child) => ({
      path: child.path.slice(1),
      name: child.path.slice(1).replaceAll('/', '-'),
      component: item.loader,
      meta: {
        adminOnly: Boolean(item.adminOnly || child.adminOnly),
      },
    })),
  ]
})

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/login',
      name: 'login',
      component: () => import('./pages/LoginPage.vue'),
      meta: { public: true },
    },
    {
      path: '/',
      component: () => import('./layouts/AppShell.vue'),
      children: [{ path: '', redirect: '/api-keys' }, ...appRoutes],
    },
    {
      path: '/:pathMatch(.*)*',
      redirect: '/api-keys',
    },
  ],
})

router.beforeEach(async (to) => {
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
