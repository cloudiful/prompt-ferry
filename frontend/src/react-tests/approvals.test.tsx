import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { AppShell } from '@/layouts/AppShell'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  ApprovalPageResponse,
  ApprovalRequest,
} from '@/generated/admin-api'

const api = vi.hoisted(() => ({
  authLogin: vi.fn(),
  authLogout: vi.fn(),
  authMe: vi.fn(),
  bridgeStatus: vi.fn(),
  relayAuthLogin: vi.fn(),
  relayAuthLogout: vi.fn(),
  relayAuthMe: vi.fn(),
  listApprovals: vi.fn(),
  getApproval: vi.fn(),
  approveApproval: vi.fn(),
  rejectApproval: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useApprovalsStore } from '@/lib/approvals'
import { useSessionStore } from '@/lib/session'
import { ApprovalsPage } from '@/pages/ApprovalsPage'

const admin = {
  display_name: 'Fixture Administrator',
  is_admin: true,
  login_name: 'fixture-admin',
  user_id: 1,
}

function approval(overrides: Partial<ApprovalRequest> = {}): ApprovalRequest {
  return {
    approval_id: 'approval-1',
    approval_status: 'pending',
    client_key_label: 'integration-key',
    created_at: '2026-10-20T08:00:00.000Z',
    decided_at: null,
    decided_by_login_name: null,
    decided_by_user_id: null,
    model: 'minimax-m2',
    path: '/v1/chat/completions',
    request_deadline_unix_ms: 1_800_000_000_000,
    request_id: 'req-approval-1',
    request_payload_json: { model: 'minimax-m2' },
    request_preview: 'Please summarise this internal document.',
    review_categories: ['confidentiality', 'policy'],
    review_decision: 'review',
    review_reason: 'Request contains a confidential marker.',
    updated_at: '2026-10-20T08:00:00.000Z',
    user_id: 2,
    user_login_name: 'fixture-operator',
    wait_deadline_unix_ms: Date.now() + 305_000,
    ...overrides,
  }
}

function page(
  overrides: Partial<ApprovalPageResponse> = {},
): ApprovalPageResponse {
  return { approvals: [], first: 0, rows: 10, total: 0, ...overrides }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle
    reject = fail
  })
  return { promise, resolve, reject }
}

function renderPage() {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { gcTime: 0, retry: false } },
  })
  const view = render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <NotifierProvider>
          <MemoryRouter initialEntries={['/approvals']}>
            <Routes>
              <Route element={<AppShell />}>
                <Route path="/approvals" element={<ApprovalsPage />} />
              </Route>
            </Routes>
          </MemoryRouter>
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
  return { ...view, queryClient }
}

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  vi.clearAllMocks()
  api.listApprovals.mockImplementation(async (options) =>
    page({
      approvals: [approval()],
      first: Number(options?.query?.first ?? 0),
      rows: Number(options?.query?.rows ?? 10),
      total: 11,
    }),
  )
  api.getApproval.mockResolvedValue(approval())
  api.approveApproval.mockResolvedValue(approval())
  api.rejectApproval.mockResolvedValue(approval())
  useSessionStore.setState({
    me: null,
    bootstrapped: true,
    bootstrapping: false,
    error: '',
  })
  useApprovalsStore.getState().resetIdentity(null)
})

afterEach(() => {
  cleanup()
  useSessionStore.setState({ me: null })
  useApprovalsStore.getState().resetIdentity(null)
  localStorage.clear()
})

describe('approvals page', () => {
  it('mounts the admin route and reads the pending approvals first', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()
    expect(
      screen.getByRole('button', { name: 'Refresh' }).closest('header'),
    ).toBeTruthy()

    expect(
      document.querySelector('[data-foundation-route-id="R-APPROVALS"]'),
    ).toBeTruthy()
    await waitFor(() => expect(api.listApprovals).toHaveBeenCalled())
    expect(api.listApprovals.mock.calls[0]?.[0]?.query).toMatchObject({
      status: 'pending',
      first: 0,
      rows: 10,
    })
    expect(
      screen.getAllByText('Request contains a confidential marker.').length,
    ).toBeGreaterThan(0)
    expect(screen.getAllByText('Pending').length).toBeGreaterThan(0)
  })

  it('resets the first row on a filter change and keeps the page size', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()
    await waitFor(() => expect(api.listApprovals).toHaveBeenCalled())

    useApprovalsStore.getState().setPage(10, 10)
    await act(async () => {
      await useApprovalsStore.getState().loadList(renderQueryClient())
    })
    api.listApprovals.mockClear()

    const filter = screen.getAllByRole('combobox', { name: 'Approvals' })[0]
    await act(async () => {
      fireEvent.click(filter)
    })
    const option = await screen.findByRole('option', { name: 'Resolved' })
    await act(async () => {
      fireEvent.click(option)
    })

    await waitFor(() => expect(api.listApprovals).toHaveBeenCalled())
    expect(api.listApprovals.mock.calls[0]?.[0]?.query).toMatchObject({
      status: 'resolved',
      first: 0,
      rows: 10,
    })
  })

  it('asks for the last page that still holds rows when a page comes back empty', async () => {
    useSessionStore.setState({ me: admin })
    const queryClient = renderQueryClient()
    useApprovalsStore.getState().resetIdentity({ user_id: 1, is_admin: true })
    api.listApprovals.mockImplementation(async (options) => {
      const first = Number(options?.query?.first ?? 0)
      if (first === 30)
        return page({ approvals: [], first, rows: 10, total: 25 })
      return page({ approvals: [approval()], first, rows: 10, total: 25 })
    })

    useApprovalsStore.getState().setPage(30, 10)
    await act(async () => {
      await useApprovalsStore.getState().loadList(queryClient)
    })

    expect(api.listApprovals).toHaveBeenCalledTimes(2)
    expect(api.listApprovals.mock.calls[0]?.[0]?.query?.first).toBe(30)
    expect(api.listApprovals.mock.calls[1]?.[0]?.query?.first).toBe(20)
  })

  it('opens the detail only after a successful read', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()
    await waitFor(() => expect(api.listApprovals).toHaveBeenCalled())

    await act(async () => {
      fireEvent.click(
        screen.getByRole('button', {
          name: 'Request contains a confidential marker.',
        }),
      )
    })
    await waitFor(() => expect(api.getApproval).toHaveBeenCalled())
    const dialog = await screen.findByRole('dialog')
    expect(within(dialog).getByText('integration-key')).toBeTruthy()
    expect(within(dialog).getByText('confidentiality')).toBeTruthy()
  })

  it('keeps the detail closed and reports the failure when the read fails', async () => {
    useSessionStore.setState({ me: admin })
    api.getApproval.mockRejectedValue(new Error('approval read failed'))
    renderPage()
    await waitFor(() => expect(api.listApprovals).toHaveBeenCalled())

    await act(async () => {
      fireEvent.click(
        screen.getByRole('button', {
          name: 'Request contains a confidential marker.',
        }),
      )
    })

    await waitFor(() => expect(api.getApproval).toHaveBeenCalled())
    expect(screen.queryByRole('dialog')).toBeNull()
    expect((await screen.findByRole('alert')).textContent).toContain(
      'approval read failed',
    )
  })

  it('answers a pending approval with one write and refreshes the list', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()
    await waitFor(() => expect(api.listApprovals).toHaveBeenCalled())
    api.listApprovals.mockClear()

    await act(async () => {
      fireEvent.click(
        screen.getAllByRole('button', { name: 'Approve' })[0] as HTMLElement,
      )
    })

    await waitFor(() => expect(api.approveApproval).toHaveBeenCalledTimes(1))
    expect(api.approveApproval.mock.calls[0]?.[0]?.body).toBeUndefined()
    expect(api.rejectApproval).not.toHaveBeenCalled()
    await waitFor(() => expect(api.listApprovals).toHaveBeenCalled())
    expect((await screen.findByRole('status')).textContent).toContain(
      'Request approved',
    )
  })

  it('rejects a pending approval', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()
    await waitFor(() => expect(api.listApprovals).toHaveBeenCalled())

    await act(async () => {
      fireEvent.click(
        screen.getAllByRole('button', { name: 'Reject' })[0] as HTMLElement,
      )
    })

    await waitFor(() => expect(api.rejectApproval).toHaveBeenCalledTimes(1))
    expect(api.approveApproval).not.toHaveBeenCalled()
    expect((await screen.findByRole('status')).textContent).toContain(
      'Request rejected',
    )
  })

  it('collapses a duplicate trigger into one write', async () => {
    useSessionStore.setState({ me: admin })
    const queryClient = renderQueryClient()
    useApprovalsStore.getState().resetIdentity({ user_id: 1, is_admin: true })
    const gate = deferred<unknown>()
    api.approveApproval.mockReturnValue(gate.promise)

    const first = useApprovalsStore
      .getState()
      .approve(queryClient, 'approval-1')
    const second = await useApprovalsStore
      .getState()
      .approve(queryClient, 'approval-1')
    expect(second.executed).toBe(false)

    gate.resolve(approval())
    expect((await first).executed).toBe(true)
    expect(api.approveApproval).toHaveBeenCalledTimes(1)
  })

  it('disables the answered row while its own key is held and leaves other rows answerable', async () => {
    useSessionStore.setState({ me: admin })
    api.listApprovals.mockResolvedValue(
      page({
        approvals: [
          approval({ approval_id: 'approval-1' }),
          approval({ approval_id: 'approval-2' }),
        ],
        first: 0,
        rows: 10,
        total: 2,
      }),
    )
    renderPage()
    await waitFor(() => expect(api.listApprovals).toHaveBeenCalled())

    const gate = deferred<unknown>()
    api.approveApproval.mockReturnValue(gate.promise)
    const row = document.querySelector(
      '[data-approval-row="approval-1"]',
    ) as HTMLElement
    const other = document.querySelector(
      '[data-approval-row="approval-2"]',
    ) as HTMLElement
    const approve = row.querySelector(
      '[data-approval-action="approve"]',
    ) as HTMLButtonElement
    const reject = row.querySelector(
      '[data-approval-action="reject"]',
    ) as HTMLButtonElement
    const otherApprove = other.querySelector(
      '[data-approval-action="approve"]',
    ) as HTMLButtonElement

    await act(async () => {
      fireEvent.click(approve)
    })

    expect(approve.disabled).toBe(true)
    expect(reject.disabled).toBe(true)
    expect(approve.querySelector('svg.animate-spin')).toBeTruthy()
    expect(reject.querySelector('svg.animate-spin')).toBeNull()
    expect(otherApprove.disabled).toBe(false)

    gate.resolve(approval())
    await act(async () => {
      await Promise.resolve()
    })
  })

  it('shows the remaining wait only for pending approvals', async () => {
    useSessionStore.setState({ me: admin })
    api.listApprovals.mockResolvedValue(
      page({
        approvals: [
          approval({ approval_id: 'approval-1' }),
          approval({
            approval_id: 'approval-2',
            approval_status: 'approved',
            decided_at: '2026-10-21T08:00:00.000Z',
          }),
        ],
        first: 0,
        rows: 10,
        total: 2,
      }),
    )
    renderPage()
    await waitFor(() => expect(api.listApprovals).toHaveBeenCalled())

    const pending = Array.from(
      document.querySelectorAll('[data-approval-remaining="approval-1"]'),
    ).map((node) => node.textContent?.trim())
    const resolved = Array.from(
      document.querySelectorAll('[data-approval-remaining="approval-2"]'),
    ).map((node) => node.textContent?.trim())
    expect(pending.length).toBeGreaterThan(0)
    expect(pending.every((value) => /^5m \d+s$/.test(value ?? ''))).toBe(true)
    expect(resolved.length).toBeGreaterThan(0)
    expect(resolved.every((value) => value === '-')).toBe(true)
  })

  it('shows only the allowed detail fields', async () => {
    useSessionStore.setState({ me: admin })
    api.getApproval.mockResolvedValue(
      approval({
        approval_id: 'approval-secret-id',
        decided_by_login_name: 'secret-decider',
        path: '/v1/secret-path',
        request_id: 'req-secret-id',
        review_decision: 'review-decision-secret',
      }),
    )
    renderPage()
    await waitFor(() => expect(api.listApprovals).toHaveBeenCalled())

    await act(async () => {
      fireEvent.click(
        screen.getByRole('button', {
          name: 'Request contains a confidential marker.',
        }),
      )
    })
    const dialog = await screen.findByRole('dialog')
    const text = dialog.textContent ?? ''
    expect(text).not.toContain('req-secret-id')
    expect(text).not.toContain('/v1/secret-path')
    expect(text).not.toContain('secret-decider')
    expect(text).not.toContain('approval-secret-id')
    expect(text).not.toContain('review-decision-secret')
  })
})

function renderQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: { queries: { gcTime: 0, retry: false } },
  })
}
