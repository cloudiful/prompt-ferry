import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  ClientKey,
  ClientKeyPageResponse,
  CreateClientKeyResponse,
  MeResponse,
  User,
} from '@/generated/admin-api'
import {
  createClientKey,
  deleteClientKey,
  listClientKeys,
  listUserOptions,
  meCreateClientKey,
  meDeleteClientKey,
  meListClientKeys,
  meUpdateClientKey,
  updateClientKey,
} from '@/generated/admin-api'
import { ApiKeysPage } from '@/pages/ApiKeysPage'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'

vi.mock('@/generated/admin-api', () => ({
  authLogin: vi.fn(),
  authLogout: vi.fn(),
  authMe: vi.fn(),
  bridgeStatus: vi.fn(),
  createClientKey: vi.fn(),
  deleteClientKey: vi.fn(),
  listClientKeys: vi.fn(),
  listUserOptions: vi.fn(),
  meCreateClientKey: vi.fn(),
  meDeleteClientKey: vi.fn(),
  meListClientKeys: vi.fn(),
  meListModels: vi.fn(),
  meUpdateClientKey: vi.fn(),
  relayAuthLogin: vi.fn(),
  relayAuthLogout: vi.fn(),
  relayAuthMe: vi.fn(),
  updateClientKey: vi.fn(),
}))

const sdkMocks = {
  createClientKey: vi.mocked(createClientKey) as unknown as ReturnType<
    typeof vi.fn
  >,
  deleteClientKey: vi.mocked(deleteClientKey) as unknown as ReturnType<
    typeof vi.fn
  >,
  listClientKeys: vi.mocked(listClientKeys) as unknown as ReturnType<
    typeof vi.fn
  >,
  listUserOptions: vi.mocked(listUserOptions) as unknown as ReturnType<
    typeof vi.fn
  >,
  meCreateClientKey: vi.mocked(meCreateClientKey) as unknown as ReturnType<
    typeof vi.fn
  >,
  meDeleteClientKey: vi.mocked(meDeleteClientKey) as unknown as ReturnType<
    typeof vi.fn
  >,
  meListClientKeys: vi.mocked(meListClientKeys) as unknown as ReturnType<
    typeof vi.fn
  >,
  meUpdateClientKey: vi.mocked(meUpdateClientKey) as unknown as ReturnType<
    typeof vi.fn
  >,
  updateClientKey: vi.mocked(updateClientKey) as unknown as ReturnType<
    typeof vi.fn
  >,
}

const admin: MeResponse = {
  display_name: 'Fixture Administrator',
  is_admin: true,
  login_name: 'admin',
  user_id: 1,
}

const operator: MeResponse = {
  display_name: 'Fixture Operator',
  is_admin: false,
  login_name: 'operator',
  user_id: 2,
}

const users: User[] = [
  {
    created_at: '2026-10-16T12:00:00.000Z',
    display_name: 'Fixture Administrator',
    is_active: true,
    is_admin: true,
    login_name: 'admin',
    updated_at: '2026-10-23T12:00:00.000Z',
    user_id: 1,
  },
  {
    created_at: '2026-10-16T12:00:00.000Z',
    display_name: 'Fixture Operator',
    is_active: true,
    is_admin: false,
    login_name: 'operator',
    updated_at: '2026-10-23T12:00:00.000Z',
    user_id: 2,
  },
]

function makeKey(
  keyId: string,
  label: string,
  userId = 1,
  enabled = true,
  secret: string | null = null,
): ClientKey {
  return {
    created_at: '2026-10-16T12:00:00.000Z',
    enabled,
    key_id: keyId,
    key_prefix: 'pf_fixture_',
    label,
    last_used_at: null,
    secret,
    user_id: userId,
  }
}

function keyPage(
  keys: ClientKey[],
  first = 0,
  rows = 10,
  total = keys.length,
): ClientKeyPageResponse {
  return { first, keys, rows, total }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (cause: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, reject, resolve }
}

function renderPage(me: MeResponse) {
  useSessionStore.setState({ me, bootstrapped: true, bootstrapping: false })
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { gcTime: 0, retry: false },
      mutations: { retry: false },
    },
  })
  const view = render(
    <QueryClientProvider client={queryClient}>
      <LocaleProvider>
        <NotifierProvider>
          <ApiKeysPage />
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
  return { ...view, queryClient }
}

beforeEach(() => {
  Element.prototype.scrollIntoView = () => undefined
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
  })
  sdkMocks.listUserOptions.mockResolvedValue({ data: { users } })
  sdkMocks.meListClientKeys.mockResolvedValue({
    data: keyPage([makeKey('key-1', 'primary')]),
  })
  sdkMocks.listClientKeys.mockResolvedValue({ data: keyPage([]) })
  sdkMocks.meCreateClientKey.mockResolvedValue({
    data: {
      ...makeKey('created-key', 'new key'),
      secret: 'pf_fixture_secret_revealed_once',
    },
  })
  sdkMocks.createClientKey.mockResolvedValue({
    data: {
      ...makeKey('created-admin-key', 'new key', 2),
      secret: 'pf_fixture_secret_revealed_once',
    },
  })
  sdkMocks.meUpdateClientKey.mockImplementation(async ({ path, body }) => ({
    data: {
      ...makeKey(path.key_id, body.label ?? 'primary', 1, body.enabled ?? true),
    },
  }))
  sdkMocks.updateClientKey.mockImplementation(async ({ path, body }) => ({
    data: {
      ...makeKey(
        path.key_id,
        body.label ?? 'operator-key',
        path.user_id,
        body.enabled ?? true,
      ),
    },
  }))
  sdkMocks.meDeleteClientKey.mockResolvedValue({ data: undefined })
  sdkMocks.deleteClientKey.mockResolvedValue({ data: undefined })
})

afterEach(() => {
  localStorage.clear()
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
  })
})

describe('API key route', () => {
  it('returns focus to the generate action on escape and cancel', async () => {
    renderPage(operator)
    const trigger = screen.getByRole('button', { name: 'Generate key' })
    trigger.focus()
    fireEvent.click(trigger)
    await screen.findByRole('dialog')
    fireEvent.keyDown(document, { key: 'Escape' })
    await waitFor(() => expect(document.activeElement).toBe(trigger))
    fireEvent.click(trigger)
    const dialog = await screen.findByRole('dialog')
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(document.activeElement).toBe(trigger))
  })

  it('does not focus a detached generate button after unmount', async () => {
    const view = renderPage(operator)
    const trigger = screen.getByRole('button', { name: 'Generate key' })
    fireEvent.click(trigger)
    await screen.findByRole('dialog')
    const focus = vi.spyOn(trigger, 'focus')
    view.unmount()
    await waitFor(() => expect(trigger.isConnected).toBe(false))
    expect(focus).not.toHaveBeenCalled()
  })

  it('loads admin options and uses self or selected-owner SDK routes', async () => {
    renderPage(admin)
    await screen.findAllByDisplayValue('primary')

    expect(listUserOptions).toHaveBeenCalledTimes(1)
    expect(meListClientKeys).toHaveBeenCalledWith(
      expect.objectContaining({ query: { first: 0, rows: 10 } }),
    )
    expect(listClientKeys).not.toHaveBeenCalled()

    fireEvent.click(screen.getByRole('combobox', { name: 'Select user' }))
    fireEvent.click(await screen.findByRole('option', { name: /operator/ }))

    await waitFor(() => {
      expect(listClientKeys).toHaveBeenCalledWith(
        expect.objectContaining({
          path: { user_id: 2 },
          query: { first: 0, rows: 10 },
        }),
      )
    })
    expect(screen.getByRole('button', { name: 'Generate key' })).toBeTruthy()
  })

  it('keeps non-admins on self APIs without requesting admin options', async () => {
    renderPage(operator)
    await screen.findAllByDisplayValue('primary')

    expect(listUserOptions).not.toHaveBeenCalled()
    expect(listClientKeys).not.toHaveBeenCalled()
    expect(meListClientKeys).toHaveBeenCalledWith(
      expect.objectContaining({ query: { first: 0, rows: 10 } }),
    )
    expect(screen.queryByRole('combobox', { name: 'Select user' })).toBeNull()
  })

  it('suppresses an error from a stale owner read after switching owners', async () => {
    const oldSelfRead = deferred<{ data: ClientKeyPageResponse }>()
    let oldSelfReadSettled = false
    const oldRead = oldSelfRead.promise.finally(() => {
      oldSelfReadSettled = true
    })
    sdkMocks.meListClientKeys.mockReturnValueOnce(oldRead)
    sdkMocks.listClientKeys.mockResolvedValueOnce({
      data: keyPage([makeKey('owner-key', 'operator-owner-key', 2)]),
    })
    renderPage(admin)

    const userSelect = await screen.findByRole('combobox', {
      name: 'Select user',
    })
    await waitFor(() =>
      expect((userSelect as HTMLButtonElement).disabled).toBe(false),
    )
    fireEvent.click(userSelect)
    fireEvent.click(await screen.findByRole('option', { name: /operator/ }))
    expect(
      await screen.findAllByDisplayValue('operator-owner-key'),
    ).toBeTruthy()

    oldSelfRead.reject({
      error: {
        code: 'fixture_stale_owner_failure',
        message: 'stale owner failed',
      },
    })
    await waitFor(() => expect(oldSelfReadSettled).toBe(true))
    await waitFor(() => expect(screen.queryByRole('alert')).toBeNull())
    expect(screen.queryByDisplayValue('primary')).toBeNull()
    expect(screen.getAllByDisplayValue('operator-owner-key')).toHaveLength(2)
  })

  it('loads later key pages and persists supported page-size changes', async () => {
    sdkMocks.meListClientKeys.mockImplementation(async ({ query }) => ({
      data:
        query?.first === 10
          ? keyPage([makeKey('page-two-key', 'page-two')], 10, 10, 11)
          : keyPage(
              [makeKey('page-one-key', 'page-one')],
              0,
              query?.rows ?? 10,
              11,
            ),
    }))
    renderPage(operator)
    expect(await screen.findAllByDisplayValue('page-one')).toBeTruthy()

    fireEvent.click(screen.getByRole('button', { name: 'Next page' }))
    expect(await screen.findAllByDisplayValue('page-two')).toBeTruthy()
    expect(sdkMocks.meListClientKeys).toHaveBeenCalledWith(
      expect.objectContaining({ query: { first: 10, rows: 10 } }),
    )

    fireEvent.click(screen.getByRole('combobox', { name: 'Rows per page' }))
    fireEvent.click(await screen.findByRole('option', { name: '20' }))
    await waitFor(() =>
      expect(
        localStorage.getItem('prompt-ferry:table-page-size:api-keys'),
      ).toBe('20'),
    )
    expect(sdkMocks.meListClientKeys).toHaveBeenCalledWith(
      expect.objectContaining({ query: { first: 0, rows: 20 } }),
    )
  })

  it('returns to the last valid key page after deleting its final row', async () => {
    let refreshCount = 0
    let firstPageReads = 0
    sdkMocks.meListClientKeys.mockImplementation(async ({ query }) => {
      const first = query?.first ?? 0
      if (first === 10) {
        refreshCount += 1
        return {
          data:
            refreshCount === 1
              ? keyPage([makeKey('last-key', 'last-page')], 10, 10, 11)
              : keyPage([], 10, 10, 9),
        }
      }
      firstPageReads += 1
      return {
        data: keyPage(
          [makeKey('first-key', 'first-page')],
          0,
          query?.rows ?? 10,
          firstPageReads === 1 ? 11 : 9,
        ),
      }
    })
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    renderPage(operator)
    expect(await screen.findAllByDisplayValue('first-page')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Next page' }))
    await waitFor(() =>
      expect(sdkMocks.meListClientKeys).toHaveBeenCalledWith(
        expect.objectContaining({ query: { first: 10, rows: 10 } }),
      ),
    )
    expect(await screen.findAllByDisplayValue('last-page')).toBeTruthy()

    const row = within(
      document.querySelector('[data-key-row="last-key"]') as HTMLElement,
    )
    fireEvent.click(row.getByRole('button', { name: 'Delete' }))
    expect(await screen.findAllByDisplayValue('first-page')).toBeTruthy()
    expect(
      screen.getByRole('button', { name: 'Page 1 (current)' }),
    ).toBeTruthy()
    const pageOffsets = sdkMocks.meListClientKeys.mock.calls.map(
      ([options]) => options?.query?.first,
    )
    expect(pageOffsets.slice(0, 4)).toEqual([0, 10, 10, 0])
    expect(pageOffsets.at(-1)).toBe(0)
  })

  it('preserves a failed create draft and shows only the flat one-time secret on retry', async () => {
    const pendingCreate = deferred<{ data: CreateClientKeyResponse }>()
    sdkMocks.meCreateClientKey
      .mockRejectedValueOnce({
        error: { code: 'fixture_create_failed', message: 'create failed' },
      })
      .mockReturnValueOnce(pendingCreate.promise)
    renderPage(admin)

    await screen.findAllByDisplayValue('primary')
    fireEvent.click(screen.getByRole('button', { name: 'Generate key' }))
    const dialog = await screen.findByRole('dialog')
    const label = within(dialog).getByRole('textbox', { name: 'Key label' })
    fireEvent.change(label, { target: { value: '  rotated key  ' } })
    fireEvent.click(
      within(dialog).getByRole('button', { name: 'Generate key' }),
    )

    const alert = await screen.findByRole('alert')
    expect(alert.textContent?.includes('create failed')).toBe(true)
    expect((label as HTMLInputElement).value).toBe('  rotated key  ')

    fireEvent.click(
      within(dialog).getByRole('button', { name: 'Generate key' }),
    )
    const submit = within(dialog).getByRole('button', { name: 'Generate key' })
    await waitFor(() =>
      expect((submit as HTMLButtonElement).disabled).toBe(true),
    )
    expect(
      (
        within(dialog).getByRole('button', {
          name: 'Cancel',
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
    fireEvent.keyDown(document, { key: 'Escape' })
    expect(screen.getByRole('dialog')).toBeTruthy()

    const firstCreateCall = sdkMocks.meCreateClientKey.mock.calls[0]?.[0] as {
      body: { label?: string | null }
    }
    expect(firstCreateCall.body).toEqual({ label: 'rotated key' })

    pendingCreate.resolve({
      data: {
        ...makeKey('created-key', 'rotated key'),
        secret: 'pf_fixture_secret_revealed_once',
      },
    })
    expect(
      await within(screen.getByRole('dialog')).findByText(
        'pf_fixture_secret_revealed_once',
      ),
    ).toBeTruthy()
    expect(meCreateClientKey).toHaveBeenCalledTimes(2)
    expect(meListClientKeys).toHaveBeenCalledTimes(2)
  })

  it('shares one key guard across rename, toggle, and delete confirmation', async () => {
    let keys = [
      makeKey('key-1', 'primary'),
      makeKey('key-2', 'integration-key-with-a-long-label', 1, false),
    ]
    sdkMocks.meListClientKeys.mockImplementation(async ({ query }) => ({
      data: keyPage(keys, query.first ?? 0, query.rows ?? 10, keys.length),
    }))
    sdkMocks.meUpdateClientKey.mockImplementation(async ({ path, body }) => {
      keys = keys.map((key) =>
        key.key_id === path.key_id
          ? {
              ...key,
              enabled: body.enabled ?? key.enabled,
              label: body.label ?? key.label,
            }
          : key,
      )
      return { data: keys.find((key) => key.key_id === path.key_id)! }
    })
    sdkMocks.meDeleteClientKey.mockImplementation(async ({ path }) => {
      keys = keys.filter((key) => key.key_id !== path.key_id)
      return { data: undefined }
    })
    const confirm = vi.spyOn(window, 'confirm')
    confirm.mockReturnValueOnce(false).mockReturnValueOnce(true)
    renderPage(admin)
    await screen.findAllByDisplayValue('primary')

    const primaryRows = document.querySelectorAll('[data-key-row="key-1"]')
    const primaryRow = within(primaryRows[0] as HTMLElement)
    const switchControl = primaryRow.getByRole('switch', { name: 'Status' })
    fireEvent.click(switchControl)
    await waitFor(() => expect(meUpdateClientKey).toHaveBeenCalledTimes(1))
    expect(sdkMocks.meUpdateClientKey.mock.calls[0]?.[0].body).toEqual({
      enabled: false,
    })

    const labelInput = primaryRow.getByRole('textbox', { name: 'Name' })
    fireEvent.change(labelInput, { target: { value: '  renamed key  ' } })
    fireEvent.keyDown(labelInput, { key: 'Enter' })
    fireEvent.blur(labelInput)
    await waitFor(() => expect(meUpdateClientKey).toHaveBeenCalledTimes(2))
    expect(sdkMocks.meUpdateClientKey.mock.calls[1]?.[0].body).toEqual({
      label: 'renamed key',
    })

    fireEvent.click(primaryRow.getByRole('button', { name: 'Delete' }))
    expect(confirm).toHaveBeenCalledWith('Delete renamed key?')
    expect(meDeleteClientKey).not.toHaveBeenCalled()
    fireEvent.click(primaryRow.getByRole('button', { name: 'Delete' }))
    await waitFor(() => expect(meDeleteClientKey).toHaveBeenCalledTimes(1))
    expect(sdkMocks.meDeleteClientKey.mock.calls[0]?.[0].path).toEqual({
      key_id: 'key-1',
    })
  })

  it('keeps the row gate through the target-owner refresh and spins only the origin action', async () => {
    const mutation = deferred<{ data: ClientKey }>()
    const refresh = deferred<{ data: ClientKeyPageResponse }>()
    sdkMocks.meListClientKeys.mockReset()
    sdkMocks.meListClientKeys
      .mockResolvedValueOnce({ data: keyPage([makeKey('key-1', 'primary')]) })
      .mockReturnValueOnce(refresh.promise)
    sdkMocks.meUpdateClientKey.mockReturnValueOnce(mutation.promise)
    renderPage(operator)
    await screen.findAllByDisplayValue('primary')

    const rowElement = document.querySelector(
      '[data-key-row="key-1"]',
    ) as HTMLElement
    const row = within(rowElement)
    const toggle = row.getByRole('switch', { name: 'Status' })
    fireEvent.click(toggle)
    await waitFor(() => expect(meUpdateClientKey).toHaveBeenCalledTimes(1))
    expect((toggle as HTMLButtonElement).disabled).toBe(true)
    expect(rowElement.querySelectorAll('.animate-spin')).toHaveLength(1)

    mutation.resolve({ data: makeKey('key-1', 'primary', 2, false) })
    await waitFor(() => expect(meListClientKeys).toHaveBeenCalledTimes(2))
    expect((toggle as HTMLButtonElement).disabled).toBe(true)
    refresh.resolve({ data: keyPage([makeKey('key-1', 'primary', 2, false)]) })
    await waitFor(() =>
      expect((toggle as HTMLButtonElement).disabled).toBe(false),
    )
  })

  it('keeps a failed rename draft retryable and saves its trimmed label', async () => {
    let currentKey = makeKey('key-1', 'primary', 2)
    sdkMocks.meListClientKeys.mockImplementation(async ({ query }) => ({
      data: keyPage([currentKey], query?.first ?? 0, query?.rows ?? 10),
    }))
    sdkMocks.meUpdateClientKey
      .mockRejectedValueOnce({
        error: { code: 'fixture_rename_failed', message: 'rename failed' },
      })
      .mockImplementation(async ({ path, body }) => {
        currentKey = makeKey(path.key_id, body.label ?? currentKey.label, 2)
        return { data: currentKey }
      })
    renderPage(operator)
    await screen.findAllByDisplayValue('primary')

    const row = within(
      document.querySelector('[data-key-row="key-1"]') as HTMLElement,
    )
    const input = row.getByRole('textbox', { name: 'Name' }) as HTMLInputElement
    fireEvent.change(input, { target: { value: '  renamed owner key  ' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    const alert = await screen.findByRole('alert')
    expect(alert.textContent?.includes('rename failed')).toBe(true)
    expect(input.value).toBe('  renamed owner key  ')

    fireEvent.keyDown(input, { key: 'Enter' })
    await waitFor(() => expect(meUpdateClientKey).toHaveBeenCalledTimes(2))
    await waitFor(() => expect(input.value).toBe('renamed owner key'))
    expect(sdkMocks.meUpdateClientKey.mock.calls[1]?.[0].body).toEqual({
      label: 'renamed owner key',
    })
  })

  it('uses owner-scoped admin create, update, and delete paths', async () => {
    let ownerKeys: ClientKey[] = [makeKey('owner-key', 'before', 2)]
    sdkMocks.listClientKeys.mockImplementation(async ({ path, query }) => ({
      data: keyPage(
        ownerKeys.filter((key) => key.user_id === path.user_id),
        query?.first ?? 0,
        query?.rows ?? 10,
      ),
    }))
    sdkMocks.createClientKey.mockImplementation(async ({ path, body }) => {
      const created = {
        ...makeKey('created-owner-key', body.label ?? 'new key', path.user_id),
        secret: 'pf_fixture_secret_revealed_once',
      }
      ownerKeys = [...ownerKeys, created]
      return { data: created }
    })
    sdkMocks.updateClientKey.mockImplementation(async ({ path, body }) => {
      ownerKeys = ownerKeys.map((key) =>
        key.key_id === path.key_id
          ? { ...key, label: body.label ?? key.label }
          : key,
      )
      return { data: ownerKeys.find((key) => key.key_id === path.key_id)! }
    })
    sdkMocks.deleteClientKey.mockImplementation(async ({ path }) => {
      ownerKeys = ownerKeys.filter((key) => key.key_id !== path.key_id)
      return { data: undefined }
    })
    vi.spyOn(window, 'confirm').mockReturnValue(true)
    renderPage(admin)
    await screen.findAllByDisplayValue('primary')
    fireEvent.click(screen.getByRole('combobox', { name: 'Select user' }))
    fireEvent.click(await screen.findByRole('option', { name: /operator/ }))
    await screen.findAllByDisplayValue('before')

    fireEvent.click(screen.getByRole('button', { name: 'Generate key' }))
    const dialog = await screen.findByRole('dialog')
    fireEvent.change(
      within(dialog).getByRole('textbox', { name: 'Key label' }),
      {
        target: { value: 'owner-created' },
      },
    )
    fireEvent.click(
      within(dialog).getByRole('button', { name: 'Generate key' }),
    )
    expect(
      await within(dialog).findByText('pf_fixture_secret_revealed_once'),
    ).toBeTruthy()
    expect(sdkMocks.createClientKey.mock.calls[0]?.[0]).toEqual(
      expect.objectContaining({
        path: { user_id: 2 },
        body: { label: 'owner-created' },
      }),
    )
    expect(meListClientKeys).toHaveBeenCalledTimes(1)

    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    const createdRow = within(
      document.querySelector(
        '[data-key-row="created-owner-key"]',
      ) as HTMLElement,
    )
    const labelInput = createdRow.getByRole('textbox', { name: 'Name' })
    fireEvent.change(labelInput, { target: { value: 'owner-renamed' } })
    fireEvent.keyDown(labelInput, { key: 'Enter' })
    await waitFor(() => expect(updateClientKey).toHaveBeenCalledTimes(1))
    expect(sdkMocks.updateClientKey.mock.calls[0]?.[0]).toEqual(
      expect.objectContaining({
        path: { user_id: 2, key_id: 'created-owner-key' },
        body: { label: 'owner-renamed' },
      }),
    )

    fireEvent.click(createdRow.getByRole('button', { name: 'Delete' }))
    await waitFor(() => expect(deleteClientKey).toHaveBeenCalledTimes(1))
    expect(sdkMocks.deleteClientKey.mock.calls[0]?.[0].path).toEqual({
      user_id: 2,
      key_id: 'created-owner-key',
    })
  })

  it('reveals and copies a secret only when the response actually includes one', async () => {
    const secret = 'pf_fixture_secret_revealed_once'
    sdkMocks.meListClientKeys.mockResolvedValue({
      data: keyPage([
        makeKey('secret-key', 'secret-bearing', 2, true, secret),
        makeKey('no-secret-key', 'list-key-without-secret', 2, true, null),
      ]),
    })
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    })
    renderPage(operator)
    await screen.findAllByDisplayValue('secret-bearing')
    expect(screen.queryByText(secret)).toBeNull()

    const row = within(
      document.querySelector('[data-key-row="secret-key"]') as HTMLElement,
    )
    fireEvent.click(row.getByRole('button', { name: 'Show secret' }))
    expect(await row.findByText(secret)).toBeTruthy()
    fireEvent.click(row.getByRole('button', { name: 'Copy' }))
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(secret))

    const noSecretRow = within(
      document.querySelector('[data-key-row="no-secret-key"]') as HTMLElement,
    )
    expect(
      noSecretRow.queryByRole('button', { name: 'Show secret' }),
    ).toBeNull()
    expect(writeText).toHaveBeenCalledTimes(1)
  })
})
