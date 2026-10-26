import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  RedactionConfigSchema,
  RedactionCustomStringRulePageResponseSchema,
} from '@/generated/admin-api'

const api = vi.hoisted(() => ({
  authLogin: vi.fn(),
  authLogout: vi.fn(),
  authMe: vi.fn(),
  bridgeStatus: vi.fn(),
  relayAuthLogin: vi.fn(),
  relayAuthLogout: vi.fn(),
  relayAuthMe: vi.fn(),
  getRedactionSetting: vi.fn(),
  listRedactionCustomStrings: vi.fn(),
  listUserOptions: vi.fn(),
  previewRedaction: vi.fn(),
  setRedactionSetting: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

import { MemoryRouter } from 'react-router'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useRedactionStore, redactionTargetToken } from '@/lib/redaction'
import { useSessionStore } from '@/lib/session'
import { RedactionPage } from '@/pages/RedactionPage'

const admin = {
  display_name: 'Fixture Administrator',
  is_admin: true,
  login_name: 'fixture-admin',
  user_id: 1,
}
const operator = {
  display_name: 'Fixture Operator',
  is_admin: false,
  login_name: 'fixture-operator',
  user_id: 2,
}
const targetUsers = [
  {
    ...admin,
    created_at: '2026-10-16T08:00:00.000Z',
    is_active: true,
    updated_at: '2026-10-23T12:00:00.000Z',
  },
  {
    ...operator,
    created_at: '2026-10-16T08:00:00.000Z',
    is_active: true,
    updated_at: '2026-10-23T12:00:00.000Z',
  },
]

function config(
  customStrings: RedactionConfigSchema['custom_strings'] = [
    {
      created_at: '2026-10-16T08:00:00.000Z',
      match_type: 'contains',
      pattern: 'fixture-rule-alpha',
      scope: 'text',
      updated_at: '2026-10-23T12:00:00.000Z',
    },
    {
      created_at: '2026-10-16T08:00:00.000Z',
      match_type: 'regex',
      pattern: 'fixture-rule-[0-9]+',
      scope: 'line',
      updated_at: '2026-10-23T12:00:00.000Z',
    },
  ],
): RedactionConfigSchema {
  return {
    custom_strings: customStrings,
    enabled: true,
    rules: {
      cidr: true,
      domain: true,
      email: true,
      ip: true,
      organization: false,
      person: false,
      phone: true,
      secret: true,
      url: true,
    },
  }
}

let serverConfig: RedactionConfigSchema

function previewFixture() {
  return {
    applied_replacements: [
      {
        display_value: 'mail@fixture.test',
        kind: 'email',
        replacement: '[EMAIL]',
        strategy: 'mask',
      },
    ],
    findings: [
      {
        confidence: 0.98,
        end: 16,
        kind: 'email',
        match_text: 'mail@fixture.test',
        normalized_key: 'email',
        source: 'rule',
        start: 0,
      },
    ],
    redacted_text: '[EMAIL] body',
    stats: {
      applied_replacements: 1,
      dropped_findings: 0,
      llm_candidates_accepted: 0,
      llm_candidates_rejected: 0,
      llm_configured: false,
      llm_error: null,
      llm_request_failed: false,
      total_findings: 1,
    },
  }
}

function serverPage(
  options: { query?: Record<string, string | number | undefined> } | undefined,
): RedactionCustomStringRulePageResponseSchema {
  const query = options?.query ?? {}
  const search = String(query.search ?? '')
    .trim()
    .toLowerCase()
  const all = serverConfig.custom_strings
    .map((rule, array_index) => ({ ...rule, array_index }))
    .filter((rule) => !search || rule.pattern.toLowerCase().includes(search))
    .sort((left, right) => right.array_index - left.array_index)
  const first = Number(query.first ?? 0)
  const rows = Number(query.rows ?? 10)
  return {
    first,
    items: all.slice(first, first + rows),
    rows,
    total: all.length,
  }
}

function renderPage(path = '/redaction/rules') {
  return render(
    <LocaleProvider>
      <NotifierProvider>
        <MemoryRouter initialEntries={[path]}>
          <RedactionPage />
        </MemoryRouter>
      </NotifierProvider>
    </LocaleProvider>,
  )
}

beforeEach(() => {
  Element.prototype.scrollIntoView = vi.fn()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  vi.clearAllMocks()
  serverConfig = config()
  api.getRedactionSetting.mockImplementation(async (options) => ({
    config: serverConfig,
    scope: options?.query?.scope ?? 'global',
    user_id: options?.query?.user_id ?? null,
  }))
  api.listRedactionCustomStrings.mockImplementation(async (options) =>
    serverPage(options),
  )
  api.listUserOptions.mockResolvedValue({ users: targetUsers })
  api.previewRedaction.mockImplementation(async () => ({
    preview: previewFixture(),
  }))
  api.setRedactionSetting.mockImplementation(async (options) => {
    serverConfig = options.body
    return {
      config: serverConfig,
      scope: options.query.scope,
      user_id: options.query.user_id ?? null,
    }
  })
  useSessionStore.setState({
    me: null,
    bootstrapped: true,
    bootstrapping: false,
    error: '',
  })
  useRedactionStore.getState().resetIdentity(null)
})

afterEach(() => {
  useSessionStore.setState({ me: null })
  useRedactionStore.getState().resetIdentity(null)
  localStorage.clear()
})

describe('redaction rules page', () => {
  it('loads the authenticated admin rules route and keeps scope, builtin, and custom dirtiness distinct', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()

    expect(
      await screen.findByRole('heading', { name: 'Built-in rules' }),
    ).toBeTruthy()
    await waitFor(() =>
      expect(api.listRedactionCustomStrings).toHaveBeenCalled(),
    )
    expect(api.listUserOptions).toHaveBeenCalledTimes(1)
    expect(api.getRedactionSetting).toHaveBeenCalledWith(
      expect.objectContaining({ query: { scope: 'global' } }),
    )

    fireEvent.click(screen.getByRole('checkbox', { name: 'Secret' }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'Redaction' }))
    expect(screen.queryByText('Unsaved')).toBeNull()

    fireEvent.change(screen.getByLabelText('Pattern 2'), {
      target: { value: 'changed-fixture-rule' },
    })
    expect(screen.getByText('Unsaved')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() =>
      expect(api.setRedactionSetting).toHaveBeenCalledTimes(1),
    )
    expect(api.setRedactionSetting.mock.calls[0]?.[0].body).toMatchObject({
      enabled: false,
      rules: { secret: false },
      custom_strings: [
        expect.objectContaining({ pattern: 'fixture-rule-alpha' }),
        expect.objectContaining({ pattern: 'changed-fixture-rule' }),
      ],
    })
    expect(await screen.findByRole('status')).toBeTruthy()
  })

  it('confirms custom deletion locally and never issues a delete request', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()
    await screen.findByLabelText('Pattern 2')

    fireEvent.click(screen.getByRole('button', { name: 'Delete 1' }))
    const dialog = screen.getByRole('dialog')
    expect(
      within(dialog).getByText(
        'Delete this custom rule? The change only takes effect after save.',
      ),
    ).toBeTruthy()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    expect(useRedactionStore.getState().config.custom_strings).toHaveLength(2)

    fireEvent.click(screen.getByRole('button', { name: 'Delete 1' }))
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }))
    expect(useRedactionStore.getState().config.custom_strings).toHaveLength(1)
    expect(screen.getByText('Unsaved')).toBeTruthy()
    expect(api.setRedactionSetting).not.toHaveBeenCalled()
  })

  it('blocks invalid regex before PATCH and exposes the local validation error', async () => {
    useSessionStore.setState({ me: admin })
    renderPage()
    await screen.findByLabelText('Pattern 2')

    fireEvent.click(screen.getByRole('button', { name: 'Add rule' }))
    fireEvent.change(screen.getByLabelText('Pattern 3'), {
      target: { value: '[' },
    })
    fireEvent.click(screen.getByRole('combobox', { name: 'Match type 3' }))
    fireEvent.click(screen.getByRole('option', { name: 'Regex' }))
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))

    expect(
      document.getElementById('redaction-save-validation-error'),
    ).toBeTruthy()
    expect(
      screen.getByLabelText('Pattern 3').getAttribute('aria-invalid'),
    ).toBe('true')
    expect(api.setRedactionSetting).not.toHaveBeenCalled()
  })

  it('uses the signed-in user scope and does not request admin user options for an ordinary user', async () => {
    useSessionStore.setState({ me: operator })
    renderPage()

    expect(await screen.findByText('My private rules')).toBeTruthy()
    await waitFor(() =>
      expect(api.listRedactionCustomStrings).toHaveBeenCalled(),
    )
    expect(api.listUserOptions).not.toHaveBeenCalled()
    expect(api.getRedactionSetting).toHaveBeenCalledWith(
      expect.objectContaining({
        query: { scope: 'user', user_id: operator.user_id },
      }),
    )

    act(() => useSessionStore.setState({ me: null }))
    expect(useRedactionStore.getState()).toMatchObject({
      identityKey: null,
      config: expect.objectContaining({ custom_strings: [] }),
      customStringReveal: new Set(),
      previewText: '',
      previewResult: null,
    })
    expect(screen.queryByLabelText('Pattern 2')).toBeNull()
  })
})

describe('redaction preview page', () => {
  beforeEach(() => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: vi.fn().mockResolvedValue(undefined) },
    })
  })

  it('renders the preview route with the input kind, preview action, and empty details', async () => {
    useSessionStore.setState({ me: admin })
    renderPage('/redaction/preview')

    expect(
      await screen.findByRole('heading', { name: 'Preview', level: 1 }),
    ).toBeTruthy()
    expect(
      document.querySelector(
        '[data-foundation-route-id="R-REDACTION-PREVIEW"]',
      ),
    ).toBeTruthy()
    expect(screen.getByRole('combobox', { name: 'Input kind' })).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Preview' })).toBeTruthy()
    expect(screen.getByText('No findings')).toBeTruthy()
    expect(screen.getByText('No replacements')).toBeTruthy()
  })

  it('runs a preview and shows stats, findings, replacements, and the highlighted output', async () => {
    useSessionStore.setState({ me: admin })
    renderPage('/redaction/preview')
    await screen.findByRole('heading', { name: 'Preview', level: 1 })

    fireEvent.change(screen.getByRole('textbox', { name: 'Input' }), {
      target: { value: 'mail@fixture.test body' },
    })
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    })

    expect(api.previewRedaction).toHaveBeenCalledTimes(1)
    expect(api.previewRedaction.mock.calls[0]?.[0].body).toMatchObject({
      input_kind: 'text',
      text: 'mail@fixture.test body',
    })
    expect(screen.getByText('Findings 1')).toBeTruthy()
    expect(screen.getByText('Replacements 1')).toBeTruthy()
    expect(
      screen.getByRole('button', { name: 'mail@fixture.test' }),
    ).toBeTruthy()
    expect(
      document.getElementById('redaction-preview-output')?.textContent,
    ).toBe('[EMAIL] body')

    fireEvent.click(screen.getByRole('button', { name: 'mail@fixture.test' }))
    expect(
      document.querySelectorAll(
        '#redaction-preview-output [data-preview-highlight="true"]',
      ).length,
    ).toBe(1)
    expect(
      document.querySelectorAll(
        '#redaction-preview-input [data-preview-highlight="true"]',
      ).length,
    ).toBe(1)
  })

  it('copies the redacted output and reports the copied state', async () => {
    useSessionStore.setState({ me: admin })
    renderPage('/redaction/preview')
    await screen.findByRole('heading', { name: 'Preview', level: 1 })

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Preview' }))
    })
    await act(async () => {
      fireEvent.click(
        screen.getByRole('button', { name: 'Copy redacted output' }),
      )
    })

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('[EMAIL] body')
    expect(screen.getByText('Copied')).toBeTruthy()
  })

  it('keeps the shared draft and avoids a configuration refetch on a loaded target', async () => {
    useSessionStore.setState({ me: admin })
    useRedactionStore.getState().resetIdentity(admin)
    const state = useRedactionStore.getState()
    useRedactionStore.setState({
      config: serverConfig,
      loadedTargetToken: redactionTargetToken(state),
    })
    useRedactionStore.getState().setPreviewText('shared draft body')

    renderPage('/redaction/preview')
    await screen.findByRole('heading', { name: 'Preview', level: 1 })

    expect(api.getRedactionSetting).not.toHaveBeenCalled()
    expect(api.listUserOptions).not.toHaveBeenCalled()
    expect(useRedactionStore.getState().previewText).toBe('shared draft body')
  })
})
