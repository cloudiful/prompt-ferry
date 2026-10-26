import { useEffect, useState } from 'react'
import {
  ChevronLeft,
  ChevronRight,
  ChevronsLeft,
  ChevronsRight,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useLocale } from '@/lib/i18n'
import { readStorage, writeStorage } from '@/storage'
import {
  normalizePageSize,
  pageChange,
  pageSizeStorageKey,
  STANDARD_PAGE_SIZE_OPTIONS,
  type TablePageChange,
} from '@/models/pagination'

export function useStoredPageSize(
  scope: string,
  fallback = 10,
  allowed: readonly number[] = STANDARD_PAGE_SIZE_OPTIONS,
): [number, (value: number) => void] {
  const storageKey = pageSizeStorageKey(scope)
  const [rows, setRows] = useState(() =>
    normalizePageSize(readStorage(storageKey), fallback, allowed),
  )

  useEffect(() => {
    writeStorage(storageKey, String(rows))
  }, [rows, storageKey])

  function setNormalizedRows(value: number): void {
    setRows(normalizePageSize(value, fallback, allowed))
  }

  return [rows, setNormalizedRows]
}

type TablePaginationProps = {
  first: number
  rows: number
  total: number
  pageSizeOptions: readonly number[]
  onChange: (change: TablePageChange) => void
}

export function TablePagination({
  first,
  rows,
  total,
  pageSizeOptions,
  onChange,
}: TablePaginationProps) {
  const { t } = useLocale()
  const pageCount = Math.max(1, Math.ceil(total / rows))
  const currentPage = Math.min(pageCount, Math.floor(first / rows) + 1)
  const firstVisiblePage = Math.max(1, Math.min(currentPage - 2, pageCount - 4))
  const lastVisiblePage = Math.min(pageCount, firstVisiblePage + 4)
  const pageNumbers = Array.from(
    { length: lastVisiblePage - firstVisiblePage + 1 },
    (_, index) => firstVisiblePage + index,
  )

  function pageLabel(page: number, current: boolean): string {
    return t(current ? 'paginationPageCurrent' : 'paginationPage', { page })
  }

  function changePage(page: number): void {
    onChange(pageChange(page, rows))
  }

  function changeRows(value: string): void {
    const nextRows = Number(value)
    if (Number.isFinite(nextRows)) onChange(pageChange(1, nextRows))
  }

  return (
    <div className="flex flex-wrap items-center justify-between gap-3 border-t px-3 py-3">
      <Select onValueChange={changeRows} value={String(rows)}>
        <SelectTrigger aria-label={t('paginationPageSize')} className="w-24">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {pageSizeOptions.map((option) => (
            <SelectItem key={option} value={String(option)}>
              {option}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <nav
        aria-label={pageLabel(currentPage, true)}
        className="flex flex-wrap items-center gap-1"
      >
        <Button
          aria-label={t('paginationFirst')}
          disabled={currentPage <= 1 || total === 0}
          onClick={() => changePage(1)}
          size="icon-sm"
          type="button"
          variant="outline"
        >
          <ChevronsLeft aria-hidden="true" />
        </Button>
        <Button
          aria-label={t('paginationPrevious')}
          disabled={currentPage <= 1 || total === 0}
          onClick={() => changePage(currentPage - 1)}
          size="icon-sm"
          type="button"
          variant="outline"
        >
          <ChevronLeft aria-hidden="true" />
        </Button>
        {pageNumbers.map((page) => (
          <Button
            aria-current={page === currentPage ? 'page' : undefined}
            aria-label={pageLabel(page, page === currentPage)}
            key={page}
            onClick={() => changePage(page)}
            size="icon-sm"
            type="button"
            variant={page === currentPage ? 'secondary' : 'outline'}
          >
            {page}
          </Button>
        ))}
        <Button
          aria-label={t('paginationNext')}
          disabled={currentPage >= pageCount || total === 0}
          onClick={() => changePage(currentPage + 1)}
          size="icon-sm"
          type="button"
          variant="outline"
        >
          <ChevronRight aria-hidden="true" />
        </Button>
        <Button
          aria-label={t('paginationLast')}
          disabled={currentPage >= pageCount || total === 0}
          onClick={() => changePage(pageCount)}
          size="icon-sm"
          type="button"
          variant="outline"
        >
          <ChevronsRight aria-hidden="true" />
        </Button>
      </nav>
    </div>
  )
}
