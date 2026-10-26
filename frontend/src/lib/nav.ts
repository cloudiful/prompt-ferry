import type { LucideIcon } from 'lucide-react'
import {
  Blocks,
  Cable,
  ChartColumn,
  ClipboardCheck,
  Database,
  KeyRound,
  Plug,
  Receipt,
  Server,
  Settings,
  ShieldCheck,
  Users,
} from 'lucide-react'
import type { MessageKey } from '@/i18n'

export type AppRoute = {
  id: string
  path: string
  titleKey: MessageKey
  adminOnly: boolean
}

export type NavSection = {
  id: string
  labelKey: MessageKey
  icon: LucideIcon
  adminOnly?: boolean
  routes: AppRoute[]
}

export const navSections: NavSection[] = [
  {
    id: 'api-keys',
    labelKey: 'apiKeys',
    icon: KeyRound,
    routes: [
      {
        id: 'R-API-KEYS',
        path: '/api-keys',
        titleKey: 'apiKeys',
        adminOnly: false,
      },
    ],
  },
  {
    id: 'available-models',
    labelKey: 'availableModels',
    icon: Database,
    routes: [
      {
        id: 'R-AVAILABLE-MODELS',
        path: '/available-models',
        titleKey: 'availableModels',
        adminOnly: false,
      },
    ],
  },
  {
    id: 'users',
    labelKey: 'user',
    icon: Users,
    adminOnly: true,
    routes: [
      {
        id: 'R-USERS',
        path: '/users',
        titleKey: 'user',
        adminOnly: true,
      },
    ],
  },
  {
    id: 'endpoints',
    labelKey: 'endpoint',
    icon: Cable,
    adminOnly: true,
    routes: [
      {
        id: 'R-ENDPOINTS-UPSTREAMS',
        path: '/endpoints/upstreams',
        titleKey: 'endpoint',
        adminOnly: true,
      },
      {
        id: 'R-ENDPOINTS-ROUTES',
        path: '/endpoints/routes',
        titleKey: 'modelRoute',
        adminOnly: true,
      },
    ],
  },
  {
    id: 'mcp',
    labelKey: 'mcp',
    icon: Blocks,
    routes: [{ id: 'R-MCP', path: '/mcp', titleKey: 'mcp', adminOnly: false }],
  },
  {
    id: 'relays',
    labelKey: 'relays',
    icon: Plug,
    adminOnly: true,
    routes: [
      {
        id: 'R-RELAYS',
        path: '/relays',
        titleKey: 'relays',
        adminOnly: true,
      },
    ],
  },
  {
    id: 'relay-host',
    labelKey: 'relayHost',
    icon: Server,
    routes: [
      {
        id: 'R-RELAY-HOST',
        path: '/relay-host',
        titleKey: 'relayHost',
        adminOnly: false,
      },
    ],
  },
  {
    id: 'redaction',
    labelKey: 'redaction',
    icon: ShieldCheck,
    routes: [
      {
        id: 'R-REDACTION-RULES',
        path: '/redaction/rules',
        titleKey: 'redactionRules',
        adminOnly: false,
      },
      {
        id: 'R-REDACTION-PREVIEW',
        path: '/redaction/preview',
        titleKey: 'redactionPreview',
        adminOnly: false,
      },
    ],
  },
  {
    id: 'request-records',
    labelKey: 'requestRecords',
    icon: ChartColumn,
    routes: [
      {
        id: 'R-USAGE-AI',
        path: '/request-records/ai',
        titleKey: 'aiRequests',
        adminOnly: false,
      },
      {
        id: 'R-USAGE-MCP',
        path: '/request-records/mcp',
        titleKey: 'mcpCalls',
        adminOnly: false,
      },
    ],
  },
  {
    id: 'billing',
    labelKey: 'billing',
    icon: Receipt,
    routes: [
      {
        id: 'R-BILLING',
        path: '/billing',
        titleKey: 'billing',
        adminOnly: false,
      },
    ],
  },
  {
    id: 'approvals',
    labelKey: 'approvals',
    icon: ClipboardCheck,
    adminOnly: true,
    routes: [
      {
        id: 'R-APPROVALS',
        path: '/approvals',
        titleKey: 'approvals',
        adminOnly: true,
      },
    ],
  },
  {
    id: 'settings',
    labelKey: 'settings',
    icon: Settings,
    routes: [
      {
        id: 'R-SETTINGS-GENERAL',
        path: '/settings/general',
        titleKey: 'settingsGeneral',
        adminOnly: false,
      },
      {
        id: 'R-SETTINGS-REQUESTS',
        path: '/settings/requests',
        titleKey: 'settingsUsage',
        adminOnly: true,
      },
      {
        id: 'R-SETTINGS-NETWORK',
        path: '/settings/network',
        titleKey: 'settingsNetwork',
        adminOnly: true,
      },
      {
        id: 'R-SETTINGS-REVIEW',
        path: '/settings/review',
        titleKey: 'settingsReview',
        adminOnly: true,
      },
      {
        id: 'R-SETTINGS-STORAGE',
        path: '/settings/storage',
        titleKey: 'settingsStorage',
        adminOnly: true,
      },
      {
        id: 'R-SETTINGS-CACHE-ALERT',
        path: '/settings/cache-alert',
        titleKey: 'settingsCacheAlert',
        adminOnly: true,
      },
      {
        id: 'R-SETTINGS-CONFIG-ARCHIVE',
        path: '/settings/config-archive',
        titleKey: 'settingsConfigArchive',
        adminOnly: true,
      },
    ],
  },
]

export const appRoutes = navSections.flatMap((section) => section.routes)

export function visibleNavSections(isAdmin: boolean): NavSection[] {
  return navSections.flatMap((section) => {
    if (section.adminOnly && !isAdmin) return []
    const routes = section.routes.filter((route) => isAdmin || !route.adminOnly)
    return routes.length > 0 ? [{ ...section, routes }] : []
  })
}

export function routeForPath(path: string): AppRoute | undefined {
  return appRoutes.find((route) => route.path === path)
}

export function defaultNavPath(isAdmin: boolean): string {
  return visibleNavSections(isAdmin)[0]?.routes[0]?.path ?? '/settings/general'
}

export function isRelayHostPath(path: string): boolean {
  return path === '/relay-host' || path.startsWith('/relay-host/')
}
