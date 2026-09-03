import type {
  RouteLocationNormalizedLoaded,
  RouteLocationRaw,
} from 'vue-router'
import type { MessageKey } from './i18n'
import type { NavigationSection } from './models'

export type NavChildItem = {
  path: string
  labelKey: MessageKey
  to: RouteLocationRaw
  adminOnly?: boolean
  isActive: (route: RouteLocationNormalizedLoaded) => boolean
}

export type NavItem = {
  section: NavigationSection
  labelKey: MessageKey
  iconActive: string
  iconInactive: string
  loader: () => Promise<unknown>
  adminOnly?: boolean
  children?: NavChildItem[]
}

export const navItems: NavItem[] = [
  {
    section: 'api-keys',
    labelKey: 'apiKeys',
    iconActive: 'i-lucide-key-round',
    iconInactive: 'i-lucide-key',
    loader: () => import('./pages/ApiKeysPage.vue'),
  },
  {
    section: 'available-models',
    labelKey: 'availableModels',
    iconActive: 'i-lucide-database',
    iconInactive: 'i-lucide-database-zap',
    loader: () => import('./pages/AvailableModelsPage.vue'),
  },
  {
    section: 'users',
    labelKey: 'user',
    iconActive: 'i-lucide-users',
    iconInactive: 'i-lucide-users-round',
    loader: () => import('./pages/UsersPage.vue'),
    adminOnly: true,
  },
  {
    section: 'endpoints',
    labelKey: 'endpoint',
    iconActive: 'i-lucide-cable',
    iconInactive: 'i-lucide-network',
    loader: () => import('./pages/EndpointsPage.vue'),
    adminOnly: true,
    children: [
      {
        path: '/endpoints/upstreams',
        labelKey: 'endpoint',
        to: { path: '/endpoints/upstreams' },
        isActive: (route) => route.path === '/endpoints/upstreams',
      },
      {
        path: '/endpoints/routes',
        labelKey: 'modelRoute',
        to: { path: '/endpoints/routes' },
        isActive: (route) => route.path === '/endpoints/routes',
      },
    ],
  },
  {
    section: 'mcp',
    labelKey: 'mcp',
    iconActive: 'i-lucide-blocks',
    iconInactive: 'i-lucide-workflow',
    loader: () => import('./pages/McpPage.vue'),
  },
  {
    section: 'relays',
    labelKey: 'relays',
    iconActive: 'i-lucide-plug-zap',
    iconInactive: 'i-lucide-plug',
    loader: () => import('./pages/RelaysPage.vue'),
    adminOnly: true,
  },
  {
    section: 'redaction',
    labelKey: 'redaction',
    iconActive: 'i-lucide-shield-check',
    iconInactive: 'i-lucide-shield',
    loader: () => import('./pages/RedactionPage.vue'),
    children: [
      {
        path: '/redaction/rules',
        labelKey: 'redactionRules',
        to: { path: '/redaction/rules' },
        isActive: (route) => route.path === '/redaction/rules',
      },
      {
        path: '/redaction/preview',
        labelKey: 'redactionPreview',
        to: { path: '/redaction/preview' },
        isActive: (route) => route.path === '/redaction/preview',
      },
    ],
  },
  {
    section: 'request-records',
    labelKey: 'requestRecords',
    iconActive: 'i-lucide-chart-no-axes-column',
    iconInactive: 'i-lucide-chart-column',
    loader: () => import('./pages/UsagePage.vue'),
    children: [
      {
        path: '/request-records/ai',
        labelKey: 'aiRequests',
        to: { path: '/request-records/ai' },
        isActive: (route) => route.path === '/request-records/ai',
      },
      {
        path: '/request-records/mcp',
        labelKey: 'mcpCalls',
        to: { path: '/request-records/mcp' },
        isActive: (route) => route.path === '/request-records/mcp',
      },
    ],
  },
  {
    section: 'billing',
    labelKey: 'billing',
    iconActive: 'i-lucide-receipt-text',
    iconInactive: 'i-lucide-receipt',
    loader: () => import('./pages/BillingPage.vue'),
  },
  {
    section: 'approvals',
    labelKey: 'approvals',
    iconActive: 'i-lucide-clipboard-check',
    iconInactive: 'i-lucide-clipboard-list',
    loader: () => import('./pages/ApprovalsPage.vue'),
    adminOnly: true,
  },
  {
    section: 'settings',
    labelKey: 'settings',
    iconActive: 'i-lucide-settings',
    iconInactive: 'i-lucide-settings-2',
    loader: () => import('./pages/SettingsPage.vue'),
    children: [
      {
        path: '/settings/general',
        labelKey: 'settingsGeneral',
        to: { path: '/settings/general' },
        isActive: (route) => route.path === '/settings/general',
      },
      {
        path: '/settings/requests',
        labelKey: 'settingsUsage',
        to: { path: '/settings/requests' },
        adminOnly: true,
        isActive: (route) => route.path === '/settings/requests',
      },
      {
        path: '/settings/network',
        labelKey: 'settingsNetwork',
        to: { path: '/settings/network' },
        adminOnly: true,
        isActive: (route) => route.path === '/settings/network',
      },
      {
        path: '/settings/review',
        labelKey: 'settingsReview',
        to: { path: '/settings/review' },
        adminOnly: true,
        isActive: (route) => route.path === '/settings/review',
      },
      {
        path: '/settings/storage',
        labelKey: 'settingsStorage',
        to: { path: '/settings/storage' },
        adminOnly: true,
        isActive: (route) => route.path === '/settings/storage',
      },
    ],
  },
]

export function visibleNavItems(isAdmin: boolean): NavItem[] {
  return navItems
    .filter((item) => isAdmin || !item.adminOnly)
    .map((item) => ({
      ...item,
      children: item.children?.filter((child) => isAdmin || !child.adminOnly),
    }))
}

export function defaultNavSection(isAdmin: boolean): NavigationSection {
  return visibleNavItems(isAdmin)[0]?.section ?? 'settings'
}
