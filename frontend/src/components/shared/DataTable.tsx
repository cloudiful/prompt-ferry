import type { HTMLAttributes, ReactNode } from 'react'
import {
  tableFeatures,
  useTable,
  type ColumnDef,
  type RowData,
} from '@tanstack/react-table'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { useLocale } from '@/lib/i18n'

export const dataTableFeatures = tableFeatures({})

export type DataTableColumnDef<TData extends RowData> = ColumnDef<
  typeof dataTableFeatures,
  TData
>

export type DataTableProps<TData extends RowData> = {
  columns: Array<DataTableColumnDef<TData>>
  data: Array<TData>
  getRowId?: (row: TData, index: number) => string
  label: string
  loading?: boolean
  busy?: boolean
  emptyLabel?: string
  className?: string
  /**
   * The attributes of one row, for the markers a domain reads back from the
   * rendered list.
   */
  rowAttributes?: (row: TData) => Record<string, string | number | undefined>
  /**
   * The native props of one row, for the interactions a domain owns.
   */
  getRowProps?: (row: TData) => HTMLAttributes<HTMLTableRowElement>
  headerAttributes?: (column: {
    id: string
  }) => Record<string, string | number | undefined>
  cellAttributes?: (cell: {
    id: string
    column: { id: string }
    row: { original: TData }
  }) => Record<string, string | number | undefined>
  tableAttributes?: HTMLAttributes<HTMLTableElement> & {
    [key: `data-${string}`]: string | number | boolean | undefined
  }
  /**
   * The card of one row on a narrow screen. The list keeps one row model:
   * the card and the table row render the same record.
   */
  renderCard?: (row: TData) => ReactNode
}

/**
 * The shared renderer of a business list.
 *
 * The table owns the row model and the column model only. Pagination,
 * sorting, filtering and every row action stay with the domain that owns the
 * resource, so the same renderer serves a server-paginated list and a list
 * that is filtered in the browser.
 */
export function DataTable<TData extends RowData>({
  columns,
  data,
  getRowId,
  label,
  loading = false,
  busy = false,
  emptyLabel,
  className,
  rowAttributes,
  getRowProps,
  headerAttributes,
  cellAttributes,
  tableAttributes,
  renderCard,
}: DataTableProps<TData>) {
  const { t } = useLocale()
  const table = useTable({
    features: dataTableFeatures,
    columns,
    data,
    getRowId,
  })
  const rows = table.getRowModel().rows
  const emptyMessage = loading ? t('loading') : (emptyLabel ?? t('noData'))

  const tableElement = (
    <Table
      aria-busy={busy}
      aria-label={label}
      className={className}
      {...tableAttributes}
    >
      <TableHeader>
        {table.getHeaderGroups().map((headerGroup) => (
          <TableRow key={headerGroup.id}>
            {headerGroup.headers.map((header) => (
              <TableHead key={header.id} {...headerAttributes?.(header.column)}>
                {header.isPlaceholder ? null : (
                  <table.FlexRender header={header} />
                )}
              </TableHead>
            ))}
          </TableRow>
        ))}
      </TableHeader>
      <TableBody>
        {rows.length === 0 ? (
          <TableRow>
            <TableCell
              className="text-muted-foreground"
              colSpan={Math.max(columns.length, 1)}
            >
              {emptyMessage}
            </TableCell>
          </TableRow>
        ) : (
          rows.map((row) => (
            <TableRow
              key={row.id}
              {...rowAttributes?.(row.original)}
              {...getRowProps?.(row.original)}
            >
              {row.getAllCells().map((cell) => (
                <TableCell
                  key={cell.id}
                  {...cellAttributes?.({
                    id: cell.id,
                    column: cell.column,
                    row: cell.row,
                  })}
                >
                  <table.FlexRender cell={cell} />
                </TableCell>
              ))}
            </TableRow>
          ))
        )}
      </TableBody>
    </Table>
  )

  if (!renderCard) return tableElement

  return (
    <>
      <div className="grid gap-3 md:hidden">
        {rows.length === 0 ? (
          <div
            className="rounded-xl border bg-card px-4 py-6 text-sm text-muted-foreground"
            role={loading ? 'status' : undefined}
          >
            {emptyMessage}
          </div>
        ) : (
          rows.map((row) => <div key={row.id}>{renderCard(row.original)}</div>)
        )}
      </div>
      <div className="hidden min-w-0 md:block">{tableElement}</div>
    </>
  )
}
