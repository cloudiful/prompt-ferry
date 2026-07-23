import type {
  TableFilterChange as AppTableFilterChange,
  TablePageChange as AppTablePageChange,
  TableSortChange as AppTableSortChange,
} from '@/table-pagination'

declare global {
  type TableFilterChange = AppTableFilterChange
  type TablePageChange = AppTablePageChange
  type TableSortChange = AppTableSortChange
}

export {}
