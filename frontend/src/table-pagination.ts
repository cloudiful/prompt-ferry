import { ref, watch, type Ref } from 'vue'
import { readStorage, writeStorage } from './storage'

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

export function pageChange(page: number, rows: number): TablePageChange {
  return {
    first: Math.max(0, page - 1) * rows,
    page: Math.max(0, page - 1),
    rows,
  }
}

const PAGE_SIZE_STORAGE_PREFIX = 'prompt-ferry:table-page-size:'

function normalizePageSize(
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

export function useStoredPageSize(
  scope: string,
  fallback: number,
  allowed: readonly number[],
): Ref<number> {
  const storageKey = `${PAGE_SIZE_STORAGE_PREFIX}${scope}`
  const pageSize = ref(
    normalizePageSize(readStorage(storageKey), fallback, allowed),
  )

  watch(
    pageSize,
    (value) => {
      const normalized = normalizePageSize(value, fallback, allowed)
      if (pageSize.value !== normalized) {
        pageSize.value = normalized
        return
      }
      writeStorage(storageKey, String(normalized))
    },
    { immediate: true },
  )

  return pageSize
}
