import { fireEvent, render, screen, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  RequestRecordOverviewBreakdownRow,
  RequestRecordOverviewModelBreakdown,
  RequestRecordOverviewUpstreamBreakdown,
} from '@/generated/admin-api'
import { messages } from '@/i18n'
import { LocaleProvider } from '@/lib/i18n'
import { BreakdownDetailPopover } from '@/components/usage/BreakdownDetailPopover'
import type { UsagePerspective } from '@/lib/usage-model'

const copy = messages['en-US']

class TestResizeObserver implements ResizeObserver {
  readonly disconnect = vi.fn()
  readonly observe = vi.fn()
  readonly unobserve = vi.fn()

  constructor(_callback: ResizeObserverCallback) {}
}

beforeEach(() => {
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: TestResizeObserver,
    })
  }
})

function upstream(
  overrides: Partial<RequestRecordOverviewUpstreamBreakdown> = {},
): RequestRecordOverviewUpstreamBreakdown {
  return {
    avg_output_tokens_per_second: 42.56,
    cache_rate: 0.5,
    endpoint_id: 'endpoint-1',
    endpoint_name: 'east-1',
    error_count: 0,
    error_rate: 0,
    request_count: 4,
    request_share: 0.4,
    token_share: 0.6,
    total_tokens: 1200,
    upstream_model: 'gpt-upstream',
    ...overrides,
  }
}

function model(
  overrides: Partial<RequestRecordOverviewModelBreakdown> = {},
): RequestRecordOverviewModelBreakdown {
  return {
    avg_output_tokens_per_second: 12.3,
    cache_rate: 0.25,
    error_count: 1,
    error_rate: 0.1,
    model: 'gpt-requested',
    request_count: 10,
    request_share: 1,
    token_share: 0.8,
    total_tokens: 5000,
    ...overrides,
  }
}

function row(
  overrides: Partial<RequestRecordOverviewBreakdownRow> = {},
): RequestRecordOverviewBreakdownRow {
  return {
    label: 'gpt-4o',
    request_count: 10,
    request_share: 1,
    success_count: 9,
    success_rate: 0.9,
    tokens: {
      cache_read_tokens: 0,
      cache_write_tokens: 0,
      input_tokens: 0,
      output_tokens: 0,
      total_tokens: 5000,
    },
    ...overrides,
  }
}

function deepFreeze<T>(value: T): T {
  if (value && typeof value === 'object') {
    for (const child of Object.values(value)) deepFreeze(child)
    Object.freeze(value)
  }
  return value
}

function renderPopover(
  value: RequestRecordOverviewBreakdownRow,
  perspective: UsagePerspective,
) {
  return render(
    <LocaleProvider>
      <BreakdownDetailPopover perspective={perspective} row={value} />
    </LocaleProvider>,
  )
}

function openPopover(perspective: UsagePerspective) {
  const trigger = screen.getByRole('button', {
    name:
      perspective === 'upstream'
        ? copy.overviewModelBreakdown
        : copy.overviewUpstreamBreakdown,
  })
  fireEvent.click(trigger)
  return document.querySelector('[data-slot="popover-content"]') as HTMLElement
}

function bodyRows(popover: HTMLElement): HTMLTableRowElement[] {
  return within(popover).getAllByRole('row').slice(1) as HTMLTableRowElement[]
}

function headerTexts(popover: HTMLElement): string[] {
  const [header] = within(popover).getAllByRole('row') as HTMLTableRowElement[]
  return within(header)
    .getAllByRole('columnheader')
    .map((cell) => cell.textContent ?? '')
}

function rowTexts(cells: HTMLTableRowElement): string[] {
  return Array.from(cells.querySelectorAll('td')).map(
    (cell) => cell.textContent ?? '',
  )
}

describe('breakdown detail popover: model perspective', () => {
  it('renders the endpoint and model inside the upstream detail with every metric', () => {
    renderPopover(
      row({
        upstream_count: 2,
        upstream_breakdown: [
          upstream(),
          upstream({
            endpoint_id: 'endpoint-2',
            endpoint_name: 'west-1',
            upstream_model: 'gpt-upstream-2',
            request_count: 6,
          }),
        ],
      }),
      'model',
    )
    const popover = openPopover('model')

    expect(headerTexts(popover)).toEqual([
      copy.overviewUpstreamEndpoint,
      copy.model,
      copy.requests,
      copy.overviewRequestShare,
      copy.overviewTotalTokens,
      copy.overviewTokenShare,
      copy.overviewCacheRate,
      copy.overviewErrorRate,
      copy.overviewAvgOutputRate,
    ])
    expect(bodyRows(popover)).toHaveLength(2)
    expect(rowTexts(bodyRows(popover)[0])).toEqual([
      'east-1',
      'gpt-upstream',
      '4',
      '40%',
      '1.2K',
      '60%',
      '50%',
      '0%',
      '42.6 token/s',
    ])
  })

  it('uses the raw endpoint id when a name is missing and the row label when the upstream model is missing', () => {
    renderPopover(
      row({
        upstream_count: 2,
        upstream_breakdown: [
          upstream({
            endpoint_id: 'abcdef0123456789',
            endpoint_name: null,
            upstream_model: null,
          }),
          upstream({ endpoint_name: 'west-1' }),
        ],
      }),
      'model',
    )
    const popover = openPopover('model')
    const first = rowTexts(bodyRows(popover)[0])
    expect(first[0]).toBe('abcdef01')
    expect(first[1]).toBe('gpt-4o')
  })

  it('dashes unknown metrics, keeps an exact zero and folds only a zero total', () => {
    renderPopover(
      row({
        upstream_count: 3,
        upstream_breakdown: [
          upstream({
            cache_rate: null,
            token_share: null,
            avg_output_tokens_per_second: null,
            request_count: 2,
          }),
          upstream({
            endpoint_id: 'endpoint-3',
            endpoint_name: 'west-2',
            request_count: 3,
            token_share: 0,
            cache_rate: 0,
            error_rate: 0,
            total_tokens: 0,
          }),
          upstream({ endpoint_id: 'endpoint-4', endpoint_name: 'north-1' }),
        ],
      }),
      'model',
    )
    const popover = openPopover('model')
    const rows = bodyRows(popover)
    expect(rows).toHaveLength(2)
    expect(rowTexts(rows[0])).toEqual([
      'east-1',
      'gpt-upstream',
      '2',
      '40%',
      '1.2K',
      '-',
      '-',
      '0%',
      '-',
    ])
    expect(screen.getByText('3 zero-token requests hidden')).toBeTruthy()
  })

  it('shows the folded rows after their visible rows and reports their request sum', () => {
    renderPopover(
      row({
        upstream_count: 4,
        upstream_breakdown: [
          upstream({
            endpoint_id: 'a',
            endpoint_name: 'a-1',
            request_count: 1,
          }),
          upstream({
            endpoint_id: 'b',
            endpoint_name: 'b-1',
            request_count: 5,
            total_tokens: 0,
          }),
          upstream({
            endpoint_id: 'c',
            endpoint_name: 'c-1',
            request_count: 2,
          }),
          upstream({
            endpoint_id: 'd',
            endpoint_name: 'd-1',
            request_count: 7,
            total_tokens: 0,
          }),
        ],
      }),
      'model',
    )
    const popover = openPopover('model')

    expect(bodyRows(popover).map((cells) => rowTexts(cells)[0])).toEqual([
      'a-1',
      'c-1',
    ])
    expect(screen.getByText('12 zero-token requests hidden')).toBeTruthy()

    fireEvent.click(
      screen.getByRole('button', { name: copy.overviewZeroTokenReveal }),
    )
    expect(bodyRows(popover).map((cells) => rowTexts(cells)[0])).toEqual([
      'a-1',
      'c-1',
      'b-1',
      'd-1',
    ])
    expect(
      screen.getByRole('button', { name: copy.overviewZeroTokenHide }),
    ).toBeTruthy()
  })

  it('resets the reveal when the row identity changes', () => {
    const first = row({
      upstream_count: 2,
      upstream_breakdown: [
        upstream({ endpoint_id: 'a', endpoint_name: 'a-1' }),
        upstream({
          endpoint_id: 'b',
          endpoint_name: 'b-1',
          total_tokens: 0,
        }),
      ],
    })
    const view = renderPopover(first, 'model')
    const popover = openPopover('model')
    fireEvent.click(
      screen.getByRole('button', { name: copy.overviewZeroTokenReveal }),
    )
    expect(bodyRows(popover).map((cells) => rowTexts(cells)[0])).toEqual([
      'a-1',
      'b-1',
    ])

    view.rerender(
      <LocaleProvider>
        <BreakdownDetailPopover
          perspective="model"
          row={row({
            label: 'gpt-4.1',
            upstream_count: 2,
            upstream_breakdown: [
              upstream({ endpoint_id: 'a', endpoint_name: 'a-1' }),
              upstream({
                endpoint_id: 'b',
                endpoint_name: 'b-1',
                total_tokens: 0,
              }),
            ],
          })}
        />
      </LocaleProvider>,
    )
    expect(bodyRows(popover).map((cells) => rowTexts(cells)[0])).toEqual([
      'a-1',
    ])
    expect(
      screen.getByRole('button', { name: copy.overviewZeroTokenReveal }),
    ).toBeTruthy()
  })

  it('is not offered for a single upstream', () => {
    render(
      <LocaleProvider>
        <BreakdownDetailPopover
          perspective="model"
          row={row({ upstream_count: 1, upstream_breakdown: [upstream()] })}
        />
      </LocaleProvider>,
    )
    expect(screen.queryByRole('button')).toBeNull()
  })

  it('leaves a deep-frozen response untouched', () => {
    const value = deepFreeze(
      row({
        upstream_count: 2,
        upstream_breakdown: [
          upstream({ total_tokens: 0 }),
          upstream({ endpoint_id: 'b', endpoint_name: 'b-1' }),
        ],
      }),
    )
    const before = JSON.stringify(value)
    renderPopover(value, 'model')
    const popover = openPopover('model')
    fireEvent.click(
      screen.getByRole('button', { name: copy.overviewZeroTokenReveal }),
    )
    expect(JSON.stringify(value)).toBe(before)
    expect(bodyRows(popover)).toHaveLength(2)
  })
})

describe('breakdown detail popover: upstream perspective', () => {
  it('renders only the model identity with every metric', () => {
    renderPopover(
      row({
        endpoint_id: 'endpoint-1',
        model: 'gpt-4o',
        model_breakdown: [
          model(),
          model({ model: 'gpt-4o-mini', total_tokens: 0 }),
        ],
      }),
      'upstream',
    )
    const popover = openPopover('upstream')

    expect(headerTexts(popover)).toEqual([
      copy.model,
      copy.requests,
      copy.overviewRequestShare,
      copy.overviewTotalTokens,
      copy.overviewTokenShare,
      copy.overviewCacheRate,
      copy.overviewErrorRate,
      copy.overviewAvgOutputRate,
    ])
    expect(bodyRows(popover)).toHaveLength(1)
    expect(rowTexts(bodyRows(popover)[0])).toEqual([
      'gpt-requested',
      '10',
      '100%',
      '5K',
      '80%',
      '25%',
      '10%',
      '12.3 token/s',
    ])
    expect(screen.getByText('10 zero-token requests hidden')).toBeTruthy()
  })

  it('is not offered for a single model', () => {
    renderPopover(row({ model_breakdown: [model()] }), 'upstream')
    expect(screen.queryByRole('button')).toBeNull()
  })

  it('dashes a missing token total without folding it', () => {
    renderPopover(
      row({
        model_breakdown: [
          model({ model: 'a', total_tokens: null as unknown as number }),
          model({ model: 'b' }),
        ],
      }),
      'upstream',
    )
    const popover = openPopover('upstream')
    const rows = bodyRows(popover)
    expect(rows).toHaveLength(2)
    expect(rowTexts(rows[0])[3]).toBe('-')
  })
})

describe('breakdown detail popover: desktop width and footer copy', () => {
  function intrinsicWidth(popover: HTMLElement): number {
    return Number(
      popover.getAttribute('data-usage-popover-intrinsic-width') ?? '0',
    )
  }

  it('sizes the popover to its intrinsic table width on a wide viewport', () => {
    const original = window.innerWidth
    window.innerWidth = 1440
    try {
      renderPopover(
        row({
          upstream_count: 2,
          upstream_breakdown: [
            upstream({ endpoint_name: 'fixture-upstream-with-a-long-name' }),
            upstream({
              endpoint_id: 'endpoint-2',
              endpoint_name: 'west-2',
              total_tokens: 0,
            }),
          ],
        }),
        'model',
      )
      const popover = openPopover('model')
      const intrinsic = intrinsicWidth(popover)
      expect(intrinsic).toBeGreaterThan(0)
      expect(popover.style.width).toBe(`${intrinsic + 2}px`)
      expect(popover.className).not.toContain('w-72')
      expect(popover.style.width).not.toBe('288px')
    } finally {
      window.innerWidth = original
    }
  })

  it('caps the popover at the available viewport width', () => {
    const original = window.innerWidth
    window.innerWidth = 390
    try {
      renderPopover(
        row({
          upstream_count: 2,
          upstream_breakdown: [
            upstream({ endpoint_name: 'fixture-upstream-with-a-long-name' }),
            upstream({
              endpoint_id: 'endpoint-2',
              endpoint_name: 'west-2',
              total_tokens: 0,
            }),
          ],
        }),
        'model',
      )
      const popover = openPopover('model')
      const intrinsic = intrinsicWidth(popover)
      expect(intrinsic).toBeGreaterThan(390 - 32)
      expect(popover.style.width).toBe(`${390 - 32}px`)
    } finally {
      window.innerWidth = original
    }
  })

  it('describes the folded count only while the rows are hidden', () => {
    renderPopover(
      row({
        upstream_count: 2,
        upstream_breakdown: [
          upstream(),
          upstream({
            endpoint_id: 'endpoint-2',
            endpoint_name: 'west-2',
            request_count: 5,
            total_tokens: 0,
          }),
        ],
      }),
      'model',
    )
    const popover = openPopover('model')
    const footer = () => popover.querySelector('[data-usage-zero-token-footer]')
    expect(footer()?.getAttribute('data-usage-zero-token-footer')).toBe(
      'collapsed',
    )
    expect(screen.getByText('5 zero-token requests hidden')).toBeTruthy()

    fireEvent.click(
      screen.getByRole('button', { name: copy.overviewZeroTokenReveal }),
    )

    expect(footer()?.getAttribute('data-usage-zero-token-footer')).toBe(
      'expanded',
    )
    expect(screen.getByText('5 zero-token requests')).toBeTruthy()
    expect(screen.queryByText('5 zero-token requests hidden')).toBeNull()
    expect(popover.textContent).not.toContain('hidden')
    expect(
      screen.getByRole('button', { name: copy.overviewZeroTokenHide }),
    ).toBeTruthy()
  })
})
