import { useState } from 'react'
import { LoaderCircle } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Switch } from '@/components/ui/switch'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { TablePagination } from '@/components/shared/TablePagination'
import type { PendingActions } from '@/lib/async-action'
import { useLocale } from '@/lib/i18n'
import { clientKeyActionKey } from '@/models/api-keys'
import type { ApiKeyItemView, ApiKeysWorkspaceView } from '@/models/api-keys'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  type TablePageChange,
} from '@/models/pagination'

type ApiKeysTableProps = {
  first: number
  loading: boolean
  onCopyKeySecret: (keyId: string) => void
  onDeleteKey: (keyId: string) => void
  onPage: (change: TablePageChange) => void
  onRenameKey: (keyId: string, label: string) => Promise<boolean>
  onToggleKey: (keyId: string) => void
  onToggleKeySecret: (keyId: string) => void
  pendingActions: PendingActions
  rows: number
  total: number
  workspace: ApiKeysWorkspaceView
}

export function ApiKeysTable({
  first,
  loading,
  onCopyKeySecret,
  onDeleteKey,
  onPage,
  onRenameKey,
  onToggleKey,
  onToggleKeySecret,
  pendingActions,
  rows,
  total,
  workspace,
}: ApiKeysTableProps) {
  const { t } = useLocale()
  const [drafts, setDrafts] = useState<Record<string, string>>({})

  function draftLabel(item: ApiKeyItemView): string {
    return drafts[item.key_id] ?? item.label
  }

  function updateDraft(keyId: string, value: string): void {
    setDrafts((current) => ({ ...current, [keyId]: value }))
  }

  function clearDraft(keyId: string): void {
    setDrafts((current) => {
      const next = { ...current }
      delete next[keyId]
      return next
    })
  }

  async function commitLabel(item: ApiKeyItemView): Promise<void> {
    const draft = drafts[item.key_id]
    if (draft === undefined) return
    const label = draft.trim()
    if (!label || label === item.label) {
      clearDraft(item.key_id)
      return
    }
    if (await onRenameKey(item.key_id, label)) clearDraft(item.key_id)
  }

  function rowPending(keyId: string): string | null {
    return pendingActions[clientKeyActionKey(keyId)] ?? null
  }

  function renderKeyLabelInput(item: ApiKeyItemView, mobile = false) {
    const pending = rowPending(item.key_id)
    return (
      <div className="flex min-w-0 items-center gap-2">
        <Input
          aria-label={t('name')}
          className={mobile ? 'min-w-0 flex-1' : 'w-48 max-w-full'}
          disabled={pending !== null}
          onBlur={() => void commitLabel(item)}
          onChange={(event) =>
            updateDraft(item.key_id, event.currentTarget.value)
          }
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              event.preventDefault()
              void commitLabel(item)
            }
          }}
          title={item.label}
          value={draftLabel(item)}
        />
        {pending === 'save' && (
          <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
        )}
      </div>
    )
  }

  function renderKeySecret(item: ApiKeyItemView) {
    if (!item.secret)
      return <span className="text-xs text-muted-foreground">-</span>

    return (
      <div className="flex min-w-0 flex-wrap items-center gap-2">
        {item.visible_secret && (
          <span className="max-w-72 overflow-x-auto rounded-md bg-muted px-2 py-1.5 font-mono text-xs">
            {item.secret}
          </span>
        )}
        <Button
          aria-label={t(item.visible_secret ? 'hideSecret' : 'showSecret')}
          onClick={() => onToggleKeySecret(item.key_id)}
          size="sm"
          type="button"
          variant="ghost"
        >
          {t(item.visible_secret ? 'hideSecret' : 'showSecret')}
        </Button>
        <Button
          aria-label={t('copy')}
          onClick={() => onCopyKeySecret(item.key_id)}
          size="sm"
          type="button"
          variant="ghost"
        >
          {t('copy')}
        </Button>
      </div>
    )
  }

  function renderKeyStatus(item: ApiKeyItemView) {
    const pending = rowPending(item.key_id)
    return (
      <div className="flex items-center gap-2">
        <Switch
          aria-label={t('status')}
          checked={item.enabled}
          disabled={pending !== null}
          onCheckedChange={() => onToggleKey(item.key_id)}
        />
        {pending === 'toggle' && (
          <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
        )}
      </div>
    )
  }

  function renderDeleteKeyButton(item: ApiKeyItemView) {
    const pending = rowPending(item.key_id)
    return (
      <Button
        aria-label={t('delete')}
        className="text-destructive"
        disabled={pending !== null}
        onClick={() => onDeleteKey(item.key_id)}
        size="sm"
        type="button"
        variant="ghost"
      >
        {pending === 'delete' && (
          <LoaderCircle aria-hidden="true" className="size-4 animate-spin" />
        )}
        {t('delete')}
      </Button>
    )
  }

  return (
    <section className="grid min-w-0 max-w-full gap-3">
      {loading && workspace.key_items.length === 0 ? (
        <div aria-label={t('loading')} className="grid gap-2" role="status">
          <div className="h-9 animate-pulse rounded-md bg-muted" />
          <div className="h-9 animate-pulse rounded-md bg-muted" />
        </div>
      ) : !workspace.has_keys ? (
        <div className="rounded-xl border bg-card px-4 py-6 text-sm text-muted-foreground">
          {t('noClientKeys')}
        </div>
      ) : (
        <>
          <div className="grid gap-3 md:hidden">
            {workspace.key_items.map((item) => (
              <article
                className="grid min-w-0 gap-3 rounded-lg border bg-card p-3"
                data-key-row={item.key_id}
                key={item.key_id}
              >
                {renderKeyLabelInput(item, true)}
                <div className="flex items-center justify-between gap-3">
                  <span className="text-xs font-semibold text-muted-foreground">
                    {t('active')}
                  </span>
                  {renderKeyStatus(item)}
                </div>
                {renderKeySecret(item)}
                <div className="flex justify-end">
                  {renderDeleteKeyButton(item)}
                </div>
              </article>
            ))}
          </div>

          <div className="hidden min-w-0 md:block">
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t('name')}</TableHead>
                  <TableHead>{t('status')}</TableHead>
                  <TableHead>{t('apiKey')}</TableHead>
                  <TableHead>
                    <span className="sr-only">{t('delete')}</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {workspace.key_items.map((item) => (
                  <TableRow data-key-row={item.key_id} key={item.key_id}>
                    <TableCell>{renderKeyLabelInput(item)}</TableCell>
                    <TableCell>{renderKeyStatus(item)}</TableCell>
                    <TableCell>{renderKeySecret(item)}</TableCell>
                    <TableCell className="text-right">
                      {renderDeleteKeyButton(item)}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        </>
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
