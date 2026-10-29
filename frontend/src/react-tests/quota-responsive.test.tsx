import { render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  EndpointProvider,
  TokenPlanUsageResponse,
} from '@/generated/admin-api'
import type { CanonicalQuotaRow } from '@/composables/useCanonicalQuotaWindows'

const sdkMock = vi.hoisted(() => ({
  tokenPlanUsage: vi.fn(),
  quotaSnapshotHistory: vi.fn(),
  quotaKeyObservationHistory: vi.fn(),
  quotaKeyObservationTrend: vi.fn(),
}))

vi.mock('@/generated/admin-api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/generated/admin-api')>()),
  tokenPlanUsage: sdkMock.tokenPlanUsage,
  quotaSnapshotHistory: sdkMock.quotaSnapshotHistory,
  quotaKeyObservationHistory: sdkMock.quotaKeyObservationHistory,
  quotaKeyObservationTrend: sdkMock.quotaKeyObservationTrend,
}))

import { __resetTokenPlanCacheForTests } from '@/lib/quota-cache'
import { LocaleProvider } from '@/lib/i18n'
import { TokenPlanUsageDialog } from '@/components/endpoints/TokenPlanUsageDialog'
import { SubscriptionQuotaWindows } from '@/components/endpoints/SubscriptionQuotaWindows'
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
  sdkMock.quotaSnapshotHistory.mockResolvedValue({
    data: { items: [], retention_days: 30 },
  })
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
  it('stacks the time window row until the row container is wide enough', () => {
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
    const grid = row.firstElementChild as HTMLElement
    expect(grid.className).not.toContain('sm:grid-cols')
    expect(grid.className).toContain('@[26rem]:grid-cols-')
    expect(grid.className).toContain('@[26rem]:items-center')
    expect(
      row.querySelector('[role="progressbar"]')?.getAttribute('aria-valuenow'),
    ).toBe('40')
    expect(row.textContent).toContain('Remaining 40.0%')
    expect(row.textContent).toContain('38.00 / 100.00 USD')
    expect(
      (row.querySelector('[role="progressbar"]') as HTMLElement).className,
    ).toContain('w-full')
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
            resetAt: null,
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
    expect(row.className).toContain('@container')
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

describe('subscription quota windows container layout', () => {
  it('uses its own width for the model card grid instead of the viewport', () => {
    render(
      <LocaleProvider>
        <SubscriptionQuotaWindows
          groups={[
            {
              modelName: 'Default model',
              observation: {
                observed_at: '2026-10-24T00:00:00.000Z',
                source: 'request',
                refreshing: false,
                stale: false,
                last_error_code: null,
                next_retry_at: null,
              },
              rows: [quotaRow()],
            },
            {
              modelName: 'Detail model',
              observation: null,
              rows: [
                quotaRow({
                  sourceId: 'model-b:detail',
                  sourceModel: 'Detail model',
                }),
              ],
            },
          ]}
          nowMs={NOW}
        />
      </LocaleProvider>,
    )
    const grid = document.querySelector('[data-quota-model="Default model"]')
      ?.parentElement as HTMLElement
    expect(grid.className).toContain('@container')
    expect(grid.className).toContain('grid-cols-[minmax(0,1fr)]')
    expect(grid.className).toContain('@[40rem]:grid-cols-2')
    expect(grid.className).not.toContain('sm:grid-cols-2')
    expect(document.querySelectorAll('[data-quota-model]')).toHaveLength(2)
    expect(screen.getByText('Detail model')).toBeTruthy()
  })
})
