import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { RefreshCw } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { HeaderActions } from '@/components/layout/HeaderActions'
import {
  DataTable,
  type DataTableColumnDef,
} from '@/components/shared/DataTable'
import {
  TablePagination,
  useStoredPageSize,
} from '@/components/shared/TablePagination'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  type TablePageChange,
} from '@/models/pagination'
import { useLocale } from '@/lib/i18n'
import { useNotifier } from '@/lib/notifier'
import { useSessionStore } from '@/lib/session'
import { meListModelsOptions } from '@/generated/admin-api/@tanstack/react-query.gen'
import type { AvailableModelsResponse } from '@/generated/admin-api'

export function AvailableModelsPage() {
  const { t } = useLocale()
  const { notifyApiError } = useNotifier()
  const me = useSessionStore((state) => state.me)
  const [first, setFirst] = useState(0)
  const [rows, setRows] = useStoredPageSize('available-models')
  const notifiedErrors = useRef(new WeakSet<object>())

  const query = useQuery({
    ...meListModelsOptions({ query: { first, rows } }),
    enabled: me !== null,
  })

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
    if (page.models.length === 0 && page.first >= page.total) {
      const correctedFirst =
        page.total > 0
          ? Math.floor((page.total - 1) / page.rows) * page.rows
          : 0
      if (correctedFirst !== first) setFirst(correctedFirst)
      return
    }
    if (page.first !== first) setFirst(page.first)
    if (page.rows !== rows) setRows(page.rows)
  }, [first, query.data, rows, setRows])

  function changePage(change: TablePageChange): void {
    setFirst(change.first)
    setRows(change.rows)
  }

  function refresh(): Promise<unknown> {
    return query.refetch()
  }

  const models = query.data?.models ?? []

  const columns = useMemo<
    Array<DataTableColumnDef<AvailableModelsResponse['models'][number]>>
  >(
    () => [
      {
        id: 'id',
        header: t('id'),
        cell: ({ row }) => (
          <span
            className="block max-w-[20rem] truncate"
            title={row.original.id}
          >
            {row.original.id}
          </span>
        ),
      },
      {
        id: 'name',
        header: t('name'),
        cell: ({ row }) => (
          <span
            className="block max-w-[24rem] truncate"
            title={row.original.name}
          >
            {row.original.name}
          </span>
        ),
      },
    ],
    [t],
  )

  return (
    <section
      className="grid min-w-0 max-w-full gap-3"
      data-foundation-route-id="R-AVAILABLE-MODELS"
    >
      <HeaderActions>
        <Button
          aria-label={t('refresh')}
          onClick={() => void refresh()}
          size="sm"
          type="button"
          variant="outline"
        >
          {query.isFetching ? (
            <span className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent" />
          ) : (
            <RefreshCw aria-hidden="true" />
          )}
          <span>{t('refresh')}</span>
        </Button>
      </HeaderActions>
      <div className="min-w-0">
        <DataTable
          columns={columns}
          data={models}
          getRowId={(model) => model.id}
          label={t('availableModels')}
          loading={query.isPending}
        />
      </div>
      <TablePagination
        first={query.data?.first ?? first}
        onChange={changePage}
        pageSizeOptions={STANDARD_PAGE_SIZE_OPTIONS}
        rows={query.data?.rows ?? rows}
        total={query.data?.total ?? 0}
      />
    </section>
  )
}
