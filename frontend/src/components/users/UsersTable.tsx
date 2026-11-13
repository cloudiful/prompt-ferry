import { useMemo, useRef, useState } from 'react'
import { KeyRound, LoaderCircle, Save, Trash2 } from 'lucide-react'
import type { User, UserUpdate } from '@/generated/admin-api'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
import { TablePagination } from '@/components/shared/TablePagination'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Input } from '@/components/ui/input'
import type { PendingActions } from '@/lib/async-action'
import { useLocale } from '@/lib/i18n'
import { userActionKey } from '@/models/users'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  type TablePageChange,
} from '@/models/pagination'

type UserDraft = Pick<User, 'display_name' | 'is_admin' | 'is_active'>

type UsersTableProps = {
  first: number
  loading: boolean
  onDeleteUser: (user: User) => void
  onOpenResetPassword: (user: User) => void
  onPage: (change: TablePageChange) => void
  onSaveUser: (userId: number, body: UserUpdate) => Promise<boolean>
  pendingActions: PendingActions
  rows: number
  total: number
  users: User[]
}

export function UsersTable({
  first,
  loading,
  onDeleteUser,
  onOpenResetPassword,
  onPage,
  onSaveUser,
  pendingActions,
  rows,
  total,
  users,
}: UsersTableProps) {
  const { t } = useLocale()
  const [drafts, setDrafts] = useState<Record<number, UserDraft>>({})
  const callbacks = useRef({ onDeleteUser, onOpenResetPassword, onSaveUser })
  callbacks.current = { onDeleteUser, onOpenResetPassword, onSaveUser }

  function valuesFor(user: User): UserDraft {
    return (
      drafts[user.user_id] ?? {
        display_name: user.display_name,
        is_admin: user.is_admin,
        is_active: user.is_active,
      }
    )
  }

  function updateDraft<K extends keyof UserDraft>(
    user: User,
    field: K,
    value: UserDraft[K],
  ): void {
    setDrafts((current) => ({
      ...current,
      [user.user_id]: {
        ...(current[user.user_id] ?? {
          display_name: user.display_name,
          is_admin: user.is_admin,
          is_active: user.is_active,
        }),
        [field]: value,
      },
    }))
  }

  function rowPending(user: User): string | null {
    return pendingActions[userActionKey(user.user_id)] ?? null
  }

  async function save(user: User): Promise<void> {
    const values = valuesFor(user)
    const saved = await callbacks.current.onSaveUser(user.user_id, {
      display_name: values.display_name,
      is_admin: values.is_admin,
      is_active: values.is_active,
    })
    if (saved) {
      setDrafts((current) => {
        const next = { ...current }
        delete next[user.user_id]
        return next
      })
    }
  }

  function renderDisplayName(user: User, mobile = false) {
    const pending = rowPending(user)
    return (
      <Input
        aria-label={t('displayName')}
        className={mobile ? 'min-w-0' : 'min-w-32'}
        disabled={pending !== null}
        onChange={(event) =>
          updateDraft(user, 'display_name', event.currentTarget.value)
        }
        title={valuesFor(user).display_name}
        value={valuesFor(user).display_name}
      />
    )
  }

  function renderAdminCheckbox(user: User) {
    const pending = rowPending(user)
    return (
      <Checkbox
        aria-label={t('userAdminToggle', { name: user.login_name })}
        checked={valuesFor(user).is_admin}
        disabled={pending !== null}
        onCheckedChange={(checked) =>
          updateDraft(user, 'is_admin', checked === true)
        }
      />
    )
  }

  function renderActiveCheckbox(user: User) {
    const pending = rowPending(user)
    return (
      <Checkbox
        aria-label={t('userActiveToggle', { name: user.login_name })}
        checked={valuesFor(user).is_active}
        disabled={pending !== null}
        onCheckedChange={(checked) =>
          updateDraft(user, 'is_active', checked === true)
        }
      />
    )
  }

  function renderActions(user: User, mobile = false) {
    const pending = rowPending(user)
    const buttonClass = mobile ? 'min-w-0' : ''
    return (
      <div
        className="flex flex-wrap items-center justify-end gap-1"
        data-user-actions={user.user_id}
      >
        <Button
          aria-label={t('resetPassword')}
          className={buttonClass}
          disabled={pending !== null}
          onClick={() => callbacks.current.onOpenResetPassword(user)}
          size="sm"
          type="button"
          variant="ghost"
        >
          {pending === 'reset' ? (
            <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
          ) : (
            <KeyRound aria-hidden="true" />
          )}
          {mobile && <span>{t('resetPassword')}</span>}
        </Button>
        <Button
          aria-label={t('save')}
          className={buttonClass}
          disabled={pending !== null}
          onClick={() => void save(user)}
          size="sm"
          type="button"
          variant="ghost"
        >
          {pending === 'save' ? (
            <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
          ) : (
            <Save aria-hidden="true" />
          )}
          {mobile && <span>{t('save')}</span>}
        </Button>
        <Button
          aria-label={t('delete')}
          className={`text-destructive ${buttonClass}`}
          disabled={pending !== null}
          onClick={() => callbacks.current.onDeleteUser(user)}
          size="sm"
          type="button"
          variant="ghost"
        >
          {pending === 'delete' ? (
            <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
          ) : (
            <Trash2 aria-hidden="true" />
          )}
          {mobile && <span>{t('delete')}</span>}
        </Button>
      </div>
    )
  }

  const columns = useMemo<Array<DataTableColumnDef<User>>>(
    () => [
      {
        id: 'login_name',
        header: t('user'),
        cell: ({ row }) => (
          <div
            className="max-w-64 whitespace-normal break-all font-semibold break-all"
            data-user-login={row.original.user_id}
            title={row.original.login_name}
          >
            {row.original.login_name}
          </div>
        ),
      },
      {
        id: 'display_name',
        header: t('displayName'),
        cell: ({ row }) => renderDisplayName(row.original),
      },
      {
        id: 'is_admin',
        header: t('adminShort'),
        cell: ({ row }) => renderAdminCheckbox(row.original),
      },
      {
        id: 'is_active',
        header: t('active'),
        cell: ({ row }) => renderActiveCheckbox(row.original),
      },
      {
        id: 'actions',
        header: () => <span className="sr-only">{t('save')}</span>,
        cell: ({ row }) => renderActions(row.original),
      },
    ],
    // The draft state and the pending actions drive the cell content.
    [drafts, pendingActions, t],
  )

  function renderCard(user: User) {
    return (
      <article
        className="grid min-w-0 gap-3 rounded-lg border bg-card p-3"
        data-user-row={user.user_id}
      >
        <div className="grid min-w-0 gap-1">
          <div
            className="break-all text-sm font-semibold"
            data-user-login={user.user_id}
          >
            {user.login_name}
          </div>
          <div className="text-xs text-muted-foreground">
            {t('id')} {user.user_id}
          </div>
        </div>
        <div className="grid gap-2">
          {renderDisplayName(user, true)}
          <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
            <label className="inline-flex min-h-8 items-center gap-2 text-xs">
              {renderAdminCheckbox(user)}
              {t('adminShort')}
            </label>
            <label className="inline-flex min-h-8 items-center gap-2 text-xs">
              {renderActiveCheckbox(user)}
              {t('active')}
            </label>
          </div>
        </div>
        {renderActions(user, true)}
      </article>
    )
  }

  return (
    <section className="grid min-w-0 max-w-full gap-3">
      {loading && users.length === 0 ? (
        <div aria-label={t('loading')} className="grid gap-2" role="status">
          <div className="h-9 animate-pulse rounded-md bg-muted" />
          <div className="h-9 animate-pulse rounded-md bg-muted" />
        </div>
      ) : users.length === 0 ? (
        <div className="rounded-xl border bg-card px-4 py-6 text-sm text-muted-foreground">
          {t('noUser')}
        </div>
      ) : (
        <DataTable
          columns={columns}
          data={users}
          getRowId={(user) => String(user.user_id)}
          label={t('user')}
          renderCard={renderCard}
          rowAttributes={(user) => ({ 'data-user-row': user.user_id })}
        />
      )}
      <TablePagination
        first={first}
        onChange={onPage}
        pageSizeOptions={STANDARD_PAGE_SIZE_OPTIONS}
        rows={rows}
        total={total}
      />
    </section>
  )
}
