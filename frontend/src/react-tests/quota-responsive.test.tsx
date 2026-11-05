import { render, screen, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  EndpointProvider,
  TokenPlanUsageResponse,
} from '@/generated/admin-api'
import type { CanonicalQuotaRow } from '@/composables/useCanonicalQuotaWindows'

const sdkMock = vi.hoisted(() => ({
  tokenPlanUsage: vi.fn(),
  quotaKeyObservationHistory: vi.fn(),
  quotaKeyObservationTrend: vi.fn(),
}))

vi.mock('@/generated/admin-api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/generated/admin-api')>()),
  tokenPlanUsage: sdkMock.tokenPlanUsage,
  quotaKeyObservationHistory: sdkMock.quotaKeyObservationHistory,
  quotaKeyObservationTrend: sdkMock.quotaKeyObservationTrend,
}))

import { __resetTokenPlanCacheForTests } from '@/lib/quota-cache'
import { LocaleProvider } from '@/lib/i18n'
import { TokenPlanUsageDialog } from '@/components/endpoints/TokenPlanUsageDialog'
import { TokenPlanQuotaWindows } from '@/components/endpoints/TokenPlanQuotaWindows'
import { TokenPlanQuotaWindowRow } from '@/components/endpoints/TokenPlanQuotaWindowRow'

const NOW = Date.parse('2026-10-24T00:00:00.000Z')

function quotaRow(
  overrides: Partial<CanonicalQuotaRow> = {},
): CanonicalQuotaRow {
  return {
    sourceId: 'model-a:primary',
    kind: 'custom',
    sourceModel: 'Default model',
    unit: 'USD',
    durationSeconds: 18_000,
    isDefaultScope: true,
    availability: 'known',
    observedRemainingPercent: 40,
    remainingPercent: 40,
    current: 38,
    limit: 100,
    remaining: 38,
    resetAt: '2026-10-24T01:00:00.000Z',
    expired: false,
    ...overrides,
  }
}

function usagePayload(): TokenPlanUsageResponse {
  return {
    keys: [
      {
        key_id: 'responsive-key',
        key_label: 'Responsive key',
        ok: true,
        model_remains: [],
      },
    ],
    provider: 'minimax',
    provider_region: null,
  }
}

function renderDialog(provider: EndpointProvider = 'minimax') {
  return render(
    <LocaleProvider>
      <TokenPlanUsageDialog
        endpointId="endpoint-responsive"
        endpointName="Responsive upstream"
        onError={() => undefined}
        onOpenChange={() => undefined}
        open
        provider={provider}
      />
    </LocaleProvider>,
  )
}

function dialogContent(): HTMLElement {
  return document.querySelector('[data-slot="dialog-content"]') as HTMLElement
}

beforeEach(() => {
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  vi.setSystemTime(NOW)
  sdkMock.tokenPlanUsage.mockResolvedValue({ data: usagePayload() })
  sdkMock.quotaKeyObservationHistory.mockResolvedValue({
    data: { items: [], retention_days: 30 },
  })
  sdkMock.quotaKeyObservationTrend.mockResolvedValue({
    data: {
      bucket_count: 0,
      bucket_seconds: 0,
      failed_observations: 0,
      from: '',
      keys_truncated: false,
      range: '24h',
      sampled_observations: 0,
      series: [],
      series_truncated: false,
      to: '',
    },
  })
  __resetTokenPlanCacheForTests()
})

afterEach(() => {
  __resetTokenPlanCacheForTests()
  vi.useRealTimers()
  localStorage.clear()
})

describe('quota dialog responsive shell', () => {
  it('overrides the shared dialog width cap and keeps the viewport margin', async () => {
    renderDialog()
    await screen.findByText('Responsive key')
    const dialog = dialogContent()
    expect(dialog.className).toContain(
      'sm:max-w-[min(calc(100vw_-_2rem),72rem)]',
    )
    expect(dialog.className).not.toContain('max-w-6xl')
    expect(dialog.className).not.toContain('sm:max-w-lg')
  })

  it('keeps one scroll region below a fixed header instead of nested scroll areas', async () => {
    renderDialog()
    await screen.findByText('Responsive key')
    const dialog = dialogContent()
    expect(dialog.className).toContain('flex')
    expect(dialog.className).toContain('flex-col')
    expect(dialog.className).toContain('overflow-hidden')
    expect(dialog.className).toContain('max-h-[calc(100dvh-2rem)]')
    const header = dialog.querySelector('[data-slot="dialog-header"]')
    expect(header?.className).toContain('shrink-0')
    const scroller = dialog.querySelector(
      '[data-quota-dialog="endpoint-responsive"]',
    ) as HTMLElement
    expect(scroller.className).toContain('flex-1')
    expect(scroller.className).toContain('min-h-0')
    expect(scroller.className).toContain('overflow-y-auto')
    expect(scroller.className).not.toContain('max-h-[min(70vh,42rem)]')
    expect(dialog.querySelectorAll('[class*="overflow-y-auto"]')).toHaveLength(
      1,
    )
  })
})

describe('quota window rows adapt to their container', () => {
  it('renders a tall remaining bar with compact slot and countdown', () => {
    render(
      <LocaleProvider>
        <TokenPlanQuotaWindowRow
          label="Weekly"
          nowMs={NOW}
          quantitySuffix=" USD"
          row={quotaRow()}
        />
      </LocaleProvider>,
    )
    const row = document.querySelector(
      '[data-quota-row="model-a:primary"]',
    ) as HTMLElement
    expect(row.className).toContain('@container')
    expect(row.textContent).toContain('5h')
    expect(row.textContent).toContain('40.0%')
    expect(row.textContent).toContain('1h')
    expect(row.textContent).toContain('38.00 / 100.00 USD')
    const bar = row.querySelector('[role="progressbar"]') as HTMLElement
    expect(bar.getAttribute('aria-valuenow')).toBe('40')
    expect(bar.getAttribute('aria-label')).toBe('Weekly')
    expect(bar.className).toContain('h-8')
    expect(bar.className).toContain('min-h-8')
    expect(bar.className).toContain('w-full')
  })

  it('renders localized compact window labels and countdown in Chinese', () => {
    localStorage.setItem('prompt-ferry:locale', 'zh-CN')
    render(
      <LocaleProvider>
        <TokenPlanQuotaWindowRow label="Weekly" nowMs={NOW} row={quotaRow()} />
      </LocaleProvider>,
    )
    const row = document.querySelector(
      '[data-quota-row="model-a:primary"]',
    ) as HTMLElement
    expect(row.textContent).toContain('5小时')
    expect(row.textContent).toContain('1小时')
    expect(row.textContent).not.toContain('5h')
    expect(row.textContent).not.toContain('1h')
  })

  it('keeps an unknown percentage as a placeholder without a fake progress bar', () => {
    render(
      <LocaleProvider>
        <TokenPlanQuotaWindowRow
          label="Weekly"
          nowMs={NOW}
          row={quotaRow({
            availability: 'unknown',
            observedRemainingPercent: null,
            remainingPercent: null,
            resetAt: '2026-10-24T01:00:00.000Z',
            current: null,
            limit: null,
          })}
        />
      </LocaleProvider>,
    )
    const row = document.querySelector(
      '[data-quota-row="model-a:primary"]',
    ) as HTMLElement
    expect(row.querySelector('[role="progressbar"]')).toBeNull()
    expect(row.textContent).toContain('—')
    expect(row.textContent).toContain('-')
    expect(row.textContent).not.toContain('1h')
    expect(row.className).toContain('@container')
  })

  it('keeps custom window identity from its measured duration', () => {
    render(
      <LocaleProvider>
        <TokenPlanQuotaWindowRow
          label="Fallback label"
          nowMs={NOW}
          row={quotaRow({
            kind: 'custom',
            durationSeconds: 30 * 24 * 60 * 60,
          })}
        />
      </LocaleProvider>,
    )
    const row = document.querySelector(
      '[data-quota-row="model-a:primary"]',
    ) as HTMLElement
    expect(row.textContent).toContain('30d')
    expect(row.textContent).not.toContain('Fallback label')
  })

  it('lays a balance row out in one column until its container has room', () => {
    render(
      <LocaleProvider>
        <TokenPlanQuotaWindowRow
          label="Balance"
          nowMs={NOW}
          row={quotaRow({
            sourceId: 'deepseek:USD',
            kind: 'balance',
            remainingPercent: null,
            observedRemainingPercent: null,
          })}
        />
      </LocaleProvider>,
    )
    const row = document.querySelector(
      '[data-quota-row="deepseek:USD"]',
    ) as HTMLElement
    expect(row.className).toContain('@container')
    const grid = row.firstElementChild as HTMLElement
    expect(grid.className).toContain('@[20rem]:grid-cols-')
    expect(grid.className).not.toContain('sm:grid-cols')
    expect(row.textContent).toContain('38.00 / 100.00')
  })
})

describe('shared quota window grid', () => {
  function keyUsage(
    windows: CanonicalQuotaRow[],
    modelRemains: TokenPlanUsageResponse['keys'][number]['model_remains'] = [],
  ): TokenPlanUsageResponse['keys'][number] {
    return {
      key_id: 'responsive-key',
      key_label: 'Responsive key',
      ok: true,
      model_remains: modelRemains,
      quota_windows: windows.map((row) => ({
        source_id: row.sourceId,
        kind: row.kind,
        source_model: row.sourceModel,
        unit: row.unit,
        duration_seconds: row.durationSeconds,
        is_default_scope: row.isDefaultScope,
        availability: row.availability,
        remaining_percent: row.remainingPercent,
        current: row.current,
        limit: row.limit,
        remaining: row.remaining,
        reset_at: row.resetAt,
      })),
    } as TokenPlanUsageResponse['keys'][number]
  }

  it('lays actual windows out with an auto-fit grid and omits a single default heading', () => {
    const { container } = render(
      <LocaleProvider>
        <TokenPlanQuotaWindows
          keyUsage={keyUsage(
            [
              quotaRow({ sourceId: 'minimax:primary', sourceModel: 'general' }),
              quotaRow({
                sourceId: 'minimax:weekly',
                sourceModel: 'general',
                kind: 'weekly',
                durationSeconds: 604_800,
              }),
            ],
            [
              {
                model_name: 'general',
                interval: null,
                weekly: null,
                observation: {
                  observed_at: '2026-10-24T00:00:00.000Z',
                  source: 'request',
                  refreshing: false,
                  stale: false,
                  last_error_code: null,
                  next_retry_at: null,
                },
              },
            ],
          )}
          nowMs={NOW}
        />
      </LocaleProvider>,
    )
    const section = container.querySelector(
      '[data-quota-model="general"]',
    ) as HTMLElement
    expect(section).toBeTruthy()
    expect(within(section).queryByText('general')).toBeNull()
    const grid = section.lastElementChild as HTMLElement
    expect(grid.className).toContain('auto-fit')
    expect(grid.className).toContain('minmax(min(100%,12rem),1fr)')
    expect(section.querySelectorAll('[data-quota-row]')).toHaveLength(2)
  })

  it('names each progressbar with its model and real window identity', () => {
    const { container } = render(
      <LocaleProvider>
        <TokenPlanQuotaWindows
          keyUsage={keyUsage([
            quotaRow({
              sourceId: 'minimax:general:interval',
              sourceModel: 'general',
            }),
            quotaRow({
              sourceId: 'minimax:general:weekly',
              sourceModel: 'general',
              kind: 'weekly',
              durationSeconds: 604_800,
            }),
            quotaRow({
              sourceId: 'opencode_go:monthly',
              sourceModel: 'zen',
              kind: 'monthly',
              durationSeconds: 2_592_000,
            }),
          ])}
          nowMs={NOW}
        />
      </LocaleProvider>,
    )
    const labels = [...container.querySelectorAll('[role="progressbar"]')].map(
      (bar) => bar.getAttribute('aria-label'),
    )
    expect(labels).toEqual([
      'general · 5-hour window',
      'general · Weekly window',
      'zen · Monthly window',
    ])
    expect(new Set(labels).size).toBe(3)
  })

  it('keeps actionable observation states even without window rows', () => {
    const { container } = render(
      <LocaleProvider>
        <TokenPlanQuotaWindows
          keyUsage={keyUsage(
            [],
            [
              {
                model_name: 'gpt-5.1-codex-mini',
                interval: null,
                weekly: null,
                windows: null,
                observation: {
                  observed_at: '2026-10-24T00:00:00.000Z',
                  source: 'request',
                  refreshing: false,
                  stale: true,
                  last_error_code: 'fixture_refresh_failed',
                  next_retry_at: '2026-10-24T01:00:00.000Z',
                },
              },
            ],
          )}
          nowMs={NOW}
        />
      </LocaleProvider>,
    )
    expect(container.textContent).toContain('Cached observation')
    expect(container.textContent).toContain(
      'Refresh failed; showing the latest successful observation.',
    )
    expect(container.textContent).toContain('Next retry:')
    expect(container.querySelectorAll('[data-quota-row]')).toHaveLength(0)
  })

  it('keeps distinct multiple model identities and hides minmax video rows', () => {
    const { container } = render(
      <LocaleProvider>
        <TokenPlanQuotaWindows
          keyUsage={keyUsage([
            quotaRow({
              sourceId: 'minimax:general:interval',
              sourceModel: 'general',
            }),
            quotaRow({
              sourceId: 'minimax:detail:interval',
              sourceModel: 'detail-model',
              isDefaultScope: false,
            }),
            quotaRow({
              sourceId: 'minimax:video:interval',
              sourceModel: 'video',
              isDefaultScope: false,
            }),
          ])}
          nowMs={NOW}
        />
      </LocaleProvider>,
    )
    expect(screen.getByText('general')).toBeTruthy()
    expect(screen.getByText('detail-model')).toBeTruthy()
    expect(screen.queryByText('video')).toBeNull()
    expect(
      container.querySelector('[data-quota-row="minimax:video:interval"]'),
    ).toBeNull()
    expect(container.querySelectorAll('[data-quota-model]')).toHaveLength(2)
  })
})
