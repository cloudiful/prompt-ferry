import { useMemo, useRef } from 'react'
import { KeyRound, LoaderCircle, Pencil, RefreshCw, Trash2 } from 'lucide-react'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
import {
  TestResultPopover,
  testResultKey,
  type TestResultSurface,
} from '@/components/shared/TestResultPopover'
import { TablePagination } from '@/components/shared/TablePagination'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import { useLocale } from '@/lib/i18n'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  type TablePageChange,
} from '@/models/pagination'
import { mcpActionKey, mcpTestKey } from '@/lib/mcp'
import type { McpServer } from '@/generated/admin-api'
import type { McpServerListItemView } from '@/models'

type McpTableProps = {
  busy: boolean
  first: number
  items: McpServerListItemView[]
  onDelete: (server: McpServer) => void
  onEdit: (server: McpServer) => void
  onOAuth?: (server: McpServer) => void
  onPageChange: (change: TablePageChange) => void
  onResultOpenChange: (key: string, open: boolean) => void
  onTest: (server: McpServer, surface: TestResultSurface) => void
  onToggle: (server: McpServer) => void
  openResultKey: string | null
  pendingActions: Readonly<Record<string, string>>
  rows: number
  total: number
}

export function McpTable({
  busy,
  first,
  items,
  onDelete,
  onEdit,
  onOAuth,
  onPageChange,
  onResultOpenChange,
  onTest,
  onToggle,
  openResultKey,
  pendingActions,
  rows,
  total,
}: McpTableProps) {
  const { t } = useLocale()
  const callbacks = useRef({ onDelete, onEdit, onOAuth, onTest, onToggle })
  callbacks.current = { onDelete, onEdit, onOAuth, onTest, onToggle }

  function renderName(item: McpServerListItemView) {
    return (
      <div className="grid min-w-0 max-w-[36rem] gap-1">
        <span
          className="block min-w-0 truncate font-semibold"
          title={item.name}
        >
          {item.name}
        </span>
        <span
          className="block min-w-0 truncate text-xs text-muted-foreground"
          title={item.endpoint_label}
        >
          {item.endpoint_label}
        </span>
      </div>
    )
  }

  function renderStatus(item: McpServerListItemView) {
    const action = pendingActions[mcpActionKey(item.server_id)]
    return (
      <div className="flex items-center gap-2 whitespace-nowrap">
        <Switch
          aria-label={t('status')}
          checked={item.enabled}
          disabled={Boolean(action)}
          onCheckedChange={() => callbacks.current.onToggle(item.server)}
        />
        {action === 'toggle' ? (
          <LoaderCircle
            aria-label={t('loading')}
            className="size-4 animate-spin text-muted-foreground"
          />
        ) : null}
      </div>
    )
  }

  function renderActions(item: McpServerListItemView) {
    const action = pendingActions[mcpActionKey(item.server_id)]
    const testing = Object.hasOwn(pendingActions, mcpTestKey(item.server_id))
    const resultKey = testResultKey('desktop', item.server_id)
    return (
      <div className="flex justify-end gap-1">
        <TestResultPopover
          message={item.test_message}
          onOpenChange={(open) => onResultOpenChange(resultKey, open)}
          open={openResultKey === resultKey}
          severity={
            item.test_ok === null ? null : item.test_ok ? 'success' : 'error'
          }
        >
          <Button
            aria-label={t('refreshCatalog')}
            disabled={testing}
            onClick={() => callbacks.current.onTest(item.server, 'desktop')}
            size="icon-sm"
            title={t('refreshCatalog')}
            type="button"
            variant="ghost"
          >
            {testing ? (
              <LoaderCircle className="animate-spin" />
            ) : (
              <RefreshCw />
            )}
          </Button>
        </TestResultPopover>
        {item.server.auth_mode === 'oauth' ? (
          <Button
            aria-label={t('oauthManage')}
            disabled={Boolean(action)}
            onClick={() => callbacks.current.onOAuth?.(item.server)}
            size="icon-sm"
            title={t('oauthManage')}
            type="button"
            variant="ghost"
          >
            <KeyRound />
          </Button>
        ) : null}
        <Button
          aria-label={t('edit')}
          disabled={Boolean(action)}
          onClick={() => callbacks.current.onEdit(item.server)}
          size="icon-sm"
          title={t('edit')}
          type="button"
          variant="ghost"
        >
          <Pencil />
        </Button>
        <Button
          aria-label={t('delete')}
          disabled={Boolean(action)}
          onClick={() => callbacks.current.onDelete(item.server)}
          size="icon-sm"
          title={t('delete')}
          type="button"
          variant="ghost"
        >
          {action === 'delete' ? (
            <LoaderCircle className="animate-spin" />
          ) : (
            <Trash2 />
          )}
        </Button>
      </div>
    )
  }

  const columns = useMemo<Array<DataTableColumnDef<McpServerListItemView>>>(
    () => [
      {
        id: 'name',
        header: t('name'),
        cell: ({ row }) => renderName(row.original),
      },
      {
        id: 'status',
        header: t('status'),
        cell: ({ row }) => renderStatus(row.original),
      },
      {
        id: 'actions',
        header: () => <span className="sr-only">{t('edit')}</span>,
        cell: ({ row }) => renderActions(row.original),
      },
    ],
    [openResultKey, pendingActions, t],
  )

  return (
    <section aria-busy={busy} className="grid min-w-0 gap-3">
      <DataTable
        columns={columns}
        data={items}
        emptyLabel={t('noMcpServer')}
        getRowId={(item) => item.server_id}
        label={t('mcp')}
        loading={busy}
        rowAttributes={(item) => ({ 'data-mcp-server-row': item.server_id })}
      />
      <TablePagination
        first={first}
        onChange={onPageChange}
        pageSizeOptions={STANDARD_PAGE_SIZE_OPTIONS}
        rows={rows}
        total={total}
      />
    </section>
  )
}
