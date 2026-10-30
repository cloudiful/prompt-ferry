import { useState } from 'react'
import { act, fireEvent, render, screen, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { EndpointsTable } from '@/components/endpoints/EndpointsTable'
import { ModelRoutesTable } from '@/components/endpoints/ModelRoutesTable'
import { McpTable } from '@/components/mcp/McpTable'
import { testResultKey } from '@/components/shared/TestResultPopover'
import { LocaleProvider } from '@/lib/i18n'
import type {
  EndpointListItemView,
  ModelRouteListItemView,
} from '@/models/endpoints'
import type { McpServerListItemView } from '@/models'
import type { McpServer } from '@/generated/admin-api'

const ROUTE_RESULT = '1/2 targets passed / model-a / first result'
const MCP_RESULT = 'fixture catalog refresh succeeded'

beforeEach(() => {
  localStorage.setItem('prompt-ferry:locale', 'en-US')
})

function routeItem(
  overrides: Partial<ModelRouteListItemView> = {},
): ModelRouteListItemView {
  return {
    enabled: true,
    model_pattern: 'fixture-*',
    owner_label: 'Owner 7',
    rule_id: 'rule-1',
    scope: 'global',
    scope_label: 'Public',
    targets: [],
    test_message: 'Not tested',
    test_results: [],
    test_severity: null,
    testing: false,
    toggling: false,
    ...overrides,
  }
}

function endpointItem(
  overrides: Partial<EndpointListItemView> = {},
): EndpointListItemView {
  return {
    base_url: 'https://api.minimax.chat/v1',
    enabled: true,
    endpoint_id: 'endpoint-1',
    has_oauth_token: false,
    mcp_enabled: false,
    name: 'Fixture upstream',
    native_api: 'auto',
    native_api_label: 'Automatic by caller protocol',
    native_api_source: 'auto',
    native_api_source_label: 'Automatic by caller protocol',
    owner_label: '',
    plan: 'platform_api_key',
    provider: 'minimax',
    provider_region: null,
    scope: 'global',
    scope_label: 'Public',
    test_message: 'Not tested',
    test_severity: null,
    testing: false,
    toggling: false,
    ...overrides,
  }
}

function mcpItem(
  overrides: Partial<McpServerListItemView> = {},
): McpServerListItemView {
  return {
    endpoint_label: 'https://mcp.context7.com/mcp',
    enabled: true,
    name: 'Fixture Context7',
    naming_mode_label: 'Passthrough',
    provider_label: 'Context7',
    scope_label: 'Public',
    server: { server_id: 'mcp-1' } as McpServer,
    server_id: 'mcp-1',
    show_provider_badge: true,
    test_message: '-',
    test_ok: null,
    timeout_label: '30000ms',
    transport: 'http',
    ...overrides,
  }
}

function RouteHarness({
  onShowDetails = vi.fn(),
}: {
  onShowDetails?: (ruleId: string) => void
}) {
  const [openKey, setOpenKey] = useState<string | null>(null)
  const [pending, setPending] = useState(false)
  const [tested, setTested] = useState(false)

  const item = routeItem({
    test_message: tested ? ROUTE_RESULT : 'Not tested',
    test_results: tested
      ? [
          {
            duration_ms: 100,
            endpoint_id: 'ep-1',
            endpoint_name: 'Upstream One',
            message: 'first result',
            model: 'model-a',
            ok: true,
            preferred: true,
            status: 200,
            target_id: 'target-1',
          },
          {
            duration_ms: 140,
            endpoint_id: 'ep-2',
            endpoint_name: 'Upstream Two',
            message: 'second result',
            model: 'model-b',
            ok: false,
            preferred: false,
            status: 503,
            target_id: 'target-2',
          },
        ]
      : [],
    test_severity: tested ? 'success' : null,
    testing: pending,
  })

  return (
    <LocaleProvider>
      <ModelRoutesTable
        first={0}
        items={[item]}
        onDelete={vi.fn()}
        onEdit={vi.fn()}
        onPageChange={vi.fn()}
        onProbe={async (ruleId, surface) => {
          setPending(true)
          await Promise.resolve()
          setPending(false)
          setTested(true)
          setOpenKey(testResultKey(surface, ruleId))
        }}
        onResultOpenChange={(key, open) => {
          if (open) return
          setOpenKey((current) => (current === key ? null : current))
        }}
        onShowDetails={onShowDetails}
        onToggle={vi.fn()}
        onToggleTarget={vi.fn()}
        openResultKey={openKey}
        pendingActions={{}}
        rows={10}
        total={1}
      />
    </LocaleProvider>
  )
}

function EndpointHarness() {
  const [openKey, setOpenKey] = useState<string | null>(null)
  const [pending, setPending] = useState(false)
  const [runs, setRuns] = useState(0)

  const item = endpointItem({
    test_message: runs === 0 ? 'Not tested' : `HTTP 200 / 14ms / run ${runs}`,
    test_severity: runs === 0 ? null : 'success',
  })

  return (
    <LocaleProvider>
      <EndpointsTable
        first={0}
        items={[item]}
        loading={false}
        onDelete={vi.fn()}
        onEdit={vi.fn()}
        onPage={vi.fn()}
        onResultOpenChange={(key, open) => {
          if (open) return
          setOpenKey((current) => (current === key ? null : current))
        }}
        onTest={async (endpointId, surface) => {
          setPending(true)
          await Promise.resolve()
          setPending(false)
          setRuns((current) => current + 1)
          setOpenKey(testResultKey(surface, endpointId))
        }}
        onTokenPlanUsage={vi.fn()}
        onToggle={vi.fn()}
        openResultKey={openKey}
        pendingActions={{
          ...(pending ? { 'networktest:endpoint:endpoint-1': 'test' } : {}),
        }}
        rows={25}
        total={1}
      />
    </LocaleProvider>
  )
}

function McpHarness() {
  const [openKey, setOpenKey] = useState<string | null>(null)
  const [pending, setPending] = useState(false)
  const [tested, setTested] = useState(false)

  const item = mcpItem({
    test_message: tested ? MCP_RESULT : '-',
    test_ok: tested ? true : null,
  })

  return (
    <LocaleProvider>
      <McpTable
        busy={false}
        first={0}
        items={[item]}
        onDelete={vi.fn()}
        onEdit={vi.fn()}
        onPageChange={vi.fn()}
        onResultOpenChange={(key, open) => {
          if (open) return
          setOpenKey((current) => (current === key ? null : current))
        }}
        onTest={async (server, surface) => {
          setPending(true)
          await Promise.resolve()
          setPending(false)
          setTested(true)
          setOpenKey(testResultKey(surface, server.server_id))
        }}
        onToggle={vi.fn()}
        openResultKey={openKey}
        pendingActions={{
          ...(pending ? { 'mcptest:mcp-1': 'test' } : {}),
        }}
        rows={10}
        total={1}
      />
    </LocaleProvider>
  )
}

describe('list test actions', () => {
  it('keeps the model-route test out of its own column', () => {
    render(<RouteHarness />)
    const headers = screen
      .getAllByRole('columnheader')
      .map((header) => (header.textContent ?? '').trim())
    expect(headers).toEqual(['Model pattern', 'Endpoint', 'Status', 'Edit'])
    expect(headers).not.toContain('Test')
  })

  it('shows the model-route result once and reaches its per-target details', async () => {
    const onShowDetails = vi.fn()
    render(<RouteHarness onShowDetails={onShowDetails} />)
    const row = document.querySelector(
      '[data-model-route-row="rule-1"]',
    ) as HTMLElement
    await act(async () => {
      fireEvent.click(
        within(row).getByRole('button', { name: 'Test fixture-*' }),
      )
    })

    expect(await screen.findByText(ROUTE_RESULT)).toBeTruthy()
    // The result lives in a one-shot popover, never in a permanent column.
    expect(within(row).queryByText(ROUTE_RESULT)).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: '1/2' }))
    expect(onShowDetails).toHaveBeenCalledWith('rule-1')
  })

  it('drops the public scope badge but keeps the private owner of a model route', () => {
    render(<RouteHarness />)
    const row = document.querySelector(
      '[data-model-route-row="rule-1"]',
    ) as HTMLElement
    expect(row.textContent).not.toContain('Public')
    expect(row.textContent).toContain('Owner 7')
  })

  it('shows the upstream test result once and keeps the action reachable', async () => {
    render(<EndpointHarness />)
    const headers = screen
      .getAllByRole('columnheader')
      .map((header) => (header.textContent ?? '').trim())
    expect(headers).toEqual(['Endpoint', 'Status', 'Usage', 'Actions'])
    expect(headers).not.toContain('Test')

    const row = document.querySelector(
      '[data-endpoint-row="endpoint-1"]',
    ) as HTMLElement
    await act(async () => {
      fireEvent.click(within(row).getByRole('button', { name: 'Test' }))
    })

    expect(await screen.findByText('HTTP 200 / 14ms / run 1')).toBeTruthy()
    expect(
      within(row).queryByRole('button', { name: 'HTTP 200 / 14ms / run 1' }),
    ).toBeNull()
  })

  it('shows the MCP catalog test result in the action popover', async () => {
    render(<McpHarness />)
    const headers = screen
      .getAllByRole('columnheader')
      .map((header) => (header.textContent ?? '').trim())
    expect(headers).toEqual(['Name', 'Status', 'Edit'])
    expect(headers).not.toContain('Test')

    const row = document.querySelector(
      '[data-mcp-server-row="mcp-1"]',
    ) as HTMLElement
    await act(async () => {
      fireEvent.click(
        within(row).getByRole('button', { name: 'Refresh catalog' }),
      )
    })

    expect(await screen.findByText(MCP_RESULT)).toBeTruthy()
  })

  it('closes the result popover on Escape without keeping a result column', async () => {
    render(<EndpointHarness />)
    const row = document.querySelector(
      '[data-endpoint-row="endpoint-1"]',
    ) as HTMLElement
    await act(async () => {
      fireEvent.click(within(row).getByRole('button', { name: 'Test' }))
    })
    expect(await screen.findByText('HTTP 200 / 14ms / run 1')).toBeTruthy()

    await act(async () => {
      fireEvent.keyDown(document.body, { key: 'Escape' })
    })
    expect(screen.queryByText('HTTP 200 / 14ms / run 1')).toBeNull()
  })

  it('never shows a stale result when a closed test is run again', async () => {
    render(<EndpointHarness />)
    const row = document.querySelector(
      '[data-endpoint-row="endpoint-1"]',
    ) as HTMLElement
    await act(async () => {
      fireEvent.click(within(row).getByRole('button', { name: 'Test' }))
    })
    expect(await screen.findByText('HTTP 200 / 14ms / run 1')).toBeTruthy()
    await act(async () => {
      fireEvent.keyDown(document.body, { key: 'Escape' })
    })

    await act(async () => {
      fireEvent.click(within(row).getByRole('button', { name: 'Test' }))
    })
    expect(await screen.findByText('HTTP 200 / 14ms / run 2')).toBeTruthy()
    expect(screen.queryByText('HTTP 200 / 14ms / run 1')).toBeNull()
  })
})
