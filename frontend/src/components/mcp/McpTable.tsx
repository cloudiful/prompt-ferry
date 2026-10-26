import { LoaderCircle, Pencil, RefreshCw, Trash2 } from 'lucide-react'
import { TestResultPopover } from '@/components/shared/TestResultPopover'
import { TablePagination } from '@/components/shared/TablePagination'
import { Button } from '@/components/ui/button'
import { Switch } from '@/components/ui/switch'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
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
  onPageChange: (change: TablePageChange) => void
  onTest: (server: McpServer) => void
  onToggle: (server: McpServer) => void
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
  onPageChange,
  onTest,
  onToggle,
  pendingActions,
  rows,
  total,
}: McpTableProps) {
  const { t } = useLocale()

  return (
    <section aria-busy={busy} className="grid min-w-0 gap-3">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t('name')}</TableHead>
            <TableHead>{t('status')}</TableHead>
            <TableHead>{t('test')}</TableHead>
            <TableHead>
              <span className="sr-only">{t('edit')}</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {items.length === 0 ? (
            <TableRow>
              <TableCell
                className="py-6 text-center text-sm text-muted-foreground"
                colSpan={4}
              >
                {busy ? (
                  <span
                    aria-label={t('loading')}
                    className="inline-flex items-center gap-2"
                    role="status"
                  >
                    <LoaderCircle
                      aria-hidden="true"
                      className="size-4 animate-spin"
                    />
                    {t('loading')}
                  </span>
                ) : (
                  t('noMcpServer')
                )}
              </TableCell>
            </TableRow>
          ) : (
            items.map((item) => {
              const action = pendingActions[mcpActionKey(item.server_id)]
              const testing = Object.hasOwn(
                pendingActions,
                mcpTestKey(item.server_id),
              )
              return (
                <TableRow
                  data-mcp-server-row={item.server_id}
                  key={item.server_id}
                >
                  <TableCell className="min-w-0 max-w-[36rem]">
                    <div className="grid min-w-0 gap-1">
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
                  </TableCell>
                  <TableCell>
                    <div className="flex items-center gap-2 whitespace-nowrap">
                      <Switch
                        aria-label={t('status')}
                        checked={item.enabled}
                        disabled={Boolean(action)}
                        onCheckedChange={() => onToggle(item.server)}
                      />
                      {action === 'toggle' ? (
                        <LoaderCircle
                          aria-label={t('loading')}
                          className="size-4 animate-spin text-muted-foreground"
                        />
                      ) : null}
                    </div>
                  </TableCell>
                  <TableCell>
                    <TestResultPopover
                      message={item.test_message}
                      severity={
                        item.test_ok === null
                          ? null
                          : item.test_ok
                            ? 'success'
                            : 'error'
                      }
                    />
                  </TableCell>
                  <TableCell>
                    <div className="flex justify-end gap-1">
                      <Button
                        aria-label={t('refreshCatalog')}
                        disabled={testing}
                        onClick={() => onTest(item.server)}
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
                      <Button
                        aria-label={t('edit')}
                        disabled={Boolean(action)}
                        onClick={() => onEdit(item.server)}
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
                        onClick={() => onDelete(item.server)}
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
                  </TableCell>
                </TableRow>
              )
            })
          )}
        </TableBody>
      </Table>
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
