import type { QueryClient } from '@tanstack/react-query'
import type { PendingActions } from './async-action'

/**
 * Read one settings resource with the shared refresh contract: cancel the
 * in-flight read of this page, drop every cached copy in the domain namespace,
 * then fetch the resource uncached.
 */
export async function fetchFreshSetting<T>(
  queryClient: QueryClient,
  options: {
    queryKey: readonly unknown[]
    namespace: readonly unknown[]
    load: () => Promise<T>
  },
): Promise<T> {
  await queryClient.cancelQueries({
    queryKey: options.queryKey,
    exact: true,
  })
  await queryClient.invalidateQueries({
    queryKey: options.namespace,
    refetchType: 'none',
  })
  return queryClient.fetchQuery({
    queryKey: options.queryKey,
    queryFn: options.load,
    staleTime: 0,
  })
}

export type SettingsRefreshGate = {
  pendingActions: PendingActions
  dirty: boolean
  failed: boolean
  loading: boolean
  loaded: boolean
}

export function settingsRefreshGateBlocked(
  input: SettingsRefreshGate,
  key: string,
): boolean {
  return (
    !input.loaded ||
    input.loading ||
    input.dirty ||
    input.failed ||
    Object.hasOwn(input.pendingActions, key)
  )
}

/** A capability answer's own code and message, without any request payload. */
export function formatEnvelopeCause(cause: unknown): string | null {
  if (cause && typeof cause === 'object' && 'error' in cause) {
    const envelope = (cause as { error?: unknown }).error
    if (envelope && typeof envelope === 'object') {
      const code = (envelope as { code?: unknown }).code
      const message = (envelope as { message?: unknown }).message
      if (typeof code === 'string' && typeof message === 'string') {
        return `${code}: ${message}`
      }
      if (typeof message === 'string') return message
    }
  }
  return null
}
