import { act, fireEvent, render, screen, within } from '@testing-library/react'
import { UsageRangePicker } from '@/components/usage/UsageRangePicker'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { RequestRecordListRow } from '@/generated/admin-api'
import { RequestRecordsTable } from '@/components/usage/RequestRecordsTable'
import { messages } from '@/i18n'
import { LocaleProvider } from '@/lib/i18n'
import { setDisplayTimeZone } from '@/lib/display-timezone'

const copy = messages['en-US']

class TestResizeObserver implements ResizeObserver {
  readonly disconnect = vi.fn()
  readonly observe = vi.fn()
  readonly unobserve = vi.fn()

  constructor(_callback: ResizeObserverCallback) {}
}

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  setDisplayTimeZone('UTC')
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: TestResizeObserver,
    })
  }
})

describe('custom range validation', () => {
  it('updates display zone without changing committed instants', () => {
    const apply = vi.fn()
    render(
      <LocaleProvider>
        <UsageRangePicker
          range="custom"
          start="2026-10-01T00:00:00.000Z"
          end="2026-10-01T02:00:00.000Z"
          onApply={apply}
        />
      </LocaleProvider>,
    )
    const trigger = screen.getByRole('button', { name: copy.timeRange })
    expect(trigger.textContent).toBe('2026-10-01 00:00 – 2026-10-01 02:00')
    act(() => setDisplayTimeZone('Asia/Tokyo'))
    expect(trigger.textContent).toBe('2026-10-01 09:00 – 2026-10-01 11:00')
    expect(apply).not.toHaveBeenCalled()
  })

  it.each([
    ['2026-03-08T05:00:00.000Z', '2026-03-09T04:00:00.000Z', '02:30'],
    ['2026-11-01T04:00:00.000Z', '2026-11-02T05:00:00.000Z', '01:30'],
  ])(
    'rejects nonexistent or ambiguous wall time for %s',
    (start, end, time) => {
      setDisplayTimeZone('America/New_York')
      const apply = vi.fn()
      render(
        <LocaleProvider>
          <UsageRangePicker
            range="custom"
            start={start}
            end={end}
            onApply={apply}
          />
        </LocaleProvider>,
      )
      fireEvent.click(screen.getByRole('button', { name: copy.timeRange }))
      const popup = document.querySelector(
        '[data-slot="popover-content"]',
      ) as HTMLElement
      fireEvent.change(within(popup).getByLabelText(copy.startTime), {
        target: { value: time },
      })
      fireEvent.click(
        within(popup).getByRole('button', { name: copy.applyTimeRange }),
      )
      expect(within(popup).getByRole('alert').textContent).toBe(
        copy.invalidTimeRange,
      )
      expect(apply).not.toHaveBeenCalled()
    },
  )
})

function record(
  overrides: Partial<RequestRecordListRow> = {},
): RequestRecordListRow {
  return {
    conversation_source: 'codex',
    created_at: '2026-03-04T23:30:00.000Z',
    has_full_request: false,
    has_parent: false,
    path: '/v1/chat/completions',
    record_id: 1,
    redaction: {
      applied: false,
      fields: [],
      findings_count: 0,
      replacements_count: 0,
      types: [],
    },
    request_category: 'ai',
    request_id: 'req-1',
    request_state: 'completed',
    route_selection_reason: 'default',
    storage_sanitized: false,
    storage_sanitized_nul_count: 0,
    ...overrides,
  }
}

function renderRows(records: RequestRecordListRow[]) {
  return render(
    <LocaleProvider>
      <RequestRecordsTable
        category="ai"
        first={0}
        isAdmin
        loading={false}
        onOpenDetail={vi.fn()}
        onPage={vi.fn()}
        onSort={vi.fn()}
        records={records}
        rows={25}
        sortField="created_at"
        sortOrder={-1}
        total={records.length}
      />
    </LocaleProvider>,
  )
}

function rateRecord(rate: number): RequestRecordListRow {
  return record({ duration_ms: 10000, output_tokens: rate * 10 })
}

describe('usage table visual hierarchy', () => {
  it('bands the output rate without changing the existing precision', () => {
    renderRows([rateRecord(10), rateRecord(30), rateRecord(60)])
    const cells = Array.from(
      document.querySelectorAll('[data-usage-cell="throughput"]'),
    ) as HTMLElement[]
    expect(cells).toHaveLength(3)
    expect(cells[0]?.textContent).toBe('10.0')
    expect(cells[1]?.textContent).toBe('30.0')
    expect(cells[2]?.textContent).toBe('60.0')
    const bands = cells.map((cell) =>
      cell
        .querySelector('[data-usage-rate-band]')
        ?.getAttribute('data-usage-rate-band'),
    )
    expect(bands).toEqual(['slow', 'medium', 'fast'])
    expect(
      cells[0]?.querySelector('[data-usage-rate-band]')?.className,
    ).toContain('bg-red-100')
    expect(
      cells[2]?.querySelector('[data-usage-rate-band]')?.className,
    ).toContain('bg-emerald-100')
    expect(
      cells[1]?.querySelector('[data-usage-rate-band]')?.className,
    ).toContain('bg-muted')
  })

  it('keeps a missing rate a plain dash without a band', () => {
    renderRows([record({ request_state: 'failed', output_tokens: 100 })])
    const cell = document.querySelector(
      '[data-usage-cell="throughput"]',
    ) as HTMLElement
    expect(cell.textContent).toBe('-')
    expect(cell.querySelector('[data-usage-rate-band]')).toBeNull()
  })

  it('de-emphasizes only the upstream parentheses of the model display', () => {
    renderRows([
      record({
        model: 'gpt-4o',
        requested_model: 'gpt-4o',
        upstream_model: 'gpt-4o-mini',
      }),
    ])
    const cell = document.querySelector(
      '[data-usage-cell="model"]',
    ) as HTMLElement
    expect(cell.textContent).toBe('gpt-4o (gpt-4o-mini)')
    const outer = cell.firstElementChild as HTMLElement
    expect(outer.className).toContain('font-semibold')
    expect(outer.getAttribute('title')).toBe('gpt-4o (gpt-4o-mini)')
    const brackets = outer.querySelector('span') as HTMLElement
    expect(brackets.textContent).toBe(' (gpt-4o-mini)')
    expect(brackets.className).toContain('text-muted-foreground')
  })

  it('does not split parentheses that belong to the requested model', () => {
    renderRows([
      record({
        model: 'legacy (preview)',
        requested_model: 'legacy (preview)',
        upstream_model: 'legacy (preview)',
      }),
    ])
    const cell = document.querySelector(
      '[data-usage-cell="model"]',
    ) as HTMLElement
    expect(cell.textContent).toBe('legacy (preview)')
    const outer = cell.firstElementChild as HTMLElement
    expect(outer.querySelector('span')).toBeNull()
  })

  it('keeps the session id and sequence readable but de-emphasized', () => {
    renderRows([
      record({ conversation_id: 'abcdef1234567890', conversation_seq: 3 }),
    ])
    const target = document.querySelector(
      '[data-usage-cell="target"]',
    ) as HTMLElement
    expect(target.textContent).toBe(
      `/v1/chat/completions${copy.session} abcdef12#3`,
    )
    const badges = Array.from(target.querySelectorAll('[data-slot="badge"]'))
    expect(badges).toHaveLength(2)
    for (const badge of badges) {
      expect(badge.className).toContain('text-muted-foreground')
    }
  })
})
