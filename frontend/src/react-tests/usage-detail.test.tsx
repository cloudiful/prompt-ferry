import { fireEvent, render, screen, within } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  RequestRecordDetail,
  RequestRecordFullResponse,
  SessionRouteOptionsResponse,
} from '@/generated/admin-api'
import { messages } from '@/i18n'
import { LocaleProvider } from '@/lib/i18n'
import { UsageDetailDialog } from '@/components/usage/UsageDetailDialog'
import { McpUsageDetailDialog } from '@/components/usage/McpUsageDetailDialog'
import {
  createUsageDetailView,
  createUsageOverrideView,
  createUsageRouteOptionsView,
  usageReasoningEffortDisplay,
} from '@/lib/usage-detail-model'

const copy = messages['en-US']

class TestResizeObserver implements ResizeObserver {
  readonly disconnect = vi.fn()
  readonly observe = vi.fn()
  readonly unobserve = vi.fn()

  constructor(_callback: ResizeObserverCallback) {}
}

class TestIntersectionObserver implements IntersectionObserver {
  static instances: TestIntersectionObserver[] = []
  readonly root = null
  readonly rootMargin = ''
  readonly scrollMargin = ''
  readonly thresholds = []
  readonly disconnect = vi.fn()
  readonly observe = vi.fn()
  readonly takeRecords = vi.fn(() => [])
  readonly unobserve = vi.fn()

  constructor(readonly callback: IntersectionObserverCallback) {
    TestIntersectionObserver.instances.push(this)
  }

  trigger(): void {
    this.callback([{ isIntersecting: true } as IntersectionObserverEntry], this)
  }
}

const writeText = vi.fn(async () => {})

beforeEach(() => {
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  Element.prototype.scrollIntoView = vi.fn()
  TestIntersectionObserver.instances = []
  vi.clearAllMocks()
  Object.defineProperty(globalThis, 'IntersectionObserver', {
    configurable: true,
    value: TestIntersectionObserver,
  })
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: TestResizeObserver,
    })
  }
  Object.defineProperty(navigator, 'clipboard', {
    configurable: true,
    value: { writeText },
  })
})

function detail(
  overrides: Partial<RequestRecordDetail> = {},
): RequestRecordDetail {
  return {
    abort_from_state: null,
    abort_reason: null,
    abort_response_started: null,
    applied_thinking_effort_override: null,
    assistant_message_json: null,
    assistant_output_items_json: null,
    client_key_label: 'team-key',
    conversation_id: 'abcdef1234567890',
    conversation_seq: 2,
    conversation_source: 'codex',
    created_at: '2026-03-04T23:30:00.000Z',
    endpoint_id: 'endpoint-1',
    endpoint_key_id: 'key-1',
    endpoint_key_label: 'primary',
    endpoint_name: 'east-1',
    error_code: null,
    error_message: null,
    has_full_request: true,
    has_parent: false,
    http_request_compressed: false,
    model: 'gpt-4o',
    path: '/v1/responses',
    record_id: 1,
    redaction: {
      applied: false,
      fields: [],
      findings_count: 0,
      replacements_count: 0,
      types: [],
    },
    request_category: 'ai',
    request_has_previous_response_id: false,
    request_id: 'req-1',
    request_raw_json: { reasoning_effort: 'low' },
    request_state: 'completed',
    request_storage_mode: 'normalized',
    requested_model: 'gpt-4o',
    response_capture_truncated: false,
    response_prompt: null,
    route_selection_reason: 'default',
    session_header_id: 'session-1',
    storage_sanitized: false,
    storage_sanitized_nul_count: 0,
    upstream_model: 'gpt-4o-mini',
    ...overrides,
  }
}

function full(
  overrides: Partial<RequestRecordFullResponse> = {},
): RequestRecordFullResponse {
  return {
    conversation_source: 'codex',
    has_more: false,
    limit: 10,
    messages: [
      {
        block_hash: 'block-1',
        content_json: { content: 'hello', role: 'user' },
        preview_text: 'hello',
        role: 'user',
        same_as_turn: null,
      },
      {
        block_hash: 'block-2',
        content_json: { content: 'hi', role: 'assistant' },
        preview_text: 'hi',
        role: 'assistant',
        same_as_turn: 2,
      },
    ],
    next_cursor: null,
    offset: 0,
    order: 'desc',
    record_id: 1,
    rendered_text: 'hello\nhi',
    request_has_previous_response_id: false,
    request_storage_mode: 'normalized',
    total_messages: 2,
    ...overrides,
  }
}

function options(
  overrides: Partial<SessionRouteOptionsResponse> = {},
): SessionRouteOptionsResponse {
  return {
    affinity: {
      endpoint_id: 'endpoint-1',
      endpoint_name: 'east-1',
      key_id: 'key-1',
      key_label: 'primary',
      rule_id: 'rule-1',
      state: 'active',
    },
    conversation_id: 'abcdef1234567890',
    current_endpoint_id: 'endpoint-1',
    current_endpoint_key_id: 'key-1',
    current_endpoint_key_label: 'primary',
    options: [
      {
        endpoint_id: 'endpoint-1',
        endpoint_name: 'east-1',
        is_override: false,
        is_preferred: true,
        keys: [{ key_id: 'key-1', key_label: 'primary' }],
      },
      {
        endpoint_id: 'endpoint-2',
        endpoint_name: 'west-1',
        is_override: false,
        is_preferred: false,
        keys: [{ key_id: 'key-2', key_label: 'secondary' }],
      },
    ],
    override_endpoint_id: null,
    override_endpoint_key_id: null,
    override_endpoint_key_label: null,
    ...overrides,
  }
}

type DialogProps = {
  detail?: RequestRecordDetail | null
  detailLoading?: boolean
  isAdmin?: boolean
  open?: boolean
  override?: ReturnType<typeof createUsageOverrideView>
  pending?: boolean
  pendingOrigin?: string | null
  requestFull?: RequestRecordFullResponse | null
  requestFullLoading?: boolean
  requestFullLoadingMore?: boolean
  routeOptions?: ReturnType<typeof createUsageRouteOptionsView> | null
  routeOptionsLoading?: boolean
  onClearOverride?: () => void
  onLoadFirst?: () => void
  onLoadMore?: () => void
  onOpenChange?: (open: boolean) => void
  onResetAffinity?: () => void
  onSaveOverride?: (endpointId: string, endpointKeyId: string | null) => void
}

function renderDialog(props: DialogProps = {}) {
  const routeOptions =
    props.routeOptions === undefined
      ? createUsageRouteOptionsView(
          options({ conversation_id: 'abcdef1234567890' }),
        )
      : props.routeOptions
  const view =
    props.detail === null
      ? null
      : createUsageDetailView(props.detail ?? detail())
  return render(
    <LocaleProvider>
      <UsageDetailDialog
        detail={view}
        detailLoading={props.detailLoading ?? false}
        isAdmin={props.isAdmin ?? true}
        onClearOverride={props.onClearOverride ?? vi.fn()}
        onLoadFirst={props.onLoadFirst ?? vi.fn()}
        onLoadMore={props.onLoadMore ?? vi.fn()}
        onOpenChange={props.onOpenChange ?? vi.fn()}
        onResetAffinity={props.onResetAffinity ?? vi.fn()}
        onSaveOverride={props.onSaveOverride ?? vi.fn()}
        open={props.open ?? true}
        override={props.override ?? null}
        pending={props.pending ?? false}
        pendingOrigin={props.pendingOrigin ?? null}
        requestFull={props.requestFull ?? null}
        requestFullLoading={props.requestFullLoading ?? false}
        requestFullLoadingMore={props.requestFullLoadingMore ?? false}
        routeOptions={routeOptions}
        routeOptionsLoading={props.routeOptionsLoading ?? false}
      />
    </LocaleProvider>,
  )
}

function contextValue(label: string): string {
  const section = document.querySelector('[data-usage-context-section]')
  const field = Array.from(section?.children[1]?.children ?? []).find(
    (entry) => entry.firstElementChild?.textContent === label,
  )
  return field?.lastElementChild?.textContent ?? ''
}

describe('AI detail: context', () => {
  it('renders the identity fields and the derived labels', () => {
    renderDialog()
    expect(contextValue(copy.model)).toBe('gpt-4o (gpt-4o-mini)')
    expect(contextValue(copy.upstream)).toBe('east-1')
    expect(contextValue(copy.upstreamId)).toBe('endpoint-1')
    expect(contextValue(copy.upstreamKey)).toContain('primary')
    expect(contextValue(copy.upstreamKey)).toContain('key-1')
    expect(contextValue(copy.clientKey)).toBe('team-key')
    expect(contextValue(copy.conversationId)).toBe('abcdef1234567890')
    expect(contextValue(copy.conversationSeq)).toBe('#2')
    expect(contextValue(copy.conversationSource)).toBe('codex')
    expect(contextValue(copy.sessionHeaderId)).toBe('session-1')
  })

  it('shows a first-turn badge and parent id only when they apply', () => {
    const view = renderDialog({
      detail: detail({ conversation_seq: 1, session_parent_id: 'parent-1' }),
    })
    expect(contextValue(copy.conversationSeq)).toBe(`#1${copy.firstTurn}`)
    expect(contextValue(copy.sessionParentId)).toBe('parent-1')

    view.rerender(
      <LocaleProvider>
        <UsageDetailDialog
          detail={createUsageDetailView(detail())}
          detailLoading={false}
          isAdmin
          onClearOverride={vi.fn()}
          onLoadFirst={vi.fn()}
          onLoadMore={vi.fn()}
          onOpenChange={vi.fn()}
          onResetAffinity={vi.fn()}
          onSaveOverride={vi.fn()}
          open
          override={null}
          pending={false}
          pendingOrigin={null}
          requestFull={null}
          requestFullLoading={false}
          requestFullLoadingMore={false}
          routeOptions={null}
          routeOptionsLoading={false}
        />
      </LocaleProvider>,
    )
    expect(contextValue(copy.sessionParentId)).toBe('')
  })

  it('renders the reasoning effort with the override arrow and hides it when unknown', () => {
    renderDialog({
      detail: detail({ applied_thinking_effort_override: 'high' }),
    })
    expect(contextValue(copy.reasoningEffort)).toBe('low → high')
  })

  it('ignores the override on the Anthropic messages path', () => {
    renderDialog({
      detail: detail({
        applied_thinking_effort_override: 'high',
        path: '/v1/messages',
      }),
    })
    expect(contextValue(copy.reasoningEffort)).toBe('low')
  })

  it('extracts reasoning from the request-full payload when the detail has none', () => {
    const view = createUsageDetailView(
      detail({
        request_raw_json: null,
        applied_thinking_effort_override: null,
      }),
    )
    expect(
      usageReasoningEffortDisplay(view, {
        ...full(),
        request_raw_json: { reasoning: { effort: 'medium' } },
      }),
    ).toBe('medium')
    expect(usageReasoningEffortDisplay(view, full())).toBeNull()
  })
})

describe('AI detail: error and abort', () => {
  it('shows the error summary, copies it and reports a missing upstream body', () => {
    renderDialog({
      detail: detail({ error_code: 'E_BOOM', error_message: 'boom' }),
    })
    expect(
      document.querySelector('[data-usage-error-summary]')?.textContent,
    ).toBe('E_BOOM: boom')
    expect(screen.getByText(copy.noUpstreamErrorBody)).toBeTruthy()
    fireEvent.click(
      within(errorSection()).getByRole('button', { name: copy.copy }),
    )
    expect(writeText).toHaveBeenCalledWith('E_BOOM: boom')
  })

  it('renders the abort details and a bounded upstream body', () => {
    renderDialog({
      detail: detail({
        error_message: 'Aborted by downstream client',
        request_state: 'aborted',
        abort_reason: 'downstream_closed',
        abort_from_state: 'upstream_processing',
        abort_response_started: true,
        upstream_error_body: 'line\n'.repeat(200),
      }),
    })
    expect(screen.getByText(copy.abortDetails)).toBeTruthy()
    expect(screen.getByText(copy.abortReasonDownstreamClosed)).toBeTruthy()
    expect(screen.getByText(copy.requestStateUpstreamProcessing)).toBeTruthy()
    expect(screen.getByText(copy.responseStartedYes)).toBeTruthy()
    expect(screen.getByText(copy.truncatedPreview)).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: copy.showFullContent }))
    expect(screen.queryByText(copy.truncatedPreview)).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: copy.collapseContent }))
    expect(screen.getByText(copy.truncatedPreview)).toBeTruthy()
  })

  it('hides the block when nothing failed', () => {
    renderDialog()
    expect(document.querySelector('[data-usage-error-section]')).toBeNull()
  })

  it('keeps a copy failure local and retryable', () => {
    writeText.mockRejectedValueOnce(new Error('denied'))
    renderDialog({
      detail: detail({ error_code: 'E_BOOM', error_message: 'boom' }),
    })
    fireEvent.click(
      within(errorSection()).getByRole('button', { name: copy.copy }),
    )
    expect(screen.getByText('E_BOOM: boom')).toBeTruthy()
  })
})

describe('AI detail: transcript', () => {
  it('renders the message items with their labels and metas', () => {
    renderDialog({ requestFull: full() })
    expect(screen.getByText('user · block-1')).toBeTruthy()
    expect(screen.getByText('assistant · block-2')).toBeTruthy()
    expect(screen.getByText('hello')).toBeTruthy()
    expect(screen.getByText(`${copy.sameAsTurn} 2`)).toBeTruthy()
    expect(screen.getByText(`(${2}/${2})`)).toBeTruthy()
  })

  it('expands one item into safe JSON and prunes a stale expansion', () => {
    const view = renderDialog({ requestFull: full() })
    const first = document.querySelector(
      '[data-usage-transcript-item="request-block-1"]',
    ) as HTMLElement
    fireEvent.click(within(first).getByRole('button'))
    expect(
      within(first).getByRole('button').getAttribute('aria-expanded'),
    ).toBe('true')
    expect(first.querySelector('[data-usage-json]')?.textContent).toContain(
      'hello',
    )

    view.rerender(
      <LocaleProvider>
        <UsageDetailDialog
          detail={createUsageDetailView(detail())}
          detailLoading={false}
          isAdmin
          onClearOverride={vi.fn()}
          onLoadFirst={vi.fn()}
          onLoadMore={vi.fn()}
          onOpenChange={vi.fn()}
          onResetAffinity={vi.fn()}
          onSaveOverride={vi.fn()}
          open
          override={null}
          pending={false}
          pendingOrigin={null}
          requestFull={full({
            messages: [
              {
                block_hash: 'other',
                content_json: {},
                preview_text: 'other',
                role: 'user',
                same_as_turn: null,
              },
            ],
          })}
          requestFullLoading={false}
          requestFullLoadingMore={false}
          routeOptions={null}
          routeOptionsLoading={false}
        />
      </LocaleProvider>,
    )
    expect(
      document.querySelector('[data-usage-transcript-item="request-block-1"]'),
    ).toBeNull()
  })

  it('requests only the first page while visible and missing', () => {
    const onLoadFirst = vi.fn()
    const view = renderDialog({ onLoadFirst })
    expect(onLoadFirst).toHaveBeenCalledTimes(1)

    view.rerender(
      <LocaleProvider>
        <UsageDetailDialog
          detail={createUsageDetailView(detail())}
          detailLoading={false}
          isAdmin
          onClearOverride={vi.fn()}
          onLoadFirst={onLoadFirst}
          onLoadMore={vi.fn()}
          onOpenChange={vi.fn()}
          onResetAffinity={vi.fn()}
          onSaveOverride={vi.fn()}
          open
          override={null}
          pending={false}
          pendingOrigin={null}
          requestFull={null}
          requestFullLoading
          requestFullLoadingMore={false}
          routeOptions={null}
          routeOptionsLoading={false}
        />
      </LocaleProvider>,
    )
    expect(onLoadFirst).toHaveBeenCalledTimes(1)
  })

  it('does not load a transcript the record does not have', () => {
    const onLoadFirst = vi.fn()
    renderDialog({ detail: detail({ has_full_request: false }), onLoadFirst })
    expect(onLoadFirst).not.toHaveBeenCalled()
  })

  it('requests the next page from the show-more button while more remains', () => {
    const onLoadMore = vi.fn()
    renderDialog({
      onLoadMore,
      requestFull: full({ has_more: true, next_cursor: '2' }),
    })
    fireEvent.click(screen.getByRole('button', { name: copy.showMoreContent }))
    expect(onLoadMore).toHaveBeenCalledTimes(1)
  })

  it('disconnects the sentinel observer on unmount', () => {
    const onLoadMore = vi.fn()
    const view = renderDialog({
      onLoadMore,
      requestFull: full({ has_more: true, next_cursor: '2' }),
    })
    const observer = TestIntersectionObserver.instances.at(-1)
    expect(observer?.observe).toHaveBeenCalled()
    view.unmount()
    expect(observer?.disconnect).toHaveBeenCalled()
  })

  it('requests the next page once from the intersection sentinel', () => {
    const onLoadMore = vi.fn()
    renderDialog({
      onLoadMore,
      requestFull: full({ has_more: true, next_cursor: '2' }),
    })
    const observer = TestIntersectionObserver.instances.at(-1)
    observer?.trigger()
    expect(onLoadMore).toHaveBeenCalledTimes(1)
  })

  it('renders a malicious payload as text, never as markup', () => {
    renderDialog({
      requestFull: full({
        messages: [
          {
            block_hash: 'evil',
            content_json: {
              content: '<img src=x onerror=alert(1)>',
              role: 'user',
            },
            preview_text: '<img src=x onerror=alert(1)>',
            role: 'user',
            same_as_turn: null,
          },
        ],
      }),
    })
    const item = document.querySelector(
      '[data-usage-transcript-item="request-evil"]',
    ) as HTMLElement
    fireEvent.click(within(item).getByRole('button'))
    expect(item.querySelector('img')).toBeNull()
    expect(item.textContent).toContain('<img src=x onerror=alert(1)>')
  })

  it('shows the loading and empty transcript states', () => {
    const view = renderDialog({
      detail: detail({ request_raw_json: null }),
      requestFullLoading: true,
    })
    expect(screen.getByText(copy.requestFullLoading)).toBeTruthy()

    view.rerender(
      <LocaleProvider>
        <UsageDetailDialog
          detail={createUsageDetailView(
            detail({
              request_raw_json: null,
              request_state: 'upstream_processing',
            }),
          )}
          detailLoading={false}
          isAdmin
          onClearOverride={vi.fn()}
          onLoadFirst={vi.fn()}
          onLoadMore={vi.fn()}
          onOpenChange={vi.fn()}
          onResetAffinity={vi.fn()}
          onSaveOverride={vi.fn()}
          open
          override={null}
          pending={false}
          pendingOrigin={null}
          requestFull={null}
          requestFullLoading={false}
          requestFullLoadingMore={false}
          routeOptions={null}
          routeOptionsLoading={false}
        />
      </LocaleProvider>,
    )
    expect(screen.getByText(copy.processingResponsePending)).toBeTruthy()
  })
})

describe('AI detail: session routing', () => {
  it('keeps the block read-only for an ordinary user', () => {
    renderDialog({ isAdmin: false })
    expect(screen.getByText(copy.sessionRouting)).toBeTruthy()
    expect(
      screen.queryByRole('combobox', { name: copy.selectUpstream }),
    ).toBeNull()
    expect(screen.queryByRole('button', { name: copy.setOverride })).toBeNull()
    expect(
      screen.queryByRole('button', { name: copy.resetAffinity }),
    ).toBeNull()
  })

  it('maps the automatic key to a null key id and passes the selection', async () => {
    const onSaveOverride = vi.fn()
    renderDialog({ onSaveOverride })
    fireEvent.click(screen.getByRole('combobox', { name: copy.selectUpstream }))
    fireEvent.click(await screen.findByRole('option', { name: 'west-1' }))
    fireEvent.click(screen.getByRole('button', { name: copy.setOverride }))
    expect(onSaveOverride).toHaveBeenCalledWith('endpoint-2', null)

    fireEvent.click(
      screen.getByRole('combobox', { name: copy.selectUpstreamKey }),
    )
    fireEvent.click(await screen.findByRole('option', { name: 'secondary' }))
    fireEvent.click(screen.getByRole('button', { name: copy.setOverride }))
    expect(onSaveOverride).toHaveBeenLastCalledWith('endpoint-2', 'key-2')
  })

  it('shows the binding state and a spinner only for the pending action', () => {
    const view = renderDialog({
      pending: true,
      pendingOrigin: 'save',
      routeOptions: createUsageRouteOptionsView(
        options({
          affinity: {
            endpoint_id: 'retired',
            endpoint_name: 'retired-1',
            key_id: 'key-retired',
            key_label: 'retired key',
            rule_id: 'rule-1',
            state: 'stale_endpoint',
          },
          override_endpoint_id: 'endpoint-2',
          override_endpoint_name: 'west-1',
        } as Partial<SessionRouteOptionsResponse>),
      ),
      override: {
        conversation_id: 'c',
        endpoint_id: 'endpoint-2',
        endpoint_key_id: null,
        endpoint_key_label: null,
        endpoint_name: 'west-1',
      },
    })
    expect(screen.getByText(copy.bindingStale)).toBeTruthy()
    expect(screen.getAllByText('west-1').length).toBeGreaterThan(0)
    expect(
      (
        screen.getByRole('button', {
          name: copy.resetAffinity,
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true)

    view.rerender(
      <LocaleProvider>
        <UsageDetailDialog
          detail={createUsageDetailView(detail())}
          detailLoading={false}
          isAdmin
          onClearOverride={vi.fn()}
          onLoadFirst={vi.fn()}
          onLoadMore={vi.fn()}
          onOpenChange={vi.fn()}
          onResetAffinity={vi.fn()}
          onSaveOverride={vi.fn()}
          open
          override={null}
          pending
          pendingOrigin="reset"
          requestFull={null}
          requestFullLoading={false}
          requestFullLoadingMore={false}
          routeOptions={createUsageRouteOptionsView(options())}
          routeOptionsLoading={false}
        />
      </LocaleProvider>,
    )
    expect(
      (
        screen.getByRole('button', {
          name: copy.setOverride,
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
  })

  it('asks for confirmation before resetting the affinity', () => {
    const onResetAffinity = vi.fn()
    const confirm = vi
      .spyOn(window, 'confirm')
      .mockReturnValueOnce(false)
      .mockReturnValueOnce(true)
    renderDialog({ onResetAffinity })

    fireEvent.click(screen.getByRole('button', { name: copy.resetAffinity }))
    expect(onResetAffinity).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: copy.resetAffinity }))
    expect(onResetAffinity).toHaveBeenCalledTimes(1)
    confirm.mockRestore()
  })

  it('blocks every dismissal path while a write is pending', () => {
    const onOpenChange = vi.fn()
    renderDialog({ onOpenChange, pending: true })
    expect(document.querySelector('[data-slot="dialog-close"]')).toBeNull()

    fireEvent.keyDown(document.activeElement ?? document.body, {
      key: 'Escape',
    })
    fireEvent.pointerDown(document.body)
    expect(onOpenChange).not.toHaveBeenCalledWith(false)
  })

  it('shows the routing unavailable state without options', () => {
    renderDialog({
      routeOptions: null,
      routeOptionsLoading: false,
    })
    expect(screen.getByText(copy.sessionRoutingUnavailable)).toBeTruthy()
  })

  it('hides the session block when the record has no conversation', () => {
    renderDialog({
      detail: detail({ conversation_id: null }),
      routeOptions: null,
    })
    expect(screen.queryByText(copy.sessionRouting)).toBeNull()
  })
})

describe('MCP detail', () => {
  function renderMcp(
    overrides: Partial<RequestRecordDetail> = {},
    open = true,
  ) {
    return render(
      <LocaleProvider>
        <McpUsageDetailDialog
          detail={createUsageDetailView(
            detail({
              conversation_id: null,
              request_category: 'mcp',
              request_raw_json: { method: 'tools/call' },
              response_prompt: '{"result":"ok"}',
              ...overrides,
            }),
          )}
          detailLoading={false}
          onOpenChange={vi.fn()}
          open={open}
        />
      </LocaleProvider>,
    )
  }

  it('renders both payloads and no AI-only sections', () => {
    renderMcp()
    expect(screen.getByText(copy.mcpRequestPayload)).toBeTruthy()
    expect(screen.getByText(copy.mcpResponsePayload)).toBeTruthy()
    expect(screen.queryByText(copy.requestContext)).toBeNull()
    expect(screen.queryByText(copy.sessionRouting)).toBeNull()
    expect(screen.queryByText(copy.rawMessages)).toBeNull()
    expect(document.querySelectorAll('[data-usage-json]')).toHaveLength(2)
  })

  it('keeps a raw non-JSON response and shows the logging-off text when empty', () => {
    const view = renderMcp({ response_prompt: 'plain text' })
    expect(document.body.textContent).toContain('plain text')

    view.rerender(
      <LocaleProvider>
        <McpUsageDetailDialog
          detail={createUsageDetailView(
            detail({
              conversation_id: null,
              request_category: 'mcp',
              request_raw_json: null,
              response_prompt: null,
            }),
          )}
          detailLoading={false}
          onOpenChange={vi.fn()}
          open
        />
      </LocaleProvider>,
    )
    expect(screen.getAllByText(copy.contentLoggingOff)).toHaveLength(2)
  })

  it('falls back to the raw text when the JSON-looking payload fails to parse', () => {
    renderMcp({ response_prompt: '{not valid json' })
    expect(document.body.textContent).toContain('{not valid json')
  })

  it('copies the parsed response payload text', async () => {
    const writeText = vi.fn(async () => {})
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    renderMcp()
    fireEvent.click(screen.getAllByRole('button', { name: copy.copy })[1])
    expect(writeText).toHaveBeenCalledWith(
      JSON.stringify({ result: 'ok' }, null, 2),
    )
  })

  it('keeps the shared error block in the MCP dialog', () => {
    renderMcp({ error_message: 'mcp failed', error_code: 'MCP_ERR' })
    expect(screen.getByText('MCP_ERR: mcp failed')).toBeTruthy()
  })
})

function errorSection(): HTMLElement {
  return document.querySelector('[data-usage-error-section]') as HTMLElement
}
