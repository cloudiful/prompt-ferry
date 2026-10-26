import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { AvailableModelsResponse, MeResponse } from '@/generated/admin-api'
import { meListModels } from '@/generated/admin-api'
import { AvailableModelsPage } from '@/pages/AvailableModelsPage'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'

vi.mock('@/generated/admin-api', () => ({
  authLogin: vi.fn(),
  authLogout: vi.fn(),
  authMe: vi.fn(),
  bridgeStatus: vi.fn(),
  meListModels: vi.fn(),
  relayAuthLogin: vi.fn(),
  relayAuthLogout: vi.fn(),
  relayAuthMe: vi.fn(),
}))

const modelsMock = vi.mocked(meListModels) as unknown as ReturnType<
  typeof vi.fn
>

const operator: MeResponse = {
  display_name: 'Fixture Operator',
  is_admin: false,
  login_name: 'operator',
  user_id: 2,
}

function modelPage(
  models: AvailableModelsResponse['models'],
  first = 0,
  rows = 10,
  total = models.length,
): AvailableModelsResponse {
  return { first, models, rows, total }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}

function renderPage() {
  useSessionStore.setState({
    me: operator,
    bootstrapped: true,
    bootstrapping: false,
  })
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { gcTime: 0, retry: false },
    },
  })
  const view = render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <NotifierProvider>
          <AvailableModelsPage />
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
  return { ...view, queryClient }
}

beforeEach(() => {
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
  })
  modelsMock.mockResolvedValue({
    data: modelPage([
      { id: 'model-alpha', name: 'Model Alpha' },
      { id: 'model-beta', name: 'Model Beta' },
    ]),
  })
})

afterEach(() => {
  localStorage.clear()
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
  })
})

describe('available models route', () => {
  it('renders model ids and names and loads subsequent pages from the self API', async () => {
    modelsMock.mockImplementation(async ({ query }) => ({
      data:
        query?.first === 10
          ? modelPage([{ id: 'model-next', name: 'Model Next' }], 10, 10, 11)
          : modelPage(
              [{ id: 'model-alpha', name: 'Model Alpha' }],
              query?.first ?? 0,
              query?.rows ?? 10,
              11,
            ),
    }))
    renderPage()

    expect(await screen.findByText('model-alpha')).toBeTruthy()
    expect(screen.getByText('Model Alpha')).toBeTruthy()
    expect(modelsMock).toHaveBeenCalledWith(
      expect.objectContaining({ query: { first: 0, rows: 10 } }),
    )

    fireEvent.click(screen.getByRole('button', { name: 'Next page' }))
    expect(await screen.findByText('model-next')).toBeTruthy()
    expect(modelsMock).toHaveBeenCalledWith(
      expect.objectContaining({ query: { first: 10, rows: 10 } }),
    )
    expect(
      localStorage.getItem('prompt-ferry:table-page-size:available-models'),
    ).toBe('10')
  })

  it('renders one localized empty tbody row', async () => {
    localStorage.setItem('prompt-ferry:locale', 'zh-CN')
    modelsMock.mockResolvedValue({ data: modelPage([], 0, 10, 0) })
    renderPage()

    const empty = await screen.findByText('没有数据')
    const tbody = empty.closest('tbody')
    expect(tbody).not.toBeNull()
    expect(within(tbody as HTMLElement).getAllByRole('row')).toHaveLength(1)
    expect(screen.getByRole('table').getAttribute('aria-busy')).toBe('false')
  })

  it('corrects an out-of-range response to the last available page', async () => {
    modelsMock
      .mockResolvedValueOnce({
        data: modelPage([{ id: 'initial', name: 'Initial' }], 0, 10, 20),
      })
      .mockResolvedValueOnce({ data: modelPage([], 10, 10, 9) })
      .mockResolvedValueOnce({
        data: modelPage([{ id: 'last-model', name: 'Last Model' }], 0, 10, 9),
      })
    renderPage()

    expect(await screen.findByText('Initial')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Next page' }))
    expect(await screen.findByText('Last Model')).toBeTruthy()
    await waitFor(() => expect(modelsMock).toHaveBeenCalledTimes(3))
    expect(
      modelsMock.mock.calls.map(([options]) => options?.query?.first),
    ).toEqual([0, 10, 0])
  })

  it('keeps a late previous-page read from replacing the selected page', async () => {
    const oldRefresh = deferred<{ data: AvailableModelsResponse }>()
    modelsMock
      .mockResolvedValueOnce({
        data: modelPage([{ id: 'first-page', name: 'First Page' }], 0, 10, 11),
      })
      .mockReturnValueOnce(oldRefresh.promise)
      .mockResolvedValueOnce({
        data: modelPage(
          [{ id: 'second-page', name: 'Second Page' }],
          10,
          10,
          11,
        ),
      })
    renderPage()

    expect(await screen.findByText('First Page')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(modelsMock).toHaveBeenCalledTimes(2))
    fireEvent.click(screen.getByRole('button', { name: 'Next page' }))
    expect(await screen.findByText('Second Page')).toBeTruthy()

    oldRefresh.resolve({
      data: modelPage([{ id: 'stale-page', name: 'Stale Page' }], 0, 10, 11),
    })
    await waitFor(() => expect(screen.getByText('Second Page')).toBeTruthy())
    expect(screen.queryByText('Stale Page')).toBeNull()
  })

  it('reports a failed read and allows an explicit refresh retry', async () => {
    modelsMock
      .mockRejectedValueOnce({
        error: { code: 'fixture_models_failed', message: 'models failed' },
      })
      .mockResolvedValueOnce({
        data: modelPage([{ id: 'retry-model', name: 'Retry Model' }]),
      })
    renderPage()

    const alert = await screen.findByRole('alert')
    expect(alert.textContent?.includes('models failed')).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }))
    expect(await screen.findByText('retry-model')).toBeTruthy()
    expect(modelsMock).toHaveBeenCalledTimes(2)
  })
})
