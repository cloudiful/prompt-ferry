import { useState } from 'react'
import { BrowserRouter, Navigate, Route, Routes } from 'react-router'
import { LoginPage } from '@/pages/LoginPage'
import { RelayHostPage } from '@/pages/RelayHostPage'
import { RelaysPage } from '@/pages/RelaysPage'
import { AppShell } from '@/layouts/AppShell'
import { ApiKeysPage } from '@/pages/ApiKeysPage'
import { AvailableModelsPage } from '@/pages/AvailableModelsPage'
import { UsersPage } from '@/pages/UsersPage'
import { EndpointsPage } from '@/pages/EndpointsPage'
import { ModelRoutesPage } from '@/pages/ModelRoutesPage'
import { McpPage } from '@/pages/McpPage'
import { RedactionPage } from '@/pages/RedactionPage'
import { BillingPage } from '@/pages/BillingPage'
import { ApprovalsPage } from '@/pages/ApprovalsPage'
import { SettingsPage } from '@/pages/SettingsPage'
import { UsagePage } from '@/pages/UsagePage'
import { appRoutes, type AppRoute } from '@/lib/nav'
import { createAppQueryClient } from '@/lib/query'
import {
  AdminGuard,
  GuardedFoundationRoute,
  WorkerAuthGate,
} from '@/lib/router'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { ThemeProvider } from '@/lib/theme'
import { QueryClientProvider } from '@tanstack/react-query'

function AppRouteElement({ route }: { route: AppRoute }) {
  switch (route.id) {
    case 'R-API-KEYS':
      return <ApiKeysPage />
    case 'R-AVAILABLE-MODELS':
      return <AvailableModelsPage />
    case 'R-USERS':
      return (
        <AdminGuard>
          <UsersPage />
        </AdminGuard>
      )
    case 'R-ENDPOINTS-UPSTREAMS':
      return (
        <AdminGuard>
          <EndpointsPage />
        </AdminGuard>
      )
    case 'R-ENDPOINTS-ROUTES':
      return (
        <AdminGuard>
          <ModelRoutesPage />
        </AdminGuard>
      )
    case 'R-MCP':
      return <McpPage />
    case 'R-REDACTION-RULES':
    case 'R-REDACTION-PREVIEW':
      return <RedactionPage />
    case 'R-BILLING':
      return <BillingPage />
    case 'R-APPROVALS':
      return (
        <AdminGuard>
          <ApprovalsPage />
        </AdminGuard>
      )
    case 'R-USAGE-AI':
    case 'R-USAGE-MCP':
      return <UsagePage />
    case 'R-SETTINGS-GENERAL':
      return <SettingsPage />
    case 'R-SETTINGS-REQUESTS':
      return (
        <AdminGuard>
          <SettingsPage />
        </AdminGuard>
      )
    case 'R-SETTINGS-NETWORK':
    case 'R-SETTINGS-REVIEW':
    case 'R-SETTINGS-STORAGE':
    case 'R-SETTINGS-CACHE-ALERT':
    case 'R-SETTINGS-CONFIG-ARCHIVE':
      return (
        <AdminGuard>
          <SettingsPage />
        </AdminGuard>
      )
    case 'R-RELAYS':
      return (
        <AdminGuard>
          <RelaysPage />
        </AdminGuard>
      )
    default:
      return <GuardedFoundationRoute route={route} />
  }
}

export default function App() {
  const [queryClient] = useState(createAppQueryClient)

  return (
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <ThemeProvider>
          <NotifierProvider>
            <BrowserRouter>
              <Routes>
                <Route element={<LoginPage />} path="/login" />
                <Route element={<AppShell />} path="/relay-host">
                  <Route element={<RelayHostPage />} index />
                </Route>
                <Route element={<WorkerAuthGate />}>
                  <Route element={<AppShell />}>
                    <Route
                      element={<Navigate replace to="/api-keys" />}
                      path="/"
                    />
                    <Route
                      element={<Navigate replace to="/endpoints/upstreams" />}
                      path="/endpoints"
                    />
                    <Route
                      element={<Navigate replace to="/redaction/rules" />}
                      path="/redaction"
                    />
                    <Route
                      element={<Navigate replace to="/request-records/ai" />}
                      path="/request-records"
                    />
                    <Route
                      element={<Navigate replace to="/settings/general" />}
                      path="/settings"
                    />
                    {appRoutes
                      .filter((route) => route.path !== '/relay-host')
                      .map((route) => (
                        <Route
                          element={<AppRouteElement route={route} />}
                          key={route.id}
                          path={route.path}
                        />
                      ))}
                    <Route
                      element={<Navigate replace to="/api-keys" />}
                      path="*"
                    />
                  </Route>
                </Route>
                <Route element={<Navigate replace to="/api-keys" />} path="*" />
              </Routes>
            </BrowserRouter>
          </NotifierProvider>
        </ThemeProvider>
      </LocaleProvider>
    </QueryClientProvider>
  )
}
