import type { QueryClient } from '@tanstack/react-query'

/**
 * Refresh one page of a paginated admin list, walking back to the last page
 * that still holds items when the requested offset is past the reported total.
 */
export async function refreshPagedQuery<
  TPage extends { rows: number; total: number },
  TItem,
>(
  queryClient: QueryClient,
  options: {
    queryKey: (first: number, rows: number) => readonly unknown[]
    namespaceKey: readonly unknown[]
    fetchPage: (first: number, rows: number) => Promise<TPage>
    items: (page: TPage) => readonly TItem[]
  },
  first: number,
  rows: number,
): Promise<TPage> {
  await queryClient.cancelQueries({
    queryKey: options.queryKey(first, rows),
    exact: true,
  })
  await queryClient.invalidateQueries({
    queryKey: options.namespaceKey,
    refetchType: 'none',
  })

  let requestedFirst = first
  while (true) {
    const page = await queryClient.fetchQuery({
      queryKey: options.queryKey(requestedFirst, rows),
      queryFn: () => options.fetchPage(requestedFirst, rows),
      staleTime: 0,
    })
    const pageRows = page.rows > 0 ? page.rows : rows
    if (
      pageRows > 0 &&
      options.items(page).length === 0 &&
      requestedFirst >= page.total
    ) {
      const correctedFirst =
        page.total > 0 ? Math.floor((page.total - 1) / pageRows) * pageRows : 0
      if (correctedFirst !== requestedFirst) {
        requestedFirst = correctedFirst
        continue
      }
    }
    return page
  }
}
