import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  CreateUserRequest,
  MeResponse,
  User,
  UserPageResponse,
  UserUpdate,
} from '@/generated/admin-api'
import {
  createUser,
  deleteUser,
  listUsers,
  resetPassword,
  updateUser,
} from '@/generated/admin-api'
import { AdminGuard } from '@/lib/router'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import { UsersPage } from '@/pages/UsersPage'

vi.mock('@/generated/admin-api', () => ({
  authLogin: vi.fn(),
  authLogout: vi.fn(),
  authMe: vi.fn(),
  bridgeStatus: vi.fn(),
  createUser: vi.fn(),
  deleteUser: vi.fn(),
  listUsers: vi.fn(),
  relayAuthLogin: vi.fn(),
  relayAuthLogout: vi.fn(),
  relayAuthMe: vi.fn(),
  resetPassword: vi.fn(),
  updateUser: vi.fn(),
}))

const sdkMocks = {
  createUser: vi.mocked(createUser) as unknown as ReturnType<typeof vi.fn>,
  deleteUser: vi.mocked(deleteUser) as unknown as ReturnType<typeof vi.fn>,
  listUsers: vi.mocked(listUsers) as unknown as ReturnType<typeof vi.fn>,
  resetPassword: vi.mocked(resetPassword) as unknown as ReturnType<
    typeof vi.fn
  >,
  updateUser: vi.mocked(updateUser) as unknown as ReturnType<typeof vi.fn>,
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

function makeUser(
  userId: number,
  loginName: string,
  options: Partial<User> = {},
): User {
  return {
    created_at: '2026-10-16T12:00:00.000Z',
    display_name: `Display ${loginName}`,
    is_active: true,
    is_admin: false,
    login_name: loginName,
    updated_at: '2026-10-23T12:00:00.000Z',
    user_id: userId,
    ...options,
  }
}

function pageResponse(
  users: User[],
  first = 0,
  rows = 10,
  total = users.length,
): UserPageResponse {
  return { first, rows, total, users }
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
          <UsersPage />
        </NotifierProvider>
      </LocaleProvider>
    </QueryClientProvider>,
  )
  return { ...view, queryClient }
}

function userRow(userId: number, occurrence = 0): HTMLElement {
  return document.querySelectorAll(`[data-user-row="${userId}"]`)[
    occurrence
  ] as HTMLElement
}

beforeEach(() => {
  vi.resetAllMocks()
  Element.prototype.scrollIntoView = () => undefined
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
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
  })
  sdkMocks.listUsers.mockResolvedValue({
    data: pageResponse([
      makeUser(1, 'admin', {
        display_name: 'Fixture Administrator',
        is_admin: true,
      }),
      makeUser(2, 'operator', { display_name: 'Fixture Operator' }),
    ]),
  })
  sdkMocks.createUser.mockImplementation(
    async ({ body }: { body: CreateUserRequest }) => ({
      data: makeUser(3, body.login_name, {
        display_name: body.display_name,
        is_admin: body.is_admin === true,
      }),
    }),
  )
  sdkMocks.updateUser.mockImplementation(
    async ({
      path,
      body,
    }: {
      path: { user_id: number }
      body: UserUpdate
    }) => ({
      data: makeUser(path.user_id, path.user_id === 1 ? 'admin' : 'operator', {
        display_name: body.display_name ?? '',
        is_active: body.is_active ?? true,
        is_admin: body.is_admin ?? false,
      }),
    }),
  )
  sdkMocks.deleteUser.mockResolvedValue({ data: undefined })
  sdkMocks.resetPassword.mockResolvedValue({ data: undefined })
})

afterEach(() => {
  localStorage.clear()
  useSessionStore.setState({
    me: null,
    bootstrapped: false,
    bootstrapping: false,
  })
})

describe('users route', () => {
  it('redirects non-admins before mounting the route or calling the admin API', async () => {
    useSessionStore.setState({
      me: operator,
      bootstrapped: true,
      bootstrapping: false,
    })
    render(
      <LocaleProvider>
        <MemoryRouter initialEntries={['/users']}>
          <Routes>
            <Route
              element={
                <AdminGuard>
                  <div data-testid="admin-content">Users</div>
                </AdminGuard>
              }
              path="/users"
            />
            <Route
              element={<div data-testid="api-keys">API keys</div>}
              path="/api-keys"
            />
          </Routes>
        </MemoryRouter>
      </LocaleProvider>,
    )

    expect(await screen.findByTestId('api-keys')).toBeTruthy()
    expect(screen.queryByTestId('admin-content')).toBeNull()
    expect(sdkMocks.listUsers).not.toHaveBeenCalled()
  })

  it('loads the paged list without loading user options', async () => {
    renderPage(admin)

    expect(
      await screen.findAllByDisplayValue('Fixture Administrator'),
    ).toHaveLength(2)
    expect(sdkMocks.listUsers).toHaveBeenCalledWith(
      expect.objectContaining({ query: { first: 0, rows: 10 } }),
    )
    expect(sdkMocks.listUsers).toHaveBeenCalledTimes(1)
    expect(screen.getByRole('button', { name: 'Create user' })).toBeTruthy()
  })

  it('shows the existing empty-users state and refreshes the same page', async () => {
    sdkMocks.listUsers.mockResolvedValue({ data: pageResponse([]) })
    renderPage(admin)

    expect(await screen.findByText('No users')).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }))
    await waitFor(() => expect(sdkMocks.listUsers).toHaveBeenCalledTimes(2))
    expect(screen.getByText('No users')).toBeTruthy()
  })

  it('saves the exact full-row update and patches the cached page without refetching', async () => {
    renderPage(admin)
    await screen.findAllByDisplayValue('Fixture Operator')
    const row = within(userRow(2))
    fireEvent.change(row.getByRole('textbox', { name: 'Display name' }), {
      target: { value: 'Operator Updated' },
    })
    fireEvent.click(
      row.getByRole('checkbox', { name: 'Toggle admin for operator' }),
    )
    fireEvent.click(
      row.getByRole('checkbox', { name: 'Toggle active for operator' }),
    )
    fireEvent.click(row.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(sdkMocks.updateUser).toHaveBeenCalledTimes(1))
    expect(sdkMocks.updateUser.mock.calls[0]?.[0]).toMatchObject({
      path: { user_id: 2 },
      body: {
        display_name: 'Operator Updated',
        is_admin: true,
        is_active: false,
      },
    })
    expect(sdkMocks.listUsers).toHaveBeenCalledTimes(1)
    expect(await screen.findAllByDisplayValue('Operator Updated')).toHaveLength(
      2,
    )
    expect(await screen.findByRole('status')).toBeTruthy()
  })

  it('keeps create drafts after failure, sends raw fields, and closes only after success', async () => {
    const pendingCreate = deferred<{ data: User }>()
    sdkMocks.createUser
      .mockRejectedValueOnce({
        error: { code: 'fixture_create_failed', message: 'create failed' },
      })
      .mockReturnValueOnce(pendingCreate.promise)
    renderPage(admin)
    await screen.findAllByDisplayValue('Fixture Administrator')
    fireEvent.click(screen.getByRole('button', { name: 'Create user' }))

    const dialog = await screen.findByRole('dialog')
    const loginName = within(dialog).getByRole('textbox', {
      name: 'Login name',
    })
    const displayName = within(dialog).getByRole('textbox', {
      name: 'Display name',
    })
    const password = within(dialog).getByLabelText('Initial password')
    fireEvent.change(loginName, { target: { value: '  fixture-created  ' } })
    fireEvent.change(displayName, { target: { value: ' Fixture Created ' } })
    fireEvent.change(password, { target: { value: ' fixture-password ' } })
    fireEvent.click(within(dialog).getByRole('checkbox'))
    fireEvent.click(within(dialog).getByRole('button', { name: 'Create' }))

    expect(await screen.findByRole('alert')).toBeTruthy()
    expect((loginName as HTMLInputElement).value).toBe('  fixture-created  ')
    expect((displayName as HTMLInputElement).value).toBe(' Fixture Created ')
    expect((password as HTMLInputElement).value).toBe(' fixture-password ')
    expect((password as HTMLInputElement).type).toBe('password')
    expect(sdkMocks.createUser.mock.calls[0]?.[0].body).toEqual({
      login_name: '  fixture-created  ',
      display_name: ' Fixture Created ',
      password: ' fixture-password ',
      is_admin: true,
    })

    fireEvent.click(within(dialog).getByRole('button', { name: 'Create' }))
    await waitFor(() =>
      expect(
        (
          within(dialog).getByRole('button', {
            name: 'Create',
          }) as HTMLButtonElement
        ).disabled,
      ).toBe(true),
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

    pendingCreate.resolve({
      data: makeUser(3, '  fixture-created  ', {
        display_name: ' Fixture Created ',
        is_admin: true,
      }),
    })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(sdkMocks.createUser).toHaveBeenCalledTimes(2)
    expect(sdkMocks.listUsers).toHaveBeenCalledTimes(2)
  })

  it('corrects the last page after deleting its final user and honors cancellation', async () => {
    let currentUsers = Array.from({ length: 11 }, (_, index) =>
      makeUser(index + 1, `fixture-user-${index + 1}`),
    )
    sdkMocks.listUsers.mockImplementation(
      async ({ query }: { query: { first?: number; rows?: number } }) => {
        const first = query.first ?? 0
        const rows = query.rows ?? 10
        return {
          data: pageResponse(
            currentUsers.slice(first, first + rows),
            first,
            rows,
            currentUsers.length,
          ),
        }
      },
    )
    sdkMocks.deleteUser.mockImplementation(
      async ({ path }: { path: { user_id: number } }) => {
        currentUsers = currentUsers.filter(
          (user) => user.user_id !== path.user_id,
        )
        return { data: undefined }
      },
    )
    const confirm = vi.spyOn(window, 'confirm')
    confirm.mockReturnValueOnce(false).mockReturnValueOnce(true)
    renderPage(admin)

    expect(await screen.findAllByText('fixture-user-1')).toHaveLength(2)
    expect(localStorage.getItem('prompt-ferry:table-page-size:users')).toBe(
      '10',
    )
    fireEvent.click(screen.getByRole('button', { name: 'Next page' }))
    expect(await screen.findAllByText('fixture-user-11')).toHaveLength(2)
    expect(sdkMocks.listUsers).toHaveBeenCalledWith(
      expect.objectContaining({ query: { first: 10, rows: 10 } }),
    )
    const lastRow = within(userRow(11))
    fireEvent.click(lastRow.getByRole('button', { name: 'Delete' }))
    expect(confirm).toHaveBeenCalledWith('Delete fixture-user-11?')
    expect(sdkMocks.deleteUser).not.toHaveBeenCalled()

    fireEvent.click(lastRow.getByRole('button', { name: 'Delete' }))
    await waitFor(() => expect(sdkMocks.deleteUser).toHaveBeenCalledTimes(1))
    expect(await screen.findAllByText('fixture-user-1')).toHaveLength(2)
    expect(
      screen.getByRole('button', { name: 'Page 1 (current)' }),
    ).toBeTruthy()
    expect(sdkMocks.listUsers).toHaveBeenLastCalledWith(
      expect.objectContaining({ query: { first: 0, rows: 10 } }),
    )
  })

  it('shares each row guard across reset/save/delete while different users remain independent', async () => {
    const pendingUpdate = deferred<{ data: User }>()
    const pendingReset = deferred<{ data: undefined }>()
    sdkMocks.updateUser.mockReturnValueOnce(pendingUpdate.promise)
    sdkMocks.resetPassword
      .mockReturnValueOnce(pendingReset.promise)
      .mockResolvedValue({ data: undefined })
    renderPage(admin)
    await screen.findAllByDisplayValue('Fixture Administrator')

    const operatorRow = within(userRow(2))
    fireEvent.click(operatorRow.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(sdkMocks.updateUser).toHaveBeenCalledTimes(1))
    const operatorReset = operatorRow.getByRole('button', {
      name: 'Reset password',
    }) as HTMLButtonElement
    const operatorDelete = operatorRow.getByRole('button', {
      name: 'Delete',
    }) as HTMLButtonElement
    expect(operatorReset.disabled).toBe(true)
    expect(operatorDelete.disabled).toBe(true)

    const adminRow = within(userRow(1))
    fireEvent.click(adminRow.getByRole('button', { name: 'Reset password' }))
    const resetDialog = await screen.findByRole('dialog')
    fireEvent.change(within(resetDialog).getByLabelText('New password'), {
      target: { value: 'first-reset-secret' },
    })
    fireEvent.click(within(resetDialog).getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(sdkMocks.resetPassword).toHaveBeenCalledTimes(1))

    const adminSave = adminRow.getByRole('button', {
      name: 'Save',
      hidden: true,
    }) as HTMLButtonElement
    const adminDelete = adminRow.getByRole('button', {
      name: 'Delete',
      hidden: true,
    }) as HTMLButtonElement
    expect(adminSave.disabled).toBe(true)
    expect(adminDelete.disabled).toBe(true)

    pendingUpdate.resolve({
      data: makeUser(2, 'operator', { display_name: 'Fixture Operator' }),
    })
    await waitFor(() => expect(operatorReset.disabled).toBe(false))
    expect(adminSave.disabled).toBe(true)

    fireEvent.click(
      operatorRow.getByRole('button', {
        name: 'Reset password',
        hidden: true,
      }),
    )
    const nextResetDialog = await screen.findByRole('dialog')
    expect(within(nextResetDialog).getByText('operator')).toBeTruthy()
    expect(
      (
        within(nextResetDialog).getByLabelText(
          'New password',
        ) as HTMLInputElement
      ).value,
    ).toBe('')

    pendingReset.resolve({ data: undefined })
    await waitFor(() => expect(screen.getByRole('dialog')).toBeTruthy())
    expect(
      within(screen.getByRole('dialog')).getByText('operator'),
    ).toBeTruthy()
    expect(adminSave.disabled).toBe(false)
    expect(
      (
        within(screen.getByRole('dialog')).getByLabelText(
          'New password',
        ) as HTMLInputElement
      ).value,
    ).toBe('')

    fireEvent.change(
      within(screen.getByRole('dialog')).getByLabelText('New password'),
      { target: { value: 'second-reset-secret' } },
    )
    fireEvent.click(
      within(screen.getByRole('dialog')).getByRole('button', { name: 'Save' }),
    )
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    expect(sdkMocks.resetPassword).toHaveBeenCalledTimes(2)
    expect(
      sdkMocks.resetPassword.mock.calls.map(([options]) => options),
    ).toEqual([
      expect.objectContaining({
        path: { user_id: 1 },
        body: { password: 'first-reset-secret' },
      }),
      expect.objectContaining({
        path: { user_id: 2 },
        body: { password: 'second-reset-secret' },
      }),
    ])

    fireEvent.click(operatorRow.getByRole('button', { name: 'Reset password' }))
    const clearedDialog = await screen.findByRole('dialog')
    expect(
      (within(clearedDialog).getByLabelText('New password') as HTMLInputElement)
        .value,
    ).toBe('')
    fireEvent.click(
      within(clearedDialog).getByRole('button', { name: 'Cancel' }),
    )
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
  })

  it('persists the selected user page size', async () => {
    renderPage(admin)
    await screen.findAllByDisplayValue('Fixture Administrator')
    fireEvent.click(screen.getByRole('combobox', { name: 'Rows per page' }))
    fireEvent.click(await screen.findByRole('option', { name: '20' }))

    await waitFor(() =>
      expect(localStorage.getItem('prompt-ferry:table-page-size:users')).toBe(
        '20',
      ),
    )
    expect(sdkMocks.listUsers).toHaveBeenCalledWith(
      expect.objectContaining({ query: { first: 0, rows: 20 } }),
    )
  })
})
