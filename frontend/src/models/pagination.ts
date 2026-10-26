export const REQUEST_RECORD_PAGE_SIZE_OPTIONS = [10, 25, 50, 100]
export const STANDARD_PAGE_SIZE_OPTIONS = [10, 20, 50]
export const APPROVAL_PAGE_SIZE_OPTIONS = [10, 25, 50]

export type TablePageChange = {
  first: number
  page: number
  rows: number
}

export type TableSortChange = {
  sortField?: string
  sortOrder?: -1 | 0 | 1 | null
}

export type TableFilterChange = Record<string, unknown>

const PAGE_SIZE_STORAGE_PREFIX = 'prompt-ferry:table-page-size:'

export function pageChange(page: number, rows: number): TablePageChange {
  const zeroBasedPage = Math.max(0, page - 1)
  return {
    first: zeroBasedPage * rows,
    page: zeroBasedPage,
    rows,
  }
}

export function normalizePageSize(
  value: number | string | null,
  fallback: number,
  allowed: readonly number[],
): number {
  const parsed =
    typeof value === 'number'
      ? value
      : typeof value === 'string'
        ? Number.parseInt(value, 10)
        : Number.NaN
  return allowed.includes(parsed) ? parsed : fallback
}

export function pageSizeStorageKey(scope: string): string {
  return `${PAGE_SIZE_STORAGE_PREFIX}${scope}`
}
