import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from '@testing-library/react'
import { useState } from 'react'
import { act } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  RequestRecordFacets,
  RequestRecordListRow,
} from '@/generated/admin-api'
import { messages } from '@/i18n'
import { LocaleProvider } from '@/lib/i18n'
import { RequestRecordsTable } from '@/components/usage/RequestRecordsTable'
import { RequestRecordsToolbar } from '@/components/usage/RequestRecordsToolbar'
import { UsageRangePicker } from '@/components/usage/UsageRangePicker'
import { setDisplayTimeZone } from '@/lib/display-timezone'
import { createDefaultUsageFilters } from '@/lib/usage-model'
import type {
  UsageCategory,
  UsageFilters,
  UsageScope,
  UsageSortOrder,
} from '@/lib/usage-model'

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

type TableProps = {
  category?: UsageCategory
  records?: RequestRecordListRow[]
  isAdmin?: boolean
  first?: number
  rows?: number
  total?: number
  sortField?: string
  sortOrder?: UsageSortOrder
  loading?: boolean
  onSort?: (field: string) => void
  onPage?: (first: number, rows: number) => void
  onOpenDetail?: (record: RequestRecordListRow) => void
}

function renderTable(props: TableProps = {}) {
  return render(
    <LocaleProvider>
      <RequestRecordsTable
        category={props.category ?? 'ai'}
        first={props.first ?? 0}
        isAdmin={props.isAdmin ?? true}
        loading={props.loading ?? false}
        onOpenDetail={props.onOpenDetail ?? vi.fn()}
        onPage={props.onPage ?? vi.fn()}
        onSort={props.onSort ?? vi.fn()}
        records={props.records ?? [record()]}
        rows={props.rows ?? 25}
        sortField={props.sortField ?? 'created_at'}
        sortOrder={props.sortOrder ?? -1}
        total={props.total ?? 1}
      />
    </LocaleProvider>,
  )
}

function columnIds(): string[] {
  return Array.from(document.querySelectorAll('[data-usage-column]')).map(
    (cell) => cell.getAttribute('data-usage-column') ?? '',
  )
}

function header(column: string): HTMLTableCellElement {
  return document.querySelector(
    `[data-usage-column="${column}"]`,
  ) as HTMLTableCellElement
}

function firstRow(): HTMLTableRowElement {
  return document.querySelector(
    '[data-usage-record-row]',
  ) as HTMLTableRowElement
}

function cellText(column: string, row = firstRow()): string {
  return row.querySelector(`[data-usage-cell="${column}"]`)?.textContent ?? ''
}

describe('request records table: columns', () => {
  it('renders the exact AI columns with the admin user column', () => {
    renderTable()
    expect(columnIds()).toEqual([
      'details',
      'request_date',
      'created_at',
      'user',
      'client_key',
      'model',
      'target',
      'request_state',
      'redaction',
      'duration_ms',
      'total_tokens',
      'throughput',
      'error',
    ])
  })

  it('keeps every header label and the date/time values on one line', () => {
    renderTable()
    const headers = Array.from(
      document.querySelectorAll('[data-usage-column]'),
    ) as HTMLElement[]
    expect(headers.length).toBeGreaterThan(0)
    for (const cell of headers)
      expect(cell.className).toContain('whitespace-nowrap')
    expect(
      within(header('request_date')).getByRole('button').className,
    ).toContain('whitespace-nowrap')
    const dateCell = firstRow().querySelector(
      '[data-usage-cell="request_date"]',
    ) as HTMLElement
    const timeCell = firstRow().querySelector(
      '[data-usage-cell="created_at"]',
    ) as HTMLElement
    const modelCell = firstRow().querySelector(
      '[data-usage-cell="model"]',
    ) as HTMLElement
    expect(dateCell.className).toContain('whitespace-nowrap')
    expect(timeCell.className).toContain('whitespace-nowrap')
    expect(modelCell.className).toContain('whitespace-normal')
  })

  it('keeps a usable AI model column and bounded upstream sizing', () => {
    renderTable()
    const modelHeader = header('model')
    const modelCell = firstRow().querySelector(
      '[data-usage-cell="model"]',
    ) as HTMLElement
    const targetHeader = header('target')
    const targetCell = firstRow().querySelector(
      '[data-usage-cell="target"]',
    ) as HTMLElement
    expect(modelHeader.className).toContain('min-w-[14rem]')
    expect(modelCell.className).toContain('min-w-[14rem]')
    expect(targetHeader.className).toContain('min-w-[12rem]')
    expect(targetCell.className).toContain('min-w-[12rem]')
    expect(modelHeader.className).toContain('whitespace-nowrap')
    expect(cellText('model').length).toBeGreaterThan(0)
    cleanup()
    renderTable({
      category: 'mcp',
      records: [record({ request_category: 'mcp' })],
    })
    expect(header('target').className).not.toContain('min-w-[12rem]')
  })

  it('drops the user column for an ordinary user', () => {
    renderTable({ isAdmin: false })
    expect(columnIds()).not.toContain('user')
  })

  it('renders the exact MCP columns and keeps the details entry point', () => {
    renderTable({
      category: 'mcp',
      records: [record({ request_category: 'mcp' })],
    })
    expect(columnIds()).toEqual([
      'details',
      'request_date',
      'created_at',
      'user',
      'client_key',
      'target',
      'provider_kind',
      'mcp_method',
      'mcp_operation',
      'request_state',
      'duration_ms',
      'error',
    ])
  })

  it('keeps the provided row order without sorting locally', () => {
    renderTable({
      records: [
        record({ record_id: 1, total_tokens: 900 }),
        record({ record_id: 2, total_tokens: 1 }),
      ],
      sortField: 'total_tokens',
      sortOrder: 1,
    })
    expect(
      Array.from(document.querySelectorAll('[data-usage-record-row]')).map(
        (row) => row.getAttribute('data-usage-record-row'),
      ),
    ).toEqual(['1', '2'])
  })
})

describe('request records table: sortable headers', () => {
  it('puts a real button in every sortable header and none elsewhere', () => {
    const onSort = vi.fn()
    renderTable({ onSort })
    const sortable = [
      ['request_date', 'usage_date'],
      ['created_at', 'created_at'],
      ['user', 'user_key'],
      ['client_key', 'client_key_label'],
      ['model', 'model_key'],
      ['target', 'target'],
      ['request_state', 'request_state'],
      ['duration_ms', 'duration_ms'],
      ['total_tokens', 'total_tokens'],
    ] as const
    for (const [column, field] of sortable) {
      const button = within(header(column)).getByRole('button')
      expect(button.getAttribute('type')).toBe('button')
      fireEvent.click(button)
      expect(onSort).toHaveBeenLastCalledWith(field)
    }
    for (const column of ['details', 'redaction', 'throughput', 'error']) {
      expect(within(header(column)).queryByRole('button')).toBeNull()
    }
  })

  it('reports each backend sort field once per click', () => {
    const onSort = vi.fn()
    renderTable({ onSort })
    const expected: [string, string][] = [
      ['request_date', 'usage_date'],
      ['created_at', 'created_at'],
      ['user', 'user_key'],
      ['client_key', 'client_key_label'],
      ['model', 'model_key'],
      ['target', 'target'],
      ['request_state', 'request_state'],
      ['duration_ms', 'duration_ms'],
      ['total_tokens', 'total_tokens'],
    ]
    for (const [column, field] of expected) {
      fireEvent.click(within(header(column)).getByRole('button'))
      expect(onSort).toHaveBeenLastCalledWith(field)
    }
    expect(onSort).toHaveBeenCalledTimes(expected.length)
  })

  it('exposes the active aria-sort and an unsorted cycle value', () => {
    const view = renderTable({ sortField: 'model_key', sortOrder: 1 })
    expect(header('model').getAttribute('aria-sort')).toBe('ascending')
    expect(header('total_tokens').getAttribute('aria-sort')).toBeNull()

    view.rerender(
      <LocaleProvider>
        <RequestRecordsTable
          category="ai"
          first={0}
          isAdmin
          loading={false}
          onOpenDetail={vi.fn()}
          onPage={vi.fn()}
          onSort={vi.fn()}
          records={[record()]}
          rows={25}
          sortField="model_key"
          sortOrder={-1}
          total={1}
        />
      </LocaleProvider>,
    )
    expect(header('model').getAttribute('aria-sort')).toBe('descending')

    view.rerender(
      <LocaleProvider>
        <RequestRecordsTable
          category="ai"
          first={0}
          isAdmin
          loading={false}
          onOpenDetail={vi.fn()}
          onPage={vi.fn()}
          onSort={vi.fn()}
          records={[record()]}
          rows={25}
          sortField="model_key"
          sortOrder={0}
          total={1}
        />
      </LocaleProvider>,
    )
    expect(header('model').getAttribute('aria-sort')).toBe('none')
  })

  it('does not add a button to the non-sortable MCP provider kind header', () => {
    renderTable({
      category: 'mcp',
      records: [record({ request_category: 'mcp' })],
    })
    expect(within(header('provider_kind')).queryByRole('button')).toBeNull()
    expect(within(header('mcp_method')).getByRole('button')).toBeTruthy()
  })
})

describe('request records table: cells', () => {
  it('renders the display-zone date and 24-hour time', () => {
    renderTable()
    expect(cellText('request_date')).toBe('2026-03-04')
    expect(cellText('created_at')).toBe('23:30:00')
  })

  it('renders the user, client key, model and upstream identities', () => {
    renderTable({
      records: [
        record({
          client_key_label: 'team-key',
          conversation_id: 'abcdef1234567890',
          conversation_seq: 3,
          model: 'gpt-4o',
          requested_model: 'gpt-4o',
          upstream_model: 'gpt-4o-mini',
          user_login_name: 'operator',
        }),
      ],
    })
    expect(cellText('user')).toBe('operator')
    expect(cellText('client_key')).toBe('team-key')
    expect(cellText('model')).toBe('gpt-4o (gpt-4o-mini)')
    // The upstream identity falls back to the request path, then the badges.
    expect(cellText('target')).toBe('/v1/chat/completionsSession abcdef12#3')
  })

  it('dashes a missing identity and shows the first-turn badge', () => {
    renderTable({
      records: [
        record({
          client_key_label: null,
          conversation_id: 'ffeeddccbbaa',
          conversation_seq: 1,
          user_login_name: null,
        }),
      ],
    })
    expect(cellText('user')).toBe('-')
    expect(cellText('client_key')).toBe('-')
    expect(cellText('target')).toContain('First turn')
  })

  it('keeps an exact zero and dashes an unknown token metric', () => {
    const view = renderTable({
      records: [
        record({
          cache_rate: 0,
          cached_tokens: 0,
          total_tokens: 1000,
        }),
      ],
    })
    expect(cellText('total_tokens')).toBe('1K · 0%')
    const tokenSpan = firstRow().querySelector(
      '[data-usage-cell="total_tokens"] span',
    ) as HTMLElement
    expect(tokenSpan.className).toContain('whitespace-nowrap')

    view.rerender(
      <LocaleProvider>
        <RequestRecordsTable
          category="ai"
          first={0}
          isAdmin
          loading={false}
          onOpenDetail={vi.fn()}
          onPage={vi.fn()}
          onSort={vi.fn()}
          records={[
            record({
              cache_rate: null,
              cached_tokens: null,
              total_tokens: null,
            }),
          ]}
          rows={25}
          sortField="created_at"
          sortOrder={-1}
          total={1}
        />
      </LocaleProvider>,
    )
    expect(cellText('total_tokens')).toBe('- · -')
  })

  it('shows only the total and the cache rate in the token cell', () => {
    renderTable({
      records: [
        record({
          cache_rate: 0.4,
          cached_tokens: 400,
          total_tokens: 1000,
        }),
      ],
    })
    const text = cellText('total_tokens')
    expect(text).toBe('1K · 40%')
    expect(text).not.toContain('/')
  })

  it('formats latency and only rates a completed AI request', () => {
    const view = renderTable({
      records: [
        record({
          duration_ms: 65000,
          output_tokens: 120,
          request_state: 'completed',
        }),
      ],
    })
    expect(cellText('duration_ms')).toBe('1m 5s')
    expect(cellText('throughput')).toBe('1.8')

    view.rerender(
      <LocaleProvider>
        <RequestRecordsTable
          category="ai"
          first={0}
          isAdmin
          loading={false}
          onOpenDetail={vi.fn()}
          onPage={vi.fn()}
          onSort={vi.fn()}
          records={[
            record({
              duration_ms: null,
              output_tokens: 120,
              request_state: 'failed',
            }),
          ]}
          rows={25}
          sortField="created_at"
          sortOrder={-1}
          total={1}
        />
      </LocaleProvider>,
    )
    expect(cellText('duration_ms')).toBe('-')
    expect(cellText('throughput')).toBe('-')
  })

  it('renders the state chips with all three severity variants', () => {
    const view = renderTable({
      records: [record({ request_state: 'completed' })],
    })
    const chip = firstRow().querySelector('[data-usage-state-chip]')
    expect(chip?.textContent).toBe(copy.requestStateCompleted)
    expect(chip?.getAttribute('data-variant')).toBe('default')

    view.rerender(
      <LocaleProvider>
        <RequestRecordsTable
          category="ai"
          first={0}
          isAdmin
          loading={false}
          onOpenDetail={vi.fn()}
          onPage={vi.fn()}
          onSort={vi.fn()}
          records={[record({ request_state: 'failed' })]}
          rows={25}
          sortField="created_at"
          sortOrder={-1}
          total={1}
        />
      </LocaleProvider>,
    )
    expect(
      firstRow()
        .querySelector('[data-usage-state-chip]')
        ?.getAttribute('data-variant'),
    ).toBe('outline')

    view.rerender(
      <LocaleProvider>
        <RequestRecordsTable
          category="ai"
          first={0}
          isAdmin
          loading={false}
          onOpenDetail={vi.fn()}
          onPage={vi.fn()}
          onSort={vi.fn()}
          records={[record({ request_state: 'received' })]}
          rows={25}
          sortField="created_at"
          sortOrder={-1}
          total={1}
        />
      </LocaleProvider>,
    )
    expect(
      firstRow()
        .querySelector('[data-usage-state-chip]')
        ?.getAttribute('data-variant'),
    ).toBe('secondary')
  })

  it('splits the running state by the first-token timestamp', () => {
    const view = renderTable({
      records: [
        record({ request_state: 'upstream_processing', ttft_ms: null }),
      ],
    })
    expect(cellText('request_state')).toBe(copy.requestStateWaitingForResponse)

    view.rerender(
      <LocaleProvider>
        <RequestRecordsTable
          category="ai"
          first={0}
          isAdmin
          loading={false}
          onOpenDetail={vi.fn()}
          onPage={vi.fn()}
          onSort={vi.fn()}
          records={[
            record({ request_state: 'upstream_processing', ttft_ms: 120 }),
          ]}
          rows={25}
          sortField="created_at"
          sortOrder={-1}
          total={1}
        />
      </LocaleProvider>,
    )
    expect(cellText('request_state')).toBe(copy.requestStateStreamingOutput)
  })

  it('renders the redaction badge and count, otherwise a dash', () => {
    const view = renderTable({
      records: [
        record({
          redaction: {
            applied: true,
            fields: ['prompt'],
            findings_count: 2,
            replacements_count: 2,
            types: ['email'],
          },
        }),
      ],
    })
    expect(cellText('redaction')).toBe(`${copy.redactionHit}2`)

    view.rerender(
      <LocaleProvider>
        <RequestRecordsTable
          category="ai"
          first={0}
          isAdmin
          loading={false}
          onOpenDetail={vi.fn()}
          onPage={vi.fn()}
          onSort={vi.fn()}
          records={[record()]}
          rows={25}
          sortField="created_at"
          sortOrder={-1}
          total={1}
        />
      </LocaleProvider>,
    )
    expect(cellText('redaction')).toBe('-')
  })

  it('opens the detail from the AI error link and the aborted link stays neutral', () => {
    const onOpenDetail = vi.fn()
    const view = renderTable({
      onOpenDetail,
      records: [
        record({
          error_code: 'E_UPSTREAM',
          error_message: 'upstream exploded',
          request_state: 'failed',
        }),
      ],
    })
    const link = screen.getByRole('button', {
      name: 'E_UPSTREAM: upstream exploded',
    })
    fireEvent.click(link)
    expect(onOpenDetail).toHaveBeenCalledWith(
      expect.objectContaining({ record_id: 1 }),
    )

    view.rerender(
      <LocaleProvider>
        <RequestRecordsTable
          category="ai"
          first={0}
          isAdmin
          loading={false}
          onOpenDetail={onOpenDetail}
          onPage={vi.fn()}
          onSort={vi.fn()}
          records={[
            record({
              error_code: 'E_ABORT',
              error_message: 'client aborted',
              request_state: 'aborted',
            }),
          ]}
          rows={25}
          sortField="created_at"
          sortOrder={-1}
          total={1}
        />
      </LocaleProvider>,
    )
    expect(
      screen.getByRole('button', { name: 'E_ABORT: client aborted' }).className,
    ).toContain('text-muted-foreground')
  })

  it('dashes a record without an error message', () => {
    renderTable({ records: [record({ error_message: null })] })
    expect(cellText('error')).toBe('-')
    expect(screen.queryByRole('button', { name: /:/ })).toBeNull()
  })

  it('renders the MCP identities and opens the MCP detail from both entry points', () => {
    const onOpenDetail = vi.fn()
    renderTable({
      category: 'mcp',
      onOpenDetail,
      records: [
        record({
          error_code: 'MCP_ERR',
          error_message: 'server refused',
          mcp_operation_name: 'tools/call',
          mcp_protocol_method: 'tools/call',
          mcp_server_name: 'filesystem',
          request_category: 'mcp',
          request_state: 'failed',
          server_provider_kind: 'context7',
        }),
      ],
    })
    expect(cellText('target')).toBe('filesystem')
    expect(cellText('provider_kind')).toBe('context7')
    expect(cellText('mcp_method')).toBe('tools/call')
    expect(cellText('mcp_operation')).toBe('tools/call')

    fireEvent.click(
      within(firstRow()).getByRole('button', { name: copy.viewDetails }),
    )
    expect(onOpenDetail).toHaveBeenCalledTimes(1)
    fireEvent.click(
      screen.getByRole('button', { name: 'MCP_ERR: server refused' }),
    )
    expect(onOpenDetail).toHaveBeenCalledTimes(2)
  })

  it('dashes an unknown MCP provider kind and empty method', () => {
    renderTable({
      category: 'mcp',
      records: [
        record({
          mcp_operation_name: null,
          mcp_protocol_method: null,
          request_category: 'mcp',
          server_provider_kind: null,
        }),
      ],
    })
    expect(cellText('provider_kind')).toBe('-')
    expect(cellText('mcp_method')).toBe('-')
    expect(cellText('mcp_operation')).toBe('-')
  })
})

describe('request records table: paging and empty state', () => {
  it('reports the next page and a new page size', async () => {
    const onPage = vi.fn()
    renderTable({ onPage, total: 60 })
    fireEvent.click(screen.getByRole('button', { name: copy.paginationNext }))
    expect(onPage).toHaveBeenLastCalledWith(25, 25)

    fireEvent.click(
      screen.getByRole('combobox', { name: copy.paginationPageSize }),
    )
    fireEvent.click(await screen.findByRole('option', { name: '50' }))
    expect(onPage).toHaveBeenLastCalledWith(0, 50)
  })

  it('shows the category empty message without inventing rows', () => {
    const view = renderTable({ records: [], total: 0 })
    expect(screen.getByText(copy.noRequestRecords)).toBeTruthy()
    act(() => {
      view.rerender(
        <LocaleProvider>
          <RequestRecordsTable
            category="mcp"
            first={0}
            isAdmin
            loading={false}
            onOpenDetail={vi.fn()}
            onPage={vi.fn()}
            onSort={vi.fn()}
            records={[]}
            rows={25}
            sortField="created_at"
            sortOrder={-1}
            total={0}
          />
        </LocaleProvider>,
      )
    })
    expect(screen.getByText(copy.noMcpCallRecords)).toBeTruthy()
  })
})

const facets: RequestRecordFacets = {
  client_keys: [{ key_id: 7, label: 'team-key', user_login_name: 'operator' }],
  dates: [],
  models: ['gpt-4o', 'gpt-4o-mini'],
  redactions: [true, false],
  states: ['completed', 'failed'],
  users: ['operator', 'admin'],
}

type ToolbarProps = {
  category?: UsageCategory
  filters?: Partial<UsageFilters>
  isAdmin?: boolean
  scope?: UsageScope
  onFilters?: (patch: Partial<UsageFilters>) => void
  onSearch?: (search: string | null) => void
  onScope?: (scope: UsageScope) => void
}

function toolbarFilters(overrides: Partial<UsageFilters> = {}): UsageFilters {
  return { ...createDefaultUsageFilters(), ...overrides }
}

function ToolbarHarness(props: ToolbarProps) {
  const [filters, setFilters] = useState(toolbarFilters(props.filters))
  return (
    <RequestRecordsToolbar
      category={props.category ?? 'ai'}
      facets={facets}
      filters={filters}
      isAdmin={props.isAdmin ?? true}
      onFilters={(patch) => {
        setFilters((current) => ({ ...current, ...patch }))
        props.onFilters?.(patch)
      }}
      onScope={props.onScope ?? vi.fn()}
      onSearch={(search) => {
        setFilters((current) => ({ ...current, search }))
        props.onSearch?.(search)
      }}
      scope={props.scope ?? 'service'}
    />
  )
}

function renderToolbar(props: ToolbarProps = {}) {
  return render(
    <LocaleProvider>
      <ToolbarHarness {...props} />
    </LocaleProvider>,
  )
}

async function choose(triggerName: string, optionName: string) {
  fireEvent.click(screen.getByRole('combobox', { name: triggerName }))
  fireEvent.click(await screen.findByRole('option', { name: optionName }))
}

describe('request records toolbar', () => {
  it('renders the search and category-specific filter controls only', () => {
    const view = renderToolbar()
    expect(screen.getByPlaceholderText(copy.search)).toBeTruthy()
    expect(
      screen.getByRole('combobox', { name: copy.allRedactionStates }),
    ).toBeTruthy()
    expect(screen.queryByRole('combobox', { name: copy.mcpScope })).toBeNull()
    expect(screen.queryByRole('heading')).toBeNull()

    view.rerender(
      <LocaleProvider>
        <RequestRecordsToolbar
          category="mcp"
          facets={facets}
          filters={toolbarFilters()}
          isAdmin={false}
          onFilters={vi.fn()}
          onScope={vi.fn()}
          onSearch={vi.fn()}
          scope="service"
        />
      </LocaleProvider>,
    )
    expect(
      screen.queryByRole('combobox', { name: copy.allRedactionStates }),
    ).toBeNull()
    expect(screen.queryByRole('combobox', { name: copy.allUsers })).toBeNull()
    expect(screen.getByRole('combobox', { name: copy.mcpScope })).toBeTruthy()
  })

  it('commits the search only on Enter and never on a keystroke', () => {
    const onSearch = vi.fn()
    renderToolbar({ onSearch })
    const input = screen.getByPlaceholderText(copy.search)
    fireEvent.change(input, { target: { value: 'needle' } })
    expect(onSearch).not.toHaveBeenCalled()

    fireEvent.keyDown(input, { key: 'Enter' })
    expect(onSearch).toHaveBeenCalledWith('needle')

    fireEvent.keyDown(input, { key: 'Enter' })
    expect(onSearch).toHaveBeenCalledTimes(1)

    fireEvent.change(input, { target: { value: '   ' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    expect(onSearch).toHaveBeenLastCalledWith(null)
    expect(onSearch).toHaveBeenCalledTimes(2)
  })

  it('syncs the draft when the parent clears the search after a drilldown', () => {
    function Harness({ search }: { search: string | null }) {
      return (
        <RequestRecordsToolbar
          category="ai"
          facets={facets}
          filters={toolbarFilters({ search })}
          isAdmin
          onFilters={vi.fn()}
          onScope={vi.fn()}
          onSearch={vi.fn()}
          scope="service"
        />
      )
    }
    const view = render(
      <LocaleProvider>
        <Harness search="needle" />
      </LocaleProvider>,
    )
    const input = screen.getByPlaceholderText(copy.search) as HTMLInputElement
    expect(input.value).toBe('needle')

    view.rerender(
      <LocaleProvider>
        <Harness search={null} />
      </LocaleProvider>,
    )
    expect(input.value).toBe('')
  })

  it('applies each filter immediately and clears back to a null filter', async () => {
    const onFilters = vi.fn()
    renderToolbar({ onFilters })

    await choose(copy.allClientKeys, 'team-key (operator)')
    expect(onFilters).toHaveBeenLastCalledWith({ client_key_id: 7 })
    await choose(copy.allClientKeys, copy.allClientKeys)
    expect(onFilters).toHaveBeenLastCalledWith({ client_key_id: null })

    await choose(copy.allUsers, 'admin')
    expect(onFilters).toHaveBeenLastCalledWith({ user: 'admin' })

    await choose(copy.allModels, 'gpt-4o-mini')
    expect(onFilters).toHaveBeenLastCalledWith({ model: 'gpt-4o-mini' })

    await choose(copy.allStatus, copy.requestStateFailed)
    expect(onFilters).toHaveBeenLastCalledWith({ request_state: 'failed' })

    await choose(copy.allRedactionStates, 'Yes')
    expect(onFilters).toHaveBeenLastCalledWith({ redaction_applied: true })
    await choose(copy.allRedactionStates, 'No')
    expect(onFilters).toHaveBeenLastCalledWith({ redaction_applied: false })
  })

  it('reports the MCP scope', async () => {
    const onScope = vi.fn()
    renderToolbar({ category: 'mcp', onScope })
    await choose(copy.mcpScope, copy.mcpScopeGateway)
    expect(onScope).toHaveBeenCalledWith('gateway')
  })

  it('shows the current filter labels on the triggers', () => {
    renderToolbar({ filters: { model: 'gpt-4o', request_state: 'failed' } })
    expect(
      screen.getByRole('combobox', { name: copy.allModels }).textContent,
    ).toContain('gpt-4o')
    expect(
      screen.getByRole('combobox', { name: copy.allStatus }).textContent,
    ).toContain(copy.requestStateFailed)
  })
})

describe('usage range picker', () => {
  function openPicker(props: {
    onApply: (input: { range: string; start?: string; end?: string }) => void
    range?: '24h' | '7d' | 'custom'
    start?: string
    end?: string
  }) {
    render(
      <LocaleProvider>
        <UsageRangePicker
          end={props.end ?? ''}
          onApply={props.onApply as never}
          range={(props.range ?? '24h') as never}
          start={props.start ?? ''}
        />
      </LocaleProvider>,
    )
    fireEvent.click(screen.getByRole('button', { name: copy.timeRange }))
    return document.querySelector(
      '[data-slot="popover-content"]',
    ) as HTMLElement
  }

  it('applies a preset immediately without bounds', () => {
    const onApply = vi.fn()
    const popover = openPicker({ onApply })
    fireEvent.click(
      within(popover).getByRole('button', { name: copy.last7Days }),
    )
    expect(onApply).toHaveBeenCalledWith({ range: '7d' })
  })

  it('labels the stored custom window with its selected days', () => {
    render(
      <LocaleProvider>
        <UsageRangePicker
          end="2026-10-21T00:00:00.000Z"
          onApply={vi.fn()}
          range="custom"
          start="2026-10-01T00:00:00.000Z"
        />
      </LocaleProvider>,
    )
    expect(
      screen.getByRole('button', { name: copy.timeRange }).textContent,
    ).toBe('2026-10-01 – 2026-10-20')
  })

  it('applies a custom window only once both ends are real days', () => {
    const onApply = vi.fn()
    const popover = openPicker({ onApply })
    const [start, end] = Array.from(
      popover.querySelectorAll('input[type="date"]'),
    ) as HTMLInputElement[]
    fireEvent.change(start, { target: { value: '2026-10-01' } })
    expect(onApply).not.toHaveBeenCalled()

    fireEvent.change(end, { target: { value: '2026-10-20' } })
    expect(onApply).toHaveBeenCalledWith({
      end: '2026-10-21T00:00:00.000Z',
      range: 'custom',
      start: '2026-10-01T00:00:00.000Z',
    })
  })

  it('reports nothing when the custom window is unchanged', () => {
    const onApply = vi.fn()
    const popover = openPicker({
      end: '2026-10-21T00:00:00.000Z',
      onApply,
      range: 'custom',
      start: '2026-10-01T00:00:00.000Z',
    })
    const [, end] = Array.from(
      popover.querySelectorAll('input[type="date"]'),
    ) as HTMLInputElement[]
    fireEvent.change(end, { target: { value: '2026-10-20' } })
    expect(onApply).not.toHaveBeenCalled()
  })
})
