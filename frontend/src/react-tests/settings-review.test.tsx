import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { LlmReviewSettings } from '@/generated/admin-api'

const api = vi.hoisted(() => ({
  getLlmReviewSetting: vi.fn(),
  setLlmReviewSetting: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

import { messages } from '@/i18n'
import { getPendingActions, runAsyncAction } from '@/lib/async-action'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import {
  disposeSettingsReview,
  reviewIdentityKey,
  setSettingsReviewNotifier,
  useSettingsReviewStore,
} from '@/lib/settings-review'
import {
  LLM_REVIEW_KEY,
  createLlmReviewForm,
  formFromSettings,
  llmReviewDirtyFields,
  llmReviewFormToRequest,
  llmReviewRefreshBlocked,
  mergeLlmReviewResponse,
  parseLlmReviewHeaders,
} from '@/lib/settings-review-model'
import { SettingsPage } from '@/pages/SettingsPage'
import { LlmReviewSettingsCard } from '@/components/settings/LlmReviewSettingsCard'

const copy = messages['en-US']

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

const stored: LlmReviewSettings = {
  custom_policy_text: 'Fixture policy',
  enabled: true,
  failure_policy: 'fail_closed',
  pending_ttl_seconds: 120,
  review_api_key: 'synthetic-review-key',
  review_base_url: 'https://review.example.internal',
  review_model: 'review-model',
  review_timeout_ms: 8000,
  webhook: {
    bearer_token: 'synthetic-webhook-token',
    enabled: true,
    extra_headers: { 'X-Fixture-Header': 'enabled' },
    url: 'https://review.example.internal/webhook',
  },
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle
    reject = fail
  })
  return { promise, reject, resolve }
}

function queryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
}

function store() {
  return useSettingsReviewStore.getState()
}

async function flushMicrotasks(times = 8) {
  for (let index = 0; index < times; index += 1) {
    await act(async () => {
      await Promise.resolve()
    })
  }
}

async function loadReview(client = queryClient()) {
  store().initializeIdentity(admin)
  await act(async () => {
    await store().refresh(client)
  })
  return client
}

function renderPage(path = '/settings/review') {
  return render(
    <QueryClientProvider client={queryClient()}>
      <LocaleProvider>
        <NotifierProvider>
          <MemoryRouter initialEntries={[path]}>
            <SettingsPage />
          </MemoryRouter>
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
}

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  api.getLlmReviewSetting.mockReset().mockResolvedValue(stored)
  api.setLlmReviewSetting.mockReset()
  useSettingsReviewStore.getState().resetIdentity(null)
  setSettingsReviewNotifier(null)
  useSessionStore.setState({
    bootstrapped: true,
    bootstrapping: false,
    error: '',
    me: admin,
  })
  Element.prototype.scrollIntoView = vi.fn()
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: class {
        readonly disconnect = vi.fn()
        readonly observe = vi.fn()
        readonly unobserve = vi.fn()
        constructor(_callback: ResizeObserverCallback) {}
      },
    })
  }
})

afterEach(() => {
  cleanup()
  disposeSettingsReview()
  useSettingsReviewStore.getState().resetIdentity(null)
})

describe('review settings pure model', () => {
  it('starts from the server default, not a frontend fiction', () => {
    expect(createLlmReviewForm()).toEqual({
      enabled: false,
      review_base_url: '',
      review_api_key: '',
      review_model: '',
      review_timeout_ms: 3000,
      failure_policy: 'fail_closed',
      pending_ttl_seconds: 300,
      custom_policy_text: '',
      webhook_enabled: false,
      webhook_url: '',
      webhook_bearer_token: '',
      webhook_headers_text: '',
    })
  })

  it('flattens a stored object, including the webhook header text', () => {
    expect(formFromSettings(stored)).toEqual({
      enabled: true,
      review_base_url: 'https://review.example.internal',
      review_api_key: 'synthetic-review-key',
      review_model: 'review-model',
      review_timeout_ms: 8000,
      failure_policy: 'fail_closed',
      pending_ttl_seconds: 120,
      custom_policy_text: 'Fixture policy',
      webhook_enabled: true,
      webhook_url: 'https://review.example.internal/webhook',
      webhook_bearer_token: 'synthetic-webhook-token',
      webhook_headers_text: 'X-Fixture-Header: enabled',
    })
    expect(
      formFromSettings({ ...stored, webhook: undefined as never }).webhook_url,
    ).toBe('')
  })

  it('builds the full body and parses header lines at the first colon', () => {
    const form = formFromSettings(stored)
    form.webhook_headers_text = 'X-Empty: \nX-Url: https://a.example/x:y'
    const parsed = parseLlmReviewHeaders(form.webhook_headers_text)
    expect(parsed.ok).toBe(true)
    if (!parsed.ok) throw new Error('unreachable')
    expect(parsed.headers).toEqual({
      'X-Empty': '',
      'X-Url': 'https://a.example/x:y',
    })
    expect(llmReviewFormToRequest(form, parsed.headers)).toEqual({
      enabled: true,
      review_base_url: 'https://review.example.internal',
      review_api_key: 'synthetic-review-key',
      review_model: 'review-model',
      review_timeout_ms: 8000,
      failure_policy: 'fail_closed',
      pending_ttl_seconds: 120,
      custom_policy_text: 'Fixture policy',
      webhook: {
        enabled: true,
        url: 'https://review.example.internal/webhook',
        bearer_token: 'synthetic-webhook-token',
        extra_headers: {
          'X-Empty': '',
          'X-Url': 'https://a.example/x:y',
        },
      },
    })
    expect(parseLlmReviewHeaders('no colon here')).toEqual({ ok: false })
    expect(parseLlmReviewHeaders(': empty name')).toEqual({ ok: false })
  })

  it('merges a saved response leaf by leaf and keeps newer edits', () => {
    const submitted = formFromSettings(stored)
    const current = {
      ...submitted,
      review_model: 'newer-model',
      webhook_headers_text: 'X-New: 1',
    }
    const merged = mergeLlmReviewResponse(current, submitted, {
      ...stored,
      review_model: 'server-model',
      review_base_url: 'https://server.example.internal',
      webhook: {
        bearer_token: 'synthetic-webhook-token',
        enabled: true,
        extra_headers: { 'X-Server': '2' },
        url: 'https://review.example.internal/webhook',
      },
    })

    expect(merged.review_model).toBe('newer-model')
    expect(merged.webhook_headers_text).toBe('X-New: 1')
    expect(merged.review_base_url).toBe('https://server.example.internal')
    // The response changed the base URL, and the two leaves the operator edited
    // during the write are still ahead of the accepted baseline.
    expect(llmReviewDirtyFields(merged, formFromSettings(stored))).toEqual([
      'review_base_url',
      'review_model',
      'webhook_headers_text',
    ])
  })

  it('blocks a scoped refresh until loaded, clean and idle', () => {
    const base = {
      pendingActions: {},
      dirty: false,
      failed: false,
      loading: false,
      loaded: true,
    }
    expect(llmReviewRefreshBlocked(base)).toBe(false)
    expect(llmReviewRefreshBlocked({ ...base, loaded: false })).toBe(true)
    expect(llmReviewRefreshBlocked({ ...base, loading: true })).toBe(true)
    expect(llmReviewRefreshBlocked({ ...base, dirty: true })).toBe(true)
    expect(llmReviewRefreshBlocked({ ...base, failed: true })).toBe(true)
    expect(
      llmReviewRefreshBlocked({
        ...base,
        pendingActions: { [LLM_REVIEW_KEY]: 'settings' },
      }),
    ).toBe(true)
  })
})

describe('review settings store', () => {
  it('reads once and writes nothing', async () => {
    await loadReview()
    expect(api.getLlmReviewSetting).toHaveBeenCalledTimes(1)
    expect(api.setLlmReviewSetting).not.toHaveBeenCalled()
    expect(store().loaded).toBe(true)
    expect(store().form.review_api_key).toBe('synthetic-review-key')
  })

  it('refuses to save before the settings have been read', async () => {
    store().initializeIdentity(admin)
    const outcome = await store().save()
    expect(outcome).toEqual({ executed: false })
    expect(api.setLlmReviewSetting).not.toHaveBeenCalled()
  })

  it('saves every field plus the assembled webhook and confirms once', async () => {
    await loadReview()
    const notifySaved = vi.fn()
    setSettingsReviewNotifier({
      notifyApiError: vi.fn(),
      notifyHeaderFormat: vi.fn(),
      notifySaved,
    })
    api.setLlmReviewSetting.mockImplementation(async (options) => options.body)

    store().setField('review_model', 'other-model')
    store().setField('webhook_headers_text', 'X-One: 1\nX-Two: 2')
    await act(async () => {
      await store().save()
    })

    expect(api.setLlmReviewSetting).toHaveBeenCalledTimes(1)
    const body = api.setLlmReviewSetting.mock.calls[0]?.[0]?.body
    expect(body.review_model).toBe('other-model')
    expect(body.review_api_key).toBe('synthetic-review-key')
    expect(body.webhook.bearer_token).toBe('synthetic-webhook-token')
    expect(body.webhook.extra_headers).toEqual({ 'X-One': '1', 'X-Two': '2' })
    expect(notifySaved).toHaveBeenCalledTimes(1)
  })

  it('rejects malformed headers locally with no request and one error', async () => {
    await loadReview()
    const notifyHeaderFormat = vi.fn()
    setSettingsReviewNotifier({
      notifyApiError: vi.fn(),
      notifyHeaderFormat,
      notifySaved: vi.fn(),
    })

    store().setField('webhook_headers_text', 'garbage line')
    const outcome = await act(async () => store().save())

    expect(outcome).toEqual({ executed: false })
    expect(api.setLlmReviewSetting).not.toHaveBeenCalled()
    expect(notifyHeaderFormat).toHaveBeenCalledTimes(1)
    expect(Object.hasOwn(getPendingActions(), LLM_REVIEW_KEY)).toBe(false)
    expect(store().form.webhook_headers_text).toBe('garbage line')
  })

  it('sends one request and one confirmation for a double save', async () => {
    await loadReview()
    const notifySaved = vi.fn()
    setSettingsReviewNotifier({
      notifyApiError: vi.fn(),
      notifyHeaderFormat: vi.fn(),
      notifySaved,
    })
    const write = deferred<LlmReviewSettings>()
    api.setLlmReviewSetting.mockReturnValueOnce(write.promise)

    const first = store().save()
    await flushMicrotasks(2)
    const second = await store().save()
    expect(second).toEqual({ executed: false })

    write.resolve(stored)
    await act(async () => {
      await first
    })
    expect(api.setLlmReviewSetting).toHaveBeenCalledTimes(1)
    expect(notifySaved).toHaveBeenCalledTimes(1)
  })

  it('keeps the draft on a failed save, reports once and retries with Save', async () => {
    await loadReview()
    const notifyApiError = vi.fn()
    setSettingsReviewNotifier({
      notifyApiError,
      notifyHeaderFormat: vi.fn(),
      notifySaved: vi.fn(),
    })
    api.setLlmReviewSetting
      .mockRejectedValueOnce(new Error('503 service unavailable'))
      .mockImplementationOnce(async (options) => options.body)

    store().setField('review_base_url', 'https://retry.example.internal')
    await act(async () => {
      await store().save()
    })
    expect(notifyApiError).toHaveBeenCalledTimes(1)
    expect(store().saveFailed).toBe(true)
    expect(store().form.review_base_url).toBe('https://retry.example.internal')

    await act(async () => {
      await store().save()
    })
    expect(api.setLlmReviewSetting).toHaveBeenCalledTimes(2)
    expect(store().saveFailed).toBe(false)
  })

  it('keeps an edit made while the save was in flight', async () => {
    await loadReview()
    const write = deferred<LlmReviewSettings>()
    api.setLlmReviewSetting.mockReturnValueOnce(write.promise)

    store().setField('custom_policy_text', 'submitted policy')
    const pending = store().save()
    await flushMicrotasks(2)
    store().setField('custom_policy_text', 'newer policy')

    write.resolve({ ...stored, custom_policy_text: 'server policy' })
    await act(async () => {
      await pending
    })

    expect(store().form.custom_policy_text).toBe('newer policy')
    expect(llmReviewDirtyFields(store().form, store().baseline)).toEqual([
      'custom_policy_text',
    ])
  })

  it('does not block an unrelated settings key', async () => {
    await loadReview()
    api.setLlmReviewSetting.mockImplementation(async (options) => options.body)
    const release: Array<() => void> = []
    const held = runAsyncAction(
      'setting:rawRetention',
      () => new Promise<void>((resolve) => release.push(resolve)),
      'settings',
    )
    await flushMicrotasks(1)

    await act(async () => {
      await store().save()
    })
    expect(api.setLlmReviewSetting).toHaveBeenCalledTimes(1)
    release.forEach((resolve) => resolve())
    await held
  })

  it('commits only the latest read, drops a pre-edit read and ignores stale identity', async () => {
    store().initializeIdentity(admin)
    const client = queryClient()
    const resolvers: Array<(value: LlmReviewSettings) => void> = []
    api.getLlmReviewSetting.mockImplementation(
      () =>
        new Promise<LlmReviewSettings>((resolve) => {
          resolvers.push(resolve)
        }),
    )

    void store().refresh(client)
    void store().refresh(client)
    await flushMicrotasks(4)
    const current = resolvers.at(-1) as (value: LlmReviewSettings) => void
    current({ ...stored, review_model: 'latest' })
    await flushMicrotasks(8)
    expect(store().form.review_model).toBe('latest')
    for (const stale of resolvers.slice(0, -1)) {
      stale({ ...stored, review_model: 'stale' })
    }
    await flushMicrotasks(8)
    expect(store().form.review_model).toBe('latest')

    const slow = deferred<LlmReviewSettings>()
    api.getLlmReviewSetting.mockReturnValueOnce(slow.promise)
    void store().refresh(client)
    await flushMicrotasks(2)
    store().setField('review_model', 'edited')
    expect(store().loading).toBe(false)
    slow.resolve({ ...stored, review_model: 'late' })
    await flushMicrotasks(8)
    expect(store().form.review_model).toBe('edited')

    const identityRead = deferred<LlmReviewSettings>()
    api.getLlmReviewSetting.mockReturnValueOnce(identityRead.promise)
    const read = store().refresh(client)
    store().resetIdentity(operator)
    identityRead.resolve(stored)
    await act(async () => {
      await read
    })
    expect(store().identityKey).toBe(reviewIdentityKey(operator))
    expect(store().loaded).toBe(false)
    expect(store().form.review_api_key).toBe('')
  })

  it('clears the secret draft on unmount and re-reads on the next mount', async () => {
    const client = await loadReview()
    expect(store().form.review_api_key).toBe('synthetic-review-key')

    disposeSettingsReview()
    expect(store().form.review_api_key).toBe('')
    expect(store().form.webhook_bearer_token).toBe('')
    expect(store().loaded).toBe(false)

    await act(async () => {
      await store().ensureLoaded(client)
    })
    expect(api.getLlmReviewSetting).toHaveBeenCalledTimes(2)
    expect(store().form.review_api_key).toBe('synthetic-review-key')
  })

  it('refuses a refresh while dirty and never refetches on rerender', async () => {
    const client = await loadReview()
    store().setField('review_model', 'dirty-model')
    await act(async () => {
      await store().refresh(client)
    })
    expect(api.getLlmReviewSetting).toHaveBeenCalledTimes(1)

    store().resetIdentity(admin)
    const fresh = await loadReview(queryClient())
    await act(async () => {
      await store().ensureLoaded(fresh)
    })
    expect(api.getLlmReviewSetting).toHaveBeenCalledTimes(2)
  })
})

describe('review settings tab', () => {
  it('renders the controls, native collapsible groups and the storage hint', async () => {
    renderPage()
    await flushMicrotasks()

    const section = document.querySelector('[data-foundation-route-id]')
    expect(section?.getAttribute('data-foundation-route-id')).toBe(
      'R-SETTINGS-REVIEW',
    )
    expect(section?.getAttribute('data-settings-tab')).toBe('review')
    expect(
      screen.getByRole('heading', { name: copy.settingsReview }),
    ).toBeTruthy()

    for (const id of [
      'settings-llm-review-enabled',
      'settings-llm-review-base-url',
      'settings-llm-review-model',
      'settings-llm-review-api-key',
      'settings-llm-review-failure-policy',
      'settings-llm-review-timeout-ms',
      'settings-llm-review-pending-ttl-seconds',
      'settings-llm-review-custom-policy',
      'settings-llm-review-webhook-enabled',
      'settings-llm-review-webhook-url',
      'settings-llm-review-webhook-bearer-token',
      'settings-llm-review-extra-headers',
    ]) {
      expect(document.getElementById(id), id).not.toBeNull()
    }

    const details = document.querySelectorAll('details')
    expect(details.length).toBe(3)
    for (const node of details) {
      expect(node.querySelector('summary')).not.toBeNull()
    }

    expect(screen.getByText(copy.llmReviewHint)).toBeTruthy()
    expect(screen.getByText(copy.extraHeadersHint)).toBeTruthy()
    expect(
      document
        .getElementById('settings-llm-review-api-key')
        ?.getAttribute('type'),
    ).toBe('password')
    expect(
      document
        .getElementById('settings-llm-review-webhook-bearer-token')
        ?.getAttribute('type'),
    ).toBe('password')
    expect(
      document.getElementById('settings-llm-review-failure-policy')
        ?.textContent,
    ).toContain(copy.failurePolicyFailClosed)
    expect(
      (
        document.getElementById(
          'settings-llm-review-timeout-ms',
        ) as HTMLInputElement
      ).value,
    ).toBe('8000')
    expect(
      (
        document.getElementById(
          'settings-llm-review-extra-headers',
        ) as HTMLTextAreaElement
      ).value,
    ).toBe('X-Fixture-Header: enabled')
  })

  it('never renders a secret as text and never persists it', async () => {
    renderPage()
    await flushMicrotasks()

    // The stored secret is echoed by the read, so it may exist only as the
    // value of its password input: never as text, a label, a title or a
    // placeholder, and never in storage.
    const secrets = ['synthetic-review-key', 'synthetic-webhook-token']
    for (const secret of secrets) {
      expect(document.body.textContent).not.toContain(secret)
      // Every observable attribute, except the password input's own value,
      // which the browser keeps for the control itself.
      const leaking = [...document.querySelectorAll('*')]
        .filter(
          (node) =>
            !(
              node instanceof HTMLInputElement &&
              node.getAttribute('type') === 'password' &&
              node.id.startsWith('settings-llm-review-')
            ),
        )
        .flatMap((node) =>
          [...node.attributes].map((attribute) => attribute.value),
        )
        .filter((value) => value.includes(secret))
      expect(leaking).toEqual([])
      const storedValues = Object.keys(localStorage).map((key) =>
        String(localStorage.getItem(key)),
      )
      expect(storedValues.join('\n')).not.toContain(secret)
    }
    expect(
      (
        document.getElementById(
          'settings-llm-review-api-key',
        ) as HTMLInputElement
      ).value,
    ).toBe('synthetic-review-key')
    expect(
      (
        document.getElementById(
          'settings-llm-review-api-key',
        ) as HTMLInputElement
      ).getAttribute('type'),
    ).toBe('password')
  })

  it('disables the controls and Save until the read succeeds', async () => {
    const read = deferred<LlmReviewSettings>()
    api.getLlmReviewSetting.mockReturnValueOnce(read.promise)
    renderPage()
    await flushMicrotasks()

    expect(
      (
        document.getElementById('settings-llm-review-enabled') as HTMLElement
      ).hasAttribute('disabled') ||
        (
          document.getElementById('settings-llm-review-enabled') as HTMLElement
        ).getAttribute('data-disabled') === '',
    ).toBe(true)
    expect(
      (document.getElementById('settings-llm-review-save') as HTMLButtonElement)
        .disabled,
    ).toBe(true)

    read.resolve(stored)
    await waitFor(() =>
      expect(
        (
          document.getElementById(
            'settings-llm-review-save',
          ) as HTMLButtonElement
        ).disabled,
      ).toBe(false),
    )
  })

  it('offers an explicit retry when the read fails', async () => {
    api.getLlmReviewSetting
      .mockRejectedValueOnce(new Error('503'))
      .mockResolvedValueOnce(stored)
    renderPage()
    await flushMicrotasks()

    expect(screen.getByRole('alert')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: copy.retry }))
    await flushMicrotasks()
    expect(api.getLlmReviewSetting).toHaveBeenCalledTimes(2)
    expect(
      (document.getElementById('settings-llm-review-model') as HTMLInputElement)
        .value,
    ).toBe('review-model')
  })

  it('shows the localized header error instead of sending a request', async () => {
    renderPage()
    await flushMicrotasks()

    const headers = document.getElementById(
      'settings-llm-review-extra-headers',
    ) as HTMLTextAreaElement
    fireEvent.change(headers, { target: { value: 'not-a-header' } })
    expect(screen.getByText(copy.invalidReviewHeadersFormat)).toBeTruthy()

    fireEvent.click(
      document.getElementById('settings-llm-review-save') as HTMLButtonElement,
    )
    await flushMicrotasks()
    expect(api.setLlmReviewSetting).not.toHaveBeenCalled()
    await waitFor(() =>
      expect(
        screen.getAllByText(copy.invalidReviewHeadersFormat).length,
      ).toBeGreaterThan(0),
    )
  })

  it('blocks the scoped refresh while the draft is unsaved', async () => {
    renderPage()
    await flushMicrotasks()

    const refreshButton = document.getElementById(
      'settings-review-refresh',
    ) as HTMLButtonElement
    expect(refreshButton.disabled).toBe(false)

    fireEvent.change(
      document.getElementById('settings-llm-review-model') as HTMLInputElement,
      { target: { value: 'edited-model' } },
    )
    expect(refreshButton.disabled).toBe(true)

    api.setLlmReviewSetting.mockImplementation(async (options) => options.body)
    await act(async () => {
      fireEvent.click(
        document.getElementById(
          'settings-llm-review-save',
        ) as HTMLButtonElement,
      )
    })
    await waitFor(() => expect(refreshButton.disabled).toBe(false))
    const readsBefore = api.getLlmReviewSetting.mock.calls.length
    fireEvent.click(refreshButton)
    await flushMicrotasks()
    expect(api.getLlmReviewSetting.mock.calls.length).toBe(readsBefore + 1)
  })

  it('keeps an ordinary user out of the review route', async () => {
    useSessionStore.setState({ me: operator })
    renderPage()
    await flushMicrotasks()
    expect(
      document.querySelector('[data-foundation-route-id="R-SETTINGS-REVIEW"]'),
    ).toBeNull()
    expect(api.getLlmReviewSetting).not.toHaveBeenCalled()
  })

  it('renders the field card without issuing its own read', async () => {
    const onChange = vi.fn()
    render(
      <LocaleProvider>
        <LlmReviewSettingsCard
          disabled={false}
          form={createLlmReviewForm()}
          onChange={onChange}
        />
      </LocaleProvider>,
    )
    expect(api.getLlmReviewSetting).not.toHaveBeenCalled()
    fireEvent.change(
      document.getElementById('settings-llm-review-model') as HTMLInputElement,
      { target: { value: 'typed' } },
    )
    expect(onChange).toHaveBeenCalledWith('review_model', 'typed')
  })
})
