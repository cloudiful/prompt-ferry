import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { ApiKeysTable } from '@/components/api-keys/ApiKeysTable'
import { KeyDialog } from '@/components/api-keys/KeyDialog'
import { useAsyncAction } from '@/lib/use-async-action'
import { clientKeyPageOptions, refreshClientKeyPage } from '@/lib/api-keys'
import { copyText } from '@/lib/clipboard'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import {
  clientKeyActionKey,
  clientKeyCreateKey,
  createApiKeysWorkspaceView,
  SELF_USER_ID,
} from '@/models/api-keys'
import type {
  CreateClientKeyRequest,
  UpdateClientKeyRequest,
} from '@/generated/admin-api'
import {
  createClientKeyMutation,
  deleteClientKeyMutation,
  listUserOptionsOptions,
  meCreateClientKeyMutation,
  meDeleteClientKeyMutation,
  meUpdateClientKeyMutation,
  updateClientKeyMutation,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { useStoredPageSize } from '@/components/shared/TablePagination'

export function ApiKeysPage() {
  const { t } = useLocale()
  const queryClient = useQueryClient()
  const asyncAction = useAsyncAction()
  const { notifyApiError, notifyError, notifySuccess } = useNotifier()
  const me = useSessionStore((state) => state.me)
  const isAdmin = me?.is_admin === true
  const [selectedUserId, setSelectedUserId] = useState(SELF_USER_ID)
  const [first, setFirst] = useState(0)
  const [rows, setRows] = useStoredPageSize('api-keys')
  const [visibleKeySecrets, setVisibleKeySecrets] = useState<
    Record<string, boolean>
  >({})
  const [createOpen, setCreateOpen] = useState(false)
  const [dialogOwnerId, setDialogOwnerId] = useState(SELF_USER_ID)
  const [generatedSecret, setGeneratedSecret] = useState('')
  const selectedUserIdRef = useRef(selectedUserId)
  const firstRef = useRef(first)
  const rowsRef = useRef(rows)
  const dialogOwnerRef = useRef(SELF_USER_ID)
  const dialogGenerationRef = useRef(0)
  const createOpenRef = useRef(false)
  const createButtonRef = useRef<HTMLButtonElement>(null)
  const notifiedErrors = useRef(new WeakSet<object>())

  selectedUserIdRef.current = selectedUserId
  firstRef.current = first
  rowsRef.current = rows

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

  const userOptionsQuery = useQuery({
    ...listUserOptionsOptions(),
    enabled: isAdmin && me !== null,
  })
  const keyPageQuery = useQuery({
    ...clientKeyPageOptions(selectedUserId, first, rows),
    enabled: me !== null,
  })

  const createSelfKey = useMutation(meCreateClientKeyMutation())
  const createOwnerKey = useMutation(createClientKeyMutation())
  const updateSelfKey = useMutation(meUpdateClientKeyMutation())
  const updateOwnerKey = useMutation(updateClientKeyMutation())
  const deleteSelfKey = useMutation(meDeleteClientKeyMutation())
  const deleteOwnerKey = useMutation(deleteClientKeyMutation())

  async function createKeyForOwner(
    ownerId: number,
    body: CreateClientKeyRequest,
  ) {
    return ownerId === SELF_USER_ID
      ? createSelfKey.mutateAsync({ body })
      : createOwnerKey.mutateAsync({ path: { user_id: ownerId }, body })
  }

  async function updateKeyForOwner(
    ownerId: number,
    keyId: string,
    body: UpdateClientKeyRequest,
  ) {
    return ownerId === SELF_USER_ID
      ? updateSelfKey.mutateAsync({ path: { key_id: keyId }, body })
      : updateOwnerKey.mutateAsync({
          path: { user_id: ownerId, key_id: keyId },
          body,
        })
  }

  async function deleteKeyForOwner(ownerId: number, keyId: string) {
    return ownerId === SELF_USER_ID
      ? deleteSelfKey.mutateAsync({ path: { key_id: keyId } })
      : deleteOwnerKey.mutateAsync({
          path: { user_id: ownerId, key_id: keyId },
        })
  }

  useEffect(() => {
    if (userOptionsQuery.error) reportApiError(userOptionsQuery.error)
  }, [reportApiError, userOptionsQuery.error])
  useEffect(() => {
    if (keyPageQuery.error) reportApiError(keyPageQuery.error)
  }, [keyPageQuery.error, reportApiError])

  useEffect(() => {
    const page = keyPageQuery.data
    if (!page || page.rows <= 0) return
    if (page.keys.length === 0 && page.first >= page.total) {
      const correctedFirst =
        page.total > 0
          ? Math.floor((page.total - 1) / page.rows) * page.rows
          : 0
      if (correctedFirst !== first) setFirst(correctedFirst)
      return
    }
    if (page.first !== first) setFirst(page.first)
    if (page.rows !== rows) setRows(page.rows)
  }, [first, keyPageQuery.data, rows, setRows])

  const users = userOptionsQuery.data?.users ?? []
  const selectedUser =
    selectedUserId === SELF_USER_ID
      ? null
      : (users.find((user) => user.user_id === selectedUserId) ?? null)
  const workspace = useMemo(
    () =>
      createApiKeysWorkspaceView({
        keys: keyPageQuery.data?.keys ?? [],
        selectedUser,
        visibleKeySecrets,
      }),
    [keyPageQuery.data?.keys, selectedUser, visibleKeySecrets],
  )
  const createPending = asyncAction.isPending(clientKeyCreateKey(dialogOwnerId))

  function changeOwner(value: string): void {
    const nextUserId = Number(value)
    if (!Number.isSafeInteger(nextUserId)) return
    selectedUserIdRef.current = nextUserId
    setSelectedUserId(nextUserId)
    setFirst(0)
    setVisibleKeySecrets({})
    setGeneratedSecret('')
  }

  async function refresh(): Promise<void> {
    const reads: Promise<unknown>[] = [keyPageQuery.refetch()]
    if (isAdmin) reads.push(userOptionsQuery.refetch())
    await Promise.all(reads)
  }

  function openCreateDialog(): void {
    dialogGenerationRef.current += 1
    dialogOwnerRef.current = selectedUserIdRef.current
    setDialogOwnerId(selectedUserIdRef.current)
    setGeneratedSecret('')
    createOpenRef.current = true
    setCreateOpen(true)
  }

  function changeCreateDialog(open: boolean): void {
    if (!open && createPending) return
    createOpenRef.current = open
    setCreateOpen(open)
    if (!open) {
      dialogGenerationRef.current += 1
      setGeneratedSecret('')
    }
  }

  async function runClientKeyWrite<T>(options: {
    key: string
    origin: string
    ownerId: number
    pageFirst: number
    pageRows: number
    write: () => Promise<T>
  }) {
    return asyncAction.run(
      options.key,
      async () => {
        const result = await options.write()
        const refreshedPage = await refreshClientKeyPage(
          queryClient,
          options.ownerId,
          options.pageFirst,
          options.pageRows,
        )
        if (
          selectedUserIdRef.current === options.ownerId &&
          firstRef.current === options.pageFirst &&
          rowsRef.current === options.pageRows
        ) {
          setFirst(refreshedPage.first)
          setRows(refreshedPage.rows)
        }
        return result
      },
      options.origin,
    )
  }

  async function submitCreateKey(label: string): Promise<void> {
    const capturedLabel = label.trim()
    if (!capturedLabel) {
      notifyError(t('keyLabelRequired'))
      return
    }

    const targetUserId = dialogOwnerRef.current
    const targetFirst = firstRef.current
    const targetRows = rowsRef.current
    const generation = dialogGenerationRef.current
    const isCurrentDialog = () =>
      createOpenRef.current &&
      dialogGenerationRef.current === generation &&
      dialogOwnerRef.current === targetUserId &&
      selectedUserIdRef.current === targetUserId
    if (!isCurrentDialog()) return

    try {
      const outcome = await runClientKeyWrite({
        key: clientKeyCreateKey(targetUserId),
        origin: 'save',
        ownerId: targetUserId,
        pageFirst: targetFirst,
        pageRows: targetRows,
        write: () => createKeyForOwner(targetUserId, { label: capturedLabel }),
      })
      if (!outcome.executed || !isCurrentDialog()) return
      setGeneratedSecret(outcome.value.secret)
      notifySuccess(t('saved'))
    } catch (cause) {
      if (isCurrentDialog()) reportApiError(cause)
    }
  }

  async function renameKey(keyId: string, label: string): Promise<boolean> {
    const ownerId = selectedUserIdRef.current
    const pageFirst = firstRef.current
    const pageRows = rowsRef.current
    try {
      const outcome = await runClientKeyWrite({
        key: clientKeyActionKey(keyId),
        origin: 'save',
        ownerId,
        pageFirst,
        pageRows,
        write: () => updateKeyForOwner(ownerId, keyId, { label: label.trim() }),
      })
      if (!outcome.executed) return false
      notifySuccess(t('saved'))
      return true
    } catch (cause) {
      reportApiError(cause)
      return false
    }
  }

  async function toggleKey(keyId: string): Promise<void> {
    const key = keyPageQuery.data?.keys.find((item) => item.key_id === keyId)
    if (!key) return
    const ownerId = selectedUserIdRef.current
    const pageFirst = firstRef.current
    const pageRows = rowsRef.current
    const payload: UpdateClientKeyRequest = { enabled: !key.enabled }
    try {
      await runClientKeyWrite({
        key: clientKeyActionKey(keyId),
        origin: 'toggle',
        ownerId,
        pageFirst,
        pageRows,
        write: () => updateKeyForOwner(ownerId, keyId, payload),
      })
    } catch (cause) {
      reportApiError(cause)
    }
  }

  async function deleteKey(keyId: string): Promise<void> {
    const key = keyPageQuery.data?.keys.find((item) => item.key_id === keyId)
    if (!key || !window.confirm(`${t('delete')} ${key.label}?`)) return
    const ownerId = selectedUserIdRef.current
    const pageFirst = firstRef.current
    const pageRows = rowsRef.current
    try {
      await runClientKeyWrite({
        key: clientKeyActionKey(keyId),
        origin: 'delete',
        ownerId,
        pageFirst,
        pageRows,
        write: () => deleteKeyForOwner(ownerId, keyId),
      })
    } catch (cause) {
      reportApiError(cause)
    }
  }

  function toggleKeySecret(keyId: string): void {
    setVisibleKeySecrets((current) => ({
      ...current,
      [keyId]: !current[keyId],
    }))
  }

  async function copySecret(secret: string | null | undefined): Promise<void> {
    if (!secret) return
    try {
      await copyText(secret)
    } catch (cause) {
      reportApiError(cause)
    }
  }

  const pageLoading = keyPageQuery.isPending
  const refreshing =
    keyPageQuery.isFetching || (isAdmin && userOptionsQuery.isFetching)

  return (
    <div
      className="grid min-w-0 max-w-full gap-3"
      data-foundation-route-id="R-API-KEYS"
    >
      <HeaderActions>
        {isAdmin && (
          <Select onValueChange={changeOwner} value={String(selectedUserId)}>
            <SelectTrigger
              aria-label={t('selectUser')}
              className="min-w-48 flex-1 sm:flex-none"
              disabled={userOptionsQuery.isPending}
            >
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={String(SELF_USER_ID)}>
                {`${t('self')} (${me?.login_name || t('user')})`}
              </SelectItem>
              {users.map((user) => (
                <SelectItem key={user.user_id} value={String(user.user_id)}>
                  {`${user.login_name}${user.display_name ? ` / ${user.display_name}` : ''}`}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        )}
        <Button
          aria-label={t('refresh')}
          onClick={() => void refresh()}
          size="sm"
          type="button"
          variant="outline"
        >
          {refreshing && (
            <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
          )}
          <span>{t('refresh')}</span>
        </Button>
        <Button
          aria-label={t('generateKey')}
          onClick={openCreateDialog}
          ref={createButtonRef}
          size="sm"
          type="button"
        >
          <span>{t('generateKey')}</span>
        </Button>
      </HeaderActions>

      <ApiKeysTable
        first={keyPageQuery.data?.first ?? first}
        loading={pageLoading}
        onCopyKeySecret={(keyId) => {
          const secret = keyPageQuery.data?.keys.find(
            (item) => item.key_id === keyId,
          )?.secret
          void copySecret(secret)
        }}
        onDeleteKey={(keyId) => void deleteKey(keyId)}
        onPage={(change) => {
          setFirst(change.first)
          setRows(change.rows)
        }}
        onRenameKey={renameKey}
        onToggleKey={(keyId) => void toggleKey(keyId)}
        onToggleKeySecret={toggleKeySecret}
        pendingActions={asyncAction.pendingActions}
        rows={keyPageQuery.data?.rows ?? rows}
        total={keyPageQuery.data?.total ?? 0}
        workspace={workspace}
      />

      <KeyDialog
        generatedSecret={generatedSecret}
        onCloseAutoFocus={(event) => {
          event.preventDefault()
          const button = createButtonRef.current
          if (button?.isConnected && !button.disabled) button.focus()
        }}
        onCopySecret={() => void copySecret(generatedSecret)}
        onOpenChange={changeCreateDialog}
        onSubmit={(label) => void submitCreateKey(label)}
        open={createOpen}
        pending={createPending}
      />
    </div>
  )
}
