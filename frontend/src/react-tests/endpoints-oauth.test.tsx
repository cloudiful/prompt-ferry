import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react'
import { useState } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  EndpointOAuthStatusResponse,
  OAuthBrowserStartResponse,
  OAuthDeviceStartResponse,
} from '@/generated/admin-api'
import { EndpointDialog } from '@/components/endpoints/EndpointDialog'
import { LocaleProvider } from '@/lib/i18n'
import { createEmptyEndpointForm } from '@/admin-mappers/forms/endpoint'
import type { EndpointForm } from '@/models'
import type { RunOAuthAction } from '@/models/endpoints/oauth'

const sdk = vi.hoisted(() => ({
  oauthBrowserComplete: vi.fn(),
  oauthBrowserStart: vi.fn(),
  oauthClear: vi.fn(),
  oauthDevicePoll: vi.fn(),
  oauthDeviceStart: vi.fn(),
  oauthRefresh: vi.fn(),
  oauthStatus: vi.fn(),
  tokenPlanUsage: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => sdk)

function status(
  overrides: Partial<EndpointOAuthStatusResponse> = {},
): EndpointOAuthStatusResponse {
  return {
    endpoint_id: 'endpoint-1',
    expired: false,
    expires_at: null,
    has_oauth_token: false,
    plan: 'platform_api_key',
    ...overrides,
  }
}

function deviceStart(
  overrides: Partial<OAuthDeviceStartResponse> = {},
): OAuthDeviceStartResponse {
  return {
    expires_in_seconds: 600,
    flow_id: 'device-flow-synthetic',
    interval_seconds: 1,
    user_code: 'SYNTHETIC-USER-CODE',
    verification_uri: 'https://verify.example.test/device',
    ...overrides,
  }
}

function browserStart(
  overrides: Partial<OAuthBrowserStartResponse> = {},
): OAuthBrowserStartResponse {
  return {
    authorize_url:
      'https://authorize.example.test/oauth?client_id=synthetic-client',
    expires_in_seconds: 600,
    flow_id: 'browser-flow-synthetic',
    redirect_uri: 'https://callback.example.test/finish',
    ...overrides,
  }
}

function endpointForm(overrides: Partial<EndpointForm> = {}): EndpointForm {
  return {
    ...createEmptyEndpointForm(),
    endpoint_id: 'endpoint-1',
    name: 'Fixture upstream',
    provider: 'openai',
    ...overrides,
  }
}

const executeAction: RunOAuthAction = async (_endpointId, action) => ({
  executed: true,
  value: await action(),
})

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}

function renderDialog(
  initialForm = endpointForm(),
  runOAuthAction: RunOAuthAction = executeAction,
) {
  function Harness() {
    const [form, setForm] = useState(initialForm)
    const [open, setOpen] = useState(true)
    return (
      <LocaleProvider>
        <EndpointDialog
          dialogGeneration={1}
          endpointId={initialForm.endpoint_id || null}
          form={form}
          onChange={(update) => setForm((current) => update(current))}
          onOpenChange={setOpen}
          onSubmit={() => undefined}
          open={open}
          pending={false}
          runOAuthAction={runOAuthAction}
          users={[]}
        />
      </LocaleProvider>
    )
  }

  return render(<Harness />)
}

beforeEach(() => {
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    })
  }
  Element.prototype.scrollIntoView = () => undefined
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  sdk.oauthStatus.mockResolvedValue({ data: status() })
  sdk.oauthRefresh.mockResolvedValue({
    data: status({ has_oauth_token: true }),
  })
  sdk.oauthClear.mockResolvedValue(undefined)
  sdk.oauthDeviceStart.mockResolvedValue({ data: deviceStart() })
  sdk.oauthDevicePoll.mockResolvedValue({
    data: { flow_id: 'device-flow-synthetic', status: 'pending' },
  })
  sdk.oauthBrowserStart.mockResolvedValue({ data: browserStart() })
  sdk.oauthBrowserComplete.mockResolvedValue({
    data: { flow_id: 'browser-flow-synthetic', status: 'pending' },
  })
})

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  localStorage.clear()
})

describe('unmounted upstream OAuth editor', () => {
  it('loads status and uses the generated refresh and clear wrappers', async () => {
    sdk.oauthStatus
      .mockResolvedValueOnce({
        data: status({
          expired: true,
          expires_at: '2026-10-24T12:00:00.000Z',
          has_oauth_token: true,
          plan: 'chatgpt_subscription',
        }),
      })
      .mockResolvedValueOnce({ data: status({ has_oauth_token: false }) })

    renderDialog()

    expect(await screen.findByText('Credential expired')).toBeTruthy()
    expect(sdk.oauthStatus).toHaveBeenCalledWith({
      path: { endpoint_id: 'endpoint-1' },
      responseStyle: 'data',
    })
    expect(screen.getByText(/Credential expires:/)).toBeTruthy()
    expect(screen.queryByText(/access_token|refresh_token/i)).toBeNull()

    fireEvent.click(screen.getByRole('button', { name: 'Refresh credential' }))
    await waitFor(() => expect(sdk.oauthRefresh).toHaveBeenCalledTimes(1))
    expect(sdk.oauthRefresh).toHaveBeenCalledWith({
      path: { endpoint_id: 'endpoint-1' },
      responseStyle: 'data',
    })
    expect(await screen.findByText('Logged in')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Clear credential' }))
    await waitFor(() => expect(sdk.oauthClear).toHaveBeenCalledTimes(1))
    expect(sdk.oauthClear).toHaveBeenCalledWith({
      path: { endpoint_id: 'endpoint-1' },
      responseStyle: 'data',
    })
    expect(await screen.findByText('Not logged in')).toBeTruthy()
    expect(sdk.oauthStatus).toHaveBeenCalledTimes(2)
    expect(sdk.tokenPlanUsage).not.toHaveBeenCalled()
  })

  it('polls device login through the shared action and completes with a status reread', async () => {
    vi.useFakeTimers()
    const calls: Array<{ endpointId: string; label: string }> = []
    let otherWritePending = false
    const runAction: RunOAuthAction = async (endpointId, action, label) => {
      calls.push({ endpointId, label })
      if (otherWritePending) return { executed: false }
      return { executed: true, value: await action() }
    }
    sdk.oauthStatus
      .mockResolvedValueOnce({ data: status() })
      .mockResolvedValueOnce({
        data: status({ has_oauth_token: true, plan: 'chatgpt_subscription' }),
      })
    sdk.oauthDevicePoll
      .mockResolvedValueOnce({
        data: { flow_id: 'device-flow-synthetic', status: 'pending' },
      })
      .mockResolvedValueOnce({
        data: { flow_id: 'device-flow-synthetic', status: 'complete' },
      })

    renderDialog(endpointForm({ plan: 'platform_api_key' }), runAction)
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    fireEvent.click(screen.getByRole('button', { name: 'Start login' }))
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(sdk.oauthDeviceStart).toHaveBeenCalledWith({
      path: { endpoint_id: 'endpoint-1' },
      responseStyle: 'data',
    })
    expect(screen.getByText('SYNTHETIC-USER-CODE')).toBeTruthy()
    expect(calls[0]).toEqual({
      endpointId: 'endpoint-1',
      label: 'oauth-device-start',
    })

    otherWritePending = true
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000)
    })
    expect(sdk.oauthDevicePoll).not.toHaveBeenCalled()
    expect(screen.getByText('SYNTHETIC-USER-CODE')).toBeTruthy()

    otherWritePending = false
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000)
    })
    expect(sdk.oauthDevicePoll).toHaveBeenCalledWith({
      path: { endpoint_id: 'endpoint-1' },
      body: { flow_id: 'device-flow-synthetic' },
      responseStyle: 'data',
    })
    expect(screen.getByText('SYNTHETIC-USER-CODE')).toBeTruthy()

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000)
    })
    expect(sdk.oauthDevicePoll).toHaveBeenCalledTimes(2)
    expect(calls.map(({ label }) => label)).toEqual([
      'oauth-device-start',
      'oauth-device-poll',
      'oauth-device-poll',
      'oauth-device-poll',
    ])
    expect(screen.getByText('Logged in')).toBeTruthy()
    expect(screen.queryByText('SYNTHETIC-USER-CODE')).toBeNull()
  })

  it('keeps browser redirect input retryable and redacts it from errors', async () => {
    const redirectUrl =
      '  https://callback.example.test/finish?code=synthetic-auth-code&state=synthetic-state  '
    sdk.oauthBrowserComplete.mockRejectedValueOnce(
      new Error(`Rejected callback ${redirectUrl.trim()}`),
    )

    renderDialog()
    fireEvent.click(screen.getByRole('button', { name: 'Get authorize link' }))
    expect(
      await screen.findByRole('textbox', { name: 'Authorize link' }),
    ).toBeTruthy()

    const redirectInput = screen.getByLabelText(
      'Redirect URL',
    ) as HTMLInputElement
    expect(redirectInput.type).toBe('password')
    fireEvent.change(redirectInput, { target: { value: redirectUrl } })
    fireEvent.click(screen.getByRole('button', { name: 'Complete login' }))

    expect(await screen.findByText(/Rejected callback/)).toBeTruthy()
    expect(sdk.oauthBrowserComplete).toHaveBeenCalledWith({
      path: { endpoint_id: 'endpoint-1' },
      body: {
        flow_id: 'browser-flow-synthetic',
        redirect_url: redirectUrl.trim(),
      },
      responseStyle: 'data',
    })
    expect(redirectInput.value).toBe(redirectUrl)
    expect(document.body.textContent).not.toContain('synthetic-auth-code')
    expect(document.body.textContent).not.toContain('synthetic-state')
    expect(document.body.textContent).not.toContain(redirectUrl.trim())
  })

  it('clears browser flow only after complete and rereads status, not login endpoint data', async () => {
    const redirectUrl =
      'https://callback.example.test/finish?code=synthetic-code&state=synthetic-state'
    sdk.oauthStatus
      .mockResolvedValueOnce({ data: status() })
      .mockResolvedValueOnce({
        data: status({ has_oauth_token: true, plan: 'chatgpt_subscription' }),
      })
    sdk.oauthBrowserComplete.mockResolvedValueOnce({
      data: {
        endpoint: {
          endpoint_id: 'other-endpoint',
          name: 'Must not replace draft',
        },
        flow_id: 'browser-flow-synthetic',
        status: 'complete',
      },
    })

    renderDialog()
    fireEvent.click(screen.getByRole('button', { name: 'Get authorize link' }))
    expect(
      await screen.findByRole('textbox', { name: 'Authorize link' }),
    ).toBeTruthy()
    fireEvent.change(screen.getByLabelText('Redirect URL'), {
      target: { value: `  ${redirectUrl}  ` },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Complete login' }))

    expect(await screen.findByText('Logged in')).toBeTruthy()
    expect(sdk.oauthBrowserComplete).toHaveBeenCalledWith({
      path: { endpoint_id: 'endpoint-1' },
      body: { flow_id: 'browser-flow-synthetic', redirect_url: redirectUrl },
      responseStyle: 'data',
    })
    expect(sdk.oauthStatus).toHaveBeenCalledTimes(2)
    expect(
      (document.getElementById('endpoint-name') as HTMLInputElement).value,
    ).toBe('Fixture upstream')
    expect(screen.queryByLabelText('Redirect URL')).toBeNull()
  })

  it('clears browser input and ignores late completion after dialog context changes', async () => {
    const completion = deferred<unknown>()
    sdk.oauthStatus.mockImplementation(
      async (options: { path: { endpoint_id: string } }) => ({
        data: status({ endpoint_id: options.path.endpoint_id }),
      }),
    )
    sdk.oauthBrowserComplete.mockReturnValueOnce(completion.promise)

    function ContextHarness() {
      const [context, setContext] = useState({
        dialogGeneration: 1,
        endpointId: 'endpoint-1',
        form: endpointForm(),
      })
      return (
        <LocaleProvider>
          <button
            onClick={() =>
              setContext({
                dialogGeneration: 2,
                endpointId: 'endpoint-2',
                form: endpointForm({
                  endpoint_id: 'endpoint-2',
                  name: 'Second upstream',
                }),
              })
            }
            type="button"
          >
            Change dialog context
          </button>
          <EndpointDialog
            dialogGeneration={context.dialogGeneration}
            endpointId={context.endpointId}
            form={context.form}
            onChange={(update) =>
              setContext((current) => ({
                ...current,
                form: update(current.form),
              }))
            }
            onOpenChange={() => undefined}
            onSubmit={() => undefined}
            open
            pending={false}
            runOAuthAction={executeAction}
            users={[]}
          />
        </LocaleProvider>
      )
    }

    render(<ContextHarness />)
    fireEvent.click(screen.getByRole('button', { name: 'Get authorize link' }))
    expect(
      await screen.findByRole('textbox', { name: 'Authorize link' }),
    ).toBeTruthy()
    const redirectInput = screen.getByLabelText('Redirect URL')
    fireEvent.change(redirectInput, {
      target: {
        value:
          'https://callback.example.test/finish?code=transient-context-secret',
      },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Complete login' }))
    await waitFor(() =>
      expect(sdk.oauthBrowserComplete).toHaveBeenCalledTimes(1),
    )

    fireEvent.click(screen.getByText('Change dialog context'))
    await waitFor(() => expect(sdk.oauthStatus).toHaveBeenCalledTimes(2))
    expect(screen.queryByLabelText('Redirect URL')).toBeNull()

    await act(async () => {
      completion.resolve({
        data: { flow_id: 'browser-flow-synthetic', status: 'complete' },
      })
      await completion.promise
    })
    expect(sdk.oauthStatus).toHaveBeenCalledTimes(2)
    expect(sdk.oauthStatus.mock.calls.map(([options]) => options)).toEqual([
      { path: { endpoint_id: 'endpoint-1' }, responseStyle: 'data' },
      { path: { endpoint_id: 'endpoint-2' }, responseStyle: 'data' },
    ])
  })

  it('shows save-first OAuth copy without issuing status reads for a new endpoint', () => {
    renderDialog(endpointForm({ endpoint_id: '', name: '' }))

    expect(
      screen.getByText(
        'Save the endpoint first, then complete the ChatGPT login here.',
      ),
    ).toBeTruthy()
    expect(sdk.oauthStatus).not.toHaveBeenCalled()
    expect(sdk.oauthDeviceStart).not.toHaveBeenCalled()
  })
})
