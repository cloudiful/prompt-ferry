import type { PendingActions } from './async-action'

export type SettingsRefreshGate = {
  pendingActions: PendingActions
  dirty: boolean
  failed: boolean
  loading: boolean
  loaded: boolean
}

/**
 * Whether a scoped settings refresh is safe: loaded, idle, clean and without an
 * unrecovered write failure. A refresh replaces the loaded value, so it must
 * not discard a draft the operator has not saved yet.
 */
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
