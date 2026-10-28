import { render, screen, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { EndpointsTable } from '@/components/endpoints/EndpointsTable'
import { LocaleProvider } from '@/lib/i18n'
import type { EndpointListItemView } from '@/models/endpoints'

beforeEach(() => {
  localStorage.setItem('prompt-ferry:locale', 'en-US')
})

function item(
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
    scope_label: 'Admin',
    test_message: 'Not tested',
    test_severity: null,
    testing: false,
    toggling: false,
    ...overrides,
  }
}

function renderTable(items: EndpointListItemView[]) {
  return render(
    <LocaleProvider>
      <EndpointsTable
        first={0}
        items={items}
        loading={false}
        onDelete={vi.fn()}
        onEdit={vi.fn()}
        onPage={vi.fn()}
        onTest={vi.fn()}
        onTokenPlanUsage={vi.fn()}
        onToggle={vi.fn()}
        pendingActions={{}}
        rows={25}
        total={items.length}
      />
    </LocaleProvider>,
  )
}

function row(endpointId: string): HTMLElement {
  return document.querySelector(
    `[data-endpoint-row="${endpointId}"]`,
  ) as HTMLElement
}

function cellTexts(endpointId: string): string[] {
  return Array.from(row(endpointId).querySelectorAll('td')).map((cell) =>
    (cell.textContent ?? '').trim(),
  )
}

describe('upstream endpoint list table', () => {
  it('drops the placeholder user column and names the action header', () => {
    renderTable([item(), item({ endpoint_id: 'endpoint-2' })])
    const headers = screen
      .getAllByRole('columnheader')
      .map((header) => (header.textContent ?? '').trim())
    expect(headers).toEqual(['Endpoint', 'Status', 'Usage', 'Test', 'Actions'])
    expect(headers).not.toContain('User')
    expect(
      screen.getAllByRole('columnheader').map((header) => header.textContent),
    ).not.toContain('—')
  })

  it('keeps the whole endpoint URL in the DOM and wraps it at word boundaries', () => {
    const baseUrl = 'https://chatgpt.com/backend-api/codex'
    renderTable([item({ base_url: baseUrl })])
    const url = document.querySelector(
      '[data-endpoint-row="endpoint-1"] [data-endpoint-url]',
    ) as HTMLElement
    expect(url.textContent).toBe(baseUrl)
    expect(url.getAttribute('title')).toBe(baseUrl)
    expect(url.className).toContain('break-words')
    expect(url.className).not.toContain('break-all')
  })

  it('joins only the distinct scope, protocol and source labels', () => {
    renderTable([
      item({ endpoint_id: 'endpoint-1' }),
      item({
        endpoint_id: 'endpoint-2',
        native_api_source_label: 'Legacy compatible',
      }),
    ])
    const meta = (endpointId: string) =>
      (row(endpointId).querySelector('[data-endpoint-meta]') as HTMLElement)
        .textContent
    expect(meta('endpoint-1')).toBe('Admin · Automatic by caller protocol')
    expect(meta('endpoint-2')).toBe(
      'Admin · Automatic by caller protocol · Legacy compatible',
    )
  })

  it('labels an embeddings endpoint with its own protocol', () => {
    renderTable([
      item({
        native_api: 'embeddings',
        native_api_label: 'Responses',
        native_api_source_label: 'Manual',
      }),
    ])
    const meta = row('endpoint-1').querySelector(
      '[data-endpoint-meta]',
    ) as HTMLElement
    expect(meta.textContent).toBe('Admin · Embeddings · Manual')
  })

  it('shows the owner inline only for an endpoint that has one', () => {
    renderTable([
      item({ endpoint_id: 'endpoint-1' }),
      item({ endpoint_id: 'endpoint-2', owner_label: 'Owner 4' }),
    ])
    expect(row('endpoint-1').querySelector('[data-endpoint-owner]')).toBeNull()
    expect(
      (row('endpoint-2').querySelector('[data-endpoint-owner]') as HTMLElement)
        .textContent,
    ).toBe('Owner 4')
    expect(cellTexts('endpoint-1')[0]).not.toContain('—')
    expect(cellTexts('endpoint-2')[0]).not.toContain('—')
  })

  it('keeps an inapplicable quota cell empty and the test column intact', () => {
    renderTable([
      item({ endpoint_id: 'endpoint-1' }),
      item({
        endpoint_id: 'endpoint-2',
        has_oauth_token: false,
        provider: 'openai',
      }),
    ])
    const cells = Array.from(
      row('endpoint-1').querySelectorAll('td'),
    ) as HTMLTableCellElement[]
    expect(cells).toHaveLength(5)
    expect(cells[2]?.textContent?.trim()).not.toBe('')
    const inapplicable = Array.from(
      row('endpoint-2').querySelectorAll('td'),
    ) as HTMLTableCellElement[]
    expect(inapplicable).toHaveLength(5)
    expect(inapplicable[2]?.textContent?.trim()).toBe('')
    expect(
      within(inapplicable[3] as HTMLTableCellElement).getByRole('button', {
        name: 'Test',
      }),
    ).toBeTruthy()
  })
})
