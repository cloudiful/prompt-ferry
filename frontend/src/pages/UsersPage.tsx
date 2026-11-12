import { useCallback, useEffect, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { CreateUserDialog } from '@/components/users/CreateUserDialog'
import { ResetPasswordDialog } from '@/components/users/ResetPasswordDialog'
import { UsersTable } from '@/components/users/UsersTable'
import { useStoredPageSize } from '@/components/shared/TablePagination'
import type {
  CreateUserRequest,
  User,
  UserPageResponse,
  UserUpdate,
} from '@/generated/admin-api'
import {
  createUserMutation,
  deleteUserMutation,
  listUsersOptions,
  resetPasswordMutation,
  updateUserMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { refreshUsersPage, usersPagesQueryKey } from '@/lib/users'
import { useAsyncAction } from '@/lib/use-async-action'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import { userActionKey, type NewUserForm } from '@/models/users'
import type { TablePageChange } from '@/models/pagination'

export function UsersPage() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const { notifyApiError, notifySuccess } = useNotifier()
  const me = useSessionStore((state) => state.me)
  const [first, setFirst] = useState(0)
  const [rows, setRows] = useStoredPageSize('users')
  const [createOpen, setCreateOpen] = useState(false)
  const [resetUser, setResetUser] = useState<User | null>(null)
  const firstRef = useRef(first)
  const rowsRef = useRef(rows)
  const createOpenRef = useRef(false)
  const createGenerationRef = useRef(0)
  const resetUserRef = useRef<User | null>(null)
  const resetGenerationRef = useRef(0)
  const notifiedErrors = useRef(new WeakSet<object>())

  firstRef.current = first
  rowsRef.current = rows

  const query = useQuery({
    ...listUsersOptions({ query: { first, rows } }),
    enabled: me?.is_admin === true,
  })

  const createUser = useMutation(createUserMutation())
  const updateUser = useMutation(updateUserMutation())
  const removeUser = useMutation(deleteUserMutation())
  const resetUserPassword = useMutation(resetPasswordMutation())

  const reportApiError = useCallback(
    (cause: unknown) => {
      if (cause && typeof cause === 'object') {
        if (notifiedErrors.current.has(cause)) return
        notifiedErrors.current.add(cause)
      }
      notifyApiError(cause)
    },
    [notifyApiError],
  )

  useEffect(() => {
    if (query.error) reportApiError(query.error)
  }, [query.error, reportApiError])

  useEffect(() => {
    const page = query.data
    if (!page || page.rows <= 0) return
    if (page.users.length === 0 && page.first >= page.total) {
      const correctedFirst =
        page.total > 0
          ? Math.floor((page.total - 1) / page.rows) * page.rows
          : 0
      if (correctedFirst !== first) {
        firstRef.current = correctedFirst
        setFirst(correctedFirst)
      }
      return
    }
    if (page.first !== first) {
      firstRef.current = page.first
      setFirst(page.first)
    }
    if (page.rows !== rows) {
      rowsRef.current = page.rows
      setRows(page.rows)
    }
  }, [first, query.data, rows, setRows])

  function changePage(change: TablePageChange): void {
    firstRef.current = change.first
    rowsRef.current = change.rows
    setFirst(change.first)
    setRows(change.rows)
  }

  function setPageAfterWrite(
    page: UserPageResponse,
    capturedFirst: number,
    capturedRows: number,
  ): void {
    if (
      firstRef.current !== capturedFirst ||
      rowsRef.current !== capturedRows
    ) {
      return
    }
    firstRef.current = page.first
    rowsRef.current = page.rows
    setFirst(page.first)
    setRows(page.rows)
  }

  async function refresh(): Promise<void> {
    await query.refetch()
  }

  function openCreateDialog(): void {
    createGenerationRef.current += 1
    createOpenRef.current = true
    setCreateOpen(true)
  }

  function changeCreateDialog(open: boolean): void {
    if (!open && asyncAction.isPending(userActionKey(null))) return
    createOpenRef.current = open
    setCreateOpen(open)
    if (!open) createGenerationRef.current += 1
  }

  async function submitCreateUser(form: NewUserForm): Promise<boolean> {
    const generation = createGenerationRef.current
    const body: CreateUserRequest = {
      login_name: form.login_name,
      password: form.password,
      display_name: form.display_name,
      is_admin: form.is_admin,
    }
    const capturedFirst = firstRef.current
    const capturedRows = rowsRef.current

    try {
      const outcome = await asyncAction.run(
        userActionKey(null),
        async () => {
          const created = await createUser.mutateAsync({ body })
          const page = await refreshUsersPage(
            queryClient,
            capturedFirst,
            capturedRows,
          )
          setPageAfterWrite(page, capturedFirst, capturedRows)
          return created
        },
        'save',
      )
      if (!outcome.executed) return false
      if (createOpenRef.current && createGenerationRef.current === generation) {
        createOpenRef.current = false
        setCreateOpen(false)
        createGenerationRef.current += 1
      }
      notifySuccess(t('userCreated'))
      return true
    } catch (cause) {
      reportApiError(cause)
      return false
    }
  }

  async function saveUser(userId: number, input: UserUpdate): Promise<boolean> {
    const capturedUserId = userId
    const body: UserUpdate = {
      display_name: input.display_name,
      is_admin: input.is_admin,
      is_active: input.is_active,
    }
    try {
      const outcome = await asyncAction.run(
        userActionKey(capturedUserId),
        async () => {
          const saved = await updateUser.mutateAsync({
            path: { user_id: capturedUserId },
            body,
          })
          queryClient.setQueriesData<UserPageResponse>(
            { queryKey: usersPagesQueryKey() },
            (page) =>
              page
                ? {
                    ...page,
                    users: page.users.map((user) =>
                      user.user_id === saved.user_id ? saved : user,
                    ),
                  }
                : page,
          )
          return saved
        },
        'save',
      )
      if (!outcome.executed) return false
      notifySuccess(t('savedUser'))
      return true
    } catch (cause) {
      reportApiError(cause)
      return false
    }
  }

  async function deleteUser(user: User): Promise<void> {
    if (!window.confirm(`${t('delete')} ${user.login_name}?`)) return
    const capturedUserId = user.user_id
    const capturedFirst = firstRef.current
    const capturedRows = rowsRef.current
    try {
      const outcome = await asyncAction.run(
        userActionKey(capturedUserId),
        async () => {
          await removeUser.mutateAsync({ path: { user_id: capturedUserId } })
          const page = await refreshUsersPage(
            queryClient,
            capturedFirst,
            capturedRows,
          )
          setPageAfterWrite(page, capturedFirst, capturedRows)
        },
        'delete',
      )
      if (outcome.executed) notifySuccess(t('userDeleted'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  function openResetPasswordDialog(user: User): void {
    resetGenerationRef.current += 1
    resetUserRef.current = user
    setResetUser(user)
  }

  function changeResetDialog(open: boolean): void {
    const currentUser = resetUserRef.current
    if (
      !open &&
      currentUser &&
      asyncAction.isPending(userActionKey(currentUser.user_id))
    ) {
      return
    }
    if (!open) {
      resetUserRef.current = null
      resetGenerationRef.current += 1
      setResetUser(null)
    }
  }

  async function submitResetPassword(password: string): Promise<void> {
    const capturedUser = resetUserRef.current
    if (!capturedUser) return
    const capturedUserId = capturedUser.user_id
    const generation = resetGenerationRef.current

    try {
      const outcome = await asyncAction.run(
        userActionKey(capturedUserId),
        () =>
          resetUserPassword.mutateAsync({
            path: { user_id: capturedUserId },
            body: { password },
          }),
        'reset',
      )
      if (!outcome.executed) return
      if (
        resetGenerationRef.current === generation &&
        resetUserRef.current?.user_id === capturedUserId
      ) {
        resetUserRef.current = null
        setResetUser(null)
        resetGenerationRef.current += 1
      }
      notifySuccess(t('passwordReset'))
    } catch (cause) {
      reportApiError(cause)
    }
  }

  const createPending = asyncAction.isPending(userActionKey(null))
  const resetPending = resetUser
    ? asyncAction.isPending(userActionKey(resetUser.user_id))
    : false

  return (
    <section
      className="grid min-w-0 max-w-full gap-3"
      data-foundation-route-id="R-USERS"
    >
      <HeaderActions>
        <Button
          aria-label={t('refresh')}
          onClick={() => void refresh()}
          size="sm"
          type="button"
          variant="outline"
        >
          {query.isFetching && (
            <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
          )}
          <span>{t('refresh')}</span>
        </Button>
        <Button
          aria-label={t('createUser')}
          onClick={openCreateDialog}
          size="sm"
          type="button"
        >
          <span>{t('createUser')}</span>
        </Button>
      </HeaderActions>

      <UsersTable
        first={query.data?.first ?? first}
        loading={query.isPending}
        onDeleteUser={(user) => void deleteUser(user)}
        onOpenResetPassword={openResetPasswordDialog}
        onPage={changePage}
        onSaveUser={saveUser}
        pendingActions={asyncAction.pendingActions}
        rows={query.data?.rows ?? rows}
        total={query.data?.total ?? 0}
        users={query.data?.users ?? []}
      />

      <CreateUserDialog
        onOpenChange={changeCreateDialog}
        onSubmit={submitCreateUser}
        open={createOpen}
        pending={createPending}
      />
      <ResetPasswordDialog
        onOpenChange={changeResetDialog}
        onSubmit={(password) => void submitResetPassword(password)}
        open={resetUser !== null}
        pending={resetPending}
        user={resetUser}
      />
    </section>
  )
}
