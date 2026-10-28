import { act, fireEvent, render, screen, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  RequestRecordOverviewBreakdownRow,
  RequestRecordOverviewResponse,
  RequestRecordOverviewTokenUsage,
} from '@/generated/admin-api'
import { messages } from '@/i18n'
import { LocaleProvider } from '@/lib/i18n'
import { ThemeProvider } from '@/lib/theme'
import { RequestRecordsOverviewPanel } from '@/components/usage/RequestRecordsOverviewPanel'
import type { UsageChartProps } from '@/components/shared/UsageChart'
import type {
  UsageCategory,
  UsagePerspective,
  UsageScope,
} from '@/lib/usage-model'
import type { UsageOverviewDrilldown } from '@/lib/usage-overview'

const copy = messages['en-US']

/** The pending chart chunk a test holds open. */
type ChartSuspension = {
  pending: boolean
  ready: Promise<void>
  resolve: () => void
}

let chartSuspension: ChartSuspension | null = null

vi.mock('@/components/shared/UsageChart', async (importOriginal) => {
  const actual =
    await importOriginal<typeof import('@/components/shared/UsageChart')>()
  const Chart = actual.UsageChart
  return {
    ...actual,
    UsageChart: (props: UsageChartProps) => {
      const suspension = chartSuspension
      if (suspension !== null && suspension.pending) throw suspension.ready
      return <Chart {...props} />
    },
  }
})

/** Keeps the chart chunk unresolved until the returned suspension resolves. */
function suspendChartChunk(): ChartSuspension {
  let resolve!: () => void
  const ready = new Promise<void>((settle) => {
    resolve = settle
  })
  const suspension: ChartSuspension = {
    pending: true,
    ready,
    resolve: () => {
      suspension.pending = false
      resolve()
    },
  }
  chartSuspension = suspension
  return suspension
}

function releaseChartChunk(): void {
  chartSuspension = null
}

class TestResizeObserver implements ResizeObserver {
  readonly disconnect = vi.fn()
  readonly observe = vi.fn()
  readonly unobserve = vi.fn()

  constructor(_callback: ResizeObserverCallback) {}
}

beforeEach(async () => {
  await import('@/components/shared/UsageChart')
  Element.prototype.scrollIntoView = vi.fn()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: TestResizeObserver,
    })
  }
})

function tokens(
  overrides: Partial<RequestRecordOverviewTokenUsage> = {},
): RequestRecordOverviewTokenUsage {
  return {
    cache_hit_rate: 0.25,
    cache_rate: 0.456,
    cache_read_tokens: 200,
    cache_write_tokens: 80,
    input_tokens: 1200,
    output_tokens: 300,
    total_tokens: 1500,
    ...overrides,
  }
}

function breakdownRow(
  overrides: Partial<RequestRecordOverviewBreakdownRow> = {},
): RequestRecordOverviewBreakdownRow {
  return {
    avg_output_tokens_per_second: 42.56,
    error_count: 1,
    error_rate: 0.083,
    label: 'gpt-4o',
    model: 'gpt-4o',
    request_count: 12,
    request_share: 0.75,
    success_count: 11,
    success_rate: 0.916,
    token_share: 0.6,
    tokens: tokens(),
    ...overrides,
  }
}

function overview(
  overrides: Partial<RequestRecordOverviewResponse> = {},
): RequestRecordOverviewResponse {
  return {
    breakdown: [breakdownRow()],
    summary: {
      avg_output_tokens_per_second: 42.56,
      error_count: 1,
      method_count: 3,
      p95_first_token_ms: 800,
      p95_total_ms: 65000,
      request_count: 12,
      success_count: 11,
      success_rate: 0.916,
      tokens: tokens(),
    },
    trend: [
      {
        bucket_at: '2026-10-20T00:00:00.000Z',
        error_count: 1,
        error_rate: 0.083,
        p95_first_token_ms: 800,
        p95_total_ms: 65000,
        request_count: 12,
        success_count: 11,
        success_rate: 0.916,
        tokens: tokens(),
      },
    ],
    trend_granularity: 'day',
    ...overrides,
  }
}

type Props = {
  overview: RequestRecordOverviewResponse | null
  loading?: boolean
  category?: UsageCategory
  scope?: UsageScope
  perspective?: UsagePerspective
  perspectivePending?: boolean
  onDrilldown?: (drilldown: UsageOverviewDrilldown) => void
  onChangePerspective?: (perspective: UsagePerspective) => void
  onChangeScope?: (scope: UsageScope) => void
}

async function renderPanel(props: Props) {
  const view = render(
    <LocaleProvider>
      <ThemeProvider>
        <RequestRecordsOverviewPanel
          category={props.category ?? 'ai'}
          loading={props.loading ?? false}
          onChangePerspective={props.onChangePerspective ?? vi.fn()}
          onChangeScope={props.onChangeScope ?? vi.fn()}
          onDrilldown={props.onDrilldown ?? vi.fn()}
          overview={props.overview}
          perspective={props.perspective ?? 'model'}
          perspectivePending={props.perspectivePending ?? false}
          scope={props.scope ?? 'service'}
        />
      </ThemeProvider>
    </LocaleProvider>,
  )
  await act(async () => {})
  return view
}

function cards(): { label: string; value: string }[] {
  return Array.from(document.querySelectorAll('[data-usage-metric]')).map(
    (card) => ({
      label: card.getAttribute('data-usage-metric') ?? '',
      value: card.querySelector('div:last-child')?.textContent ?? '',
    }),
  )
}

async function chooseOption(triggerName: string, optionName: string) {
  fireEvent.click(screen.getByRole('combobox', { name: triggerName }))
  fireEvent.click(await screen.findByRole('option', { name: optionName }))
}

function rowByLabel(label: string): HTMLTableRowElement {
  return document.querySelector(
    `[data-usage-breakdown-row="${label}"]`,
  ) as HTMLTableRowElement
}

describe('request records overview: AI cards', () => {
  it('shows every AI metric with its formatted value', async () => {
    await renderPanel({ overview: overview() })
    expect(cards()).toEqual([
      { label: copy.overviewTotalTokens, value: '1.5K' },
      { label: copy.overviewInputTokens, value: '1.2K' },
      { label: copy.overviewOutputTokens, value: '300' },
      { label: copy.overviewCacheRate, value: '45.6%' },
      { label: copy.overviewCacheHitRate, value: '25%' },
      { label: copy.overviewAvgOutputRate, value: '42.6 token/s' },
      { label: copy.requests, value: '12' },
      { label: copy.overviewSuccessRate, value: '91.6%' },
      { label: copy.overviewErrors, value: '1' },
      { label: copy.overviewP95Latency, value: '1m 5s' },
    ])
  })

  it('mounts the lazily loaded trend chart host', async () => {
    await renderPanel({ overview: overview() })
    expect(document.querySelector('[data-usage-chart]')).toBeTruthy()
  })

  it('dashes an unknown metric and keeps an exact zero', async () => {
    await renderPanel({
      overview: overview({
        summary: {
          avg_output_tokens_per_second: null,
          error_count: 0,
          method_count: 0,
          p95_total_ms: null,
          request_count: 0,
          success_count: 0,
          success_rate: 0,
          tokens: tokens({ cache_hit_rate: null, cache_rate: 0 }),
        },
        breakdown: [],
        trend: [],
      }),
    })
    const shown = cards()
    expect(shown).toEqual([])
    expect(screen.getByText(copy.noAiRequestRecords)).toBeTruthy()
  })
})

describe('request records overview: MCP cards', () => {
  it('shows the five MCP metrics and no perspective or distribution select', async () => {
    await renderPanel({
      category: 'mcp',
      overview: overview({
        summary: {
          avg_output_tokens_per_second: null,
          error_count: 2,
          method_count: 4,
          p95_total_ms: 1500,
          request_count: 9,
          success_count: 7,
          success_rate: 0.777,
          tokens: tokens(),
        },
        breakdown: [
          breakdownRow({
            label: 'files',
            server_provider_kind: 'context7',
            usage_unit: 'requests',
          }),
        ],
      }),
    })
    expect(cards()).toEqual([
      { label: copy.requests, value: '9' },
      { label: copy.overviewSuccessRate, value: '77.7%' },
      { label: copy.overviewErrors, value: '2' },
      { label: copy.overviewP95Latency, value: '1.5s' },
      { label: copy.overviewMcpMethods, value: '4' },
    ])
    expect(
      screen.queryByRole('combobox', { name: copy.overviewPerspective }),
    ).toBeNull()
    expect(screen.getByRole('combobox', { name: copy.mcpScope })).toBeTruthy()
    expect(screen.getByText(copy.overviewMcpServerDistribution)).toBeTruthy()
    expect(screen.getByText('context7 · requests')).toBeTruthy()
  })
})

describe('request records overview: states', () => {
  it('reports loading before the first response', async () => {
    await renderPanel({ overview: null, loading: true })
    expect(screen.getByText(copy.loading)).toBeTruthy()
  })

  it('reports the MCP empty state without traffic', async () => {
    await renderPanel({
      category: 'mcp',
      overview: overview({
        summary: {
          avg_output_tokens_per_second: null,
          error_count: 0,
          method_count: 0,
          p95_total_ms: null,
          request_count: 0,
          success_count: 0,
          success_rate: 0,
          tokens: tokens({ total_tokens: 0 }),
        },
        breakdown: [],
        trend: [],
      }),
    })
    expect(screen.queryByText(copy.overviewMcpServerDistribution)).toBeNull()
    expect(screen.getByText(copy.noMcpCallRecords)).toBeTruthy()
  })

  it('keeps the perspective control reachable for an empty AI window', async () => {
    await renderPanel({
      overview: overview({
        summary: {
          avg_output_tokens_per_second: null,
          error_count: 0,
          method_count: 0,
          p95_total_ms: null,
          request_count: 0,
          success_count: 0,
          success_rate: 0,
          tokens: tokens({ total_tokens: 0 }),
        },
        breakdown: [],
        trend: [],
      }),
    })
    expect(screen.getByText(copy.noAiRequestRecords)).toBeTruthy()
  })
})

describe('request records overview: controls', () => {
  it('disables the perspective select while a change is pending', async () => {
    await renderPanel({ overview: overview(), perspectivePending: true })
    const select = screen.getByRole('combobox', {
      name: copy.overviewPerspective,
    })
    expect(select.getAttribute('aria-busy')).toBe('true')
    expect((select as HTMLButtonElement).disabled).toBe(true)
  })

  it('reports a perspective change', async () => {
    const onChangePerspective = vi.fn()
    await renderPanel({ overview: overview(), onChangePerspective })
    await chooseOption(
      copy.overviewPerspective,
      copy.overviewPerspectiveUpstream,
    )
    expect(onChangePerspective).toHaveBeenCalledWith('upstream')
  })

  it('reports an MCP scope change', async () => {
    const onChangeScope = vi.fn()
    await renderPanel({ category: 'mcp', overview: overview(), onChangeScope })
    await chooseOption(copy.mcpScope, copy.mcpScopeGateway)
    expect(onChangeScope).toHaveBeenCalledWith('gateway')
  })
})

describe('request records overview: distribution', () => {
  it('titles the model distribution and drills into a model row', async () => {
    const onDrilldown = vi.fn()
    await renderPanel({ overview: overview(), onDrilldown })
    expect(screen.getByText(copy.overviewModelDistribution)).toBeTruthy()

    fireEvent.click(rowByLabel('gpt-4o'))
    expect(onDrilldown).toHaveBeenCalledWith({
      model: 'gpt-4o',
      mcp_server_id: null,
    })
  })

  it('drills into a model row with the keyboard', async () => {
    const onDrilldown = vi.fn()
    await renderPanel({ overview: overview(), onDrilldown })
    const row = rowByLabel('gpt-4o')
    expect(row.tabIndex).toBe(0)
    fireEvent.keyDown(row, { key: 'Enter' })
    expect(onDrilldown).toHaveBeenCalledWith({
      model: 'gpt-4o',
      mcp_server_id: null,
    })
  })

  it('titles the upstream distribution and keeps a direct upstream inert', async () => {
    const onDrilldown = vi.fn()
    await renderPanel({
      overview: overview({
        breakdown: [
          breakdownRow({
            endpoint_id: null,
            label: '(direct)',
            model: 'gpt-4o',
          }),
        ],
      }),
      perspective: 'upstream',
      onDrilldown,
    })
    expect(screen.getByText(copy.overviewUpstreamDistribution)).toBeTruthy()
    const row = rowByLabel('(direct)')
    expect(row.getAttribute('data-drilldown')).toBe('false')
    expect(row.tabIndex).toBe(-1)
    fireEvent.click(row)
    expect(onDrilldown).not.toHaveBeenCalled()
  })

  it('drills into a real upstream endpoint', async () => {
    const onDrilldown = vi.fn()
    await renderPanel({
      overview: overview({
        breakdown: [
          breakdownRow({
            endpoint_id: 'endpoint-1',
            label: 'east-1',
            model: 'gpt-4o',
          }),
        ],
      }),
      perspective: 'upstream',
      onDrilldown,
    })
    fireEvent.click(rowByLabel('east-1'))
    expect(onDrilldown).toHaveBeenCalledWith({ endpoint_id: 'endpoint-1' })
  })

  it('maps the MCP gateway row to the gateway scope', async () => {
    const onDrilldown = vi.fn()
    await renderPanel({
      category: 'mcp',
      overview: overview({
        breakdown: [breakdownRow({ label: 'gateway', mcp_scope: 'gateway' })],
      }),
      onDrilldown,
    })
    fireEvent.click(rowByLabel('gateway'))
    expect(onDrilldown).toHaveBeenCalledWith({
      mcp_scope: 'gateway',
      mcp_server_id: null,
      model: null,
    })
  })

  it('maps a named MCP server to the service scope', async () => {
    const onDrilldown = vi.fn()
    await renderPanel({
      category: 'mcp',
      overview: overview({
        breakdown: [
          breakdownRow({
            label: 'files',
            mcp_scope: 'service',
            mcp_server_id: 'server-1',
          }),
        ],
      }),
      onDrilldown,
    })
    fireEvent.click(rowByLabel('files'))
    expect(onDrilldown).toHaveBeenCalledWith({
      mcp_scope: 'service',
      mcp_server_id: 'server-1',
      model: null,
    })
  })

  it('maps a name-only MCP row to a label search in the service scope', async () => {
    const onDrilldown = vi.fn()
    await renderPanel({
      category: 'mcp',
      overview: overview({
        breakdown: [
          breakdownRow({
            label: 'legacy',
            mcp_scope: 'service',
            mcp_server_id: null,
          }),
        ],
      }),
      onDrilldown,
    })
    fireEvent.click(rowByLabel('legacy'))
    expect(onDrilldown).toHaveBeenCalledWith({
      mcp_scope: 'service',
      mcp_server_id: null,
      model: 'legacy',
    })
  })

  it('does not drill when the click starts inside the row controls', async () => {
    const onDrilldown = vi.fn()
    await renderPanel({
      overview: overview({
        breakdown: [
          breakdownRow({
            label: 'gpt-4o',
            upstream_count: 2,
            upstream_breakdown: [
              {
                avg_output_tokens_per_second: 1,
                cache_rate: 0.1,
                endpoint_id: 'a',
                endpoint_name: 'a-1',
                error_count: 0,
                error_rate: 0,
                request_count: 3,
                request_share: 0.3,
                token_share: 0.3,
                total_tokens: 10,
                upstream_model: 'm-a',
              },
              {
                avg_output_tokens_per_second: 1,
                cache_rate: 0.1,
                endpoint_id: 'b',
                endpoint_name: 'b-1',
                error_count: 0,
                error_rate: 0,
                request_count: 3,
                request_share: 0.3,
                token_share: 0.3,
                total_tokens: 10,
                upstream_model: 'm-b',
              },
            ],
          }),
        ],
      }),
      onDrilldown,
    })
    const row = rowByLabel('gpt-4o')
    const trigger = within(row).getByRole('button', {
      name: copy.overviewUpstreamBreakdown,
    })
    fireEvent.click(trigger)
    expect(onDrilldown).not.toHaveBeenCalled()
  })
})

describe('request records overview: chart suspension', () => {
  it('commits the metrics and the distribution while the chart chunk is pending', async () => {
    suspendChartChunk()
    try {
      await renderPanel({ overview: overview() })
      expect(document.querySelectorAll('[data-usage-metric]')).toHaveLength(10)
      expect(rowByLabel('gpt-4o')).toBeTruthy()
      expect(
        document.querySelector('[data-usage-chart-placeholder]'),
      ).toBeTruthy()
      expect(document.querySelector('[data-usage-chart]')).toBeNull()
    } finally {
      releaseChartChunk()
    }
  })

  it('shows the trend chart once the chunk resolves', async () => {
    const suspension = suspendChartChunk()
    try {
      await renderPanel({ overview: overview() })
      expect(
        document.querySelector('[data-usage-chart-placeholder]'),
      ).toBeTruthy()
      await act(async () => {
        suspension.resolve()
      })
      expect(document.querySelector('[data-usage-chart]')).toBeTruthy()
      expect(
        document.querySelector('[data-usage-chart-placeholder]'),
      ).toBeNull()
    } finally {
      releaseChartChunk()
    }
  })
})
