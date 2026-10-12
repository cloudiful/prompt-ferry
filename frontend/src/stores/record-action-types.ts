/**
 * Shared vocabulary for the record-scoped action modules.
 *
 * Every guarded domain in this workspace follows the same two rules: operations
 * that would write the same record share one key, and a record that is not yet
 * persisted is written under a `create` key of its own. Both live here so the
 * key grammar, the row projection, and the payload snapshot have one definition
 * instead of one copy per domain.
 *
 * The domains themselves stay independent: each factory owns its own keys,
 * slices, and reload, and none of them reaches into another's state.
 */

/** The key a not-yet-persisted record is written under. */
export const CREATE_KEY = 'create'

/**
 * Which action holds a key. Surfaces read this to spin only their own control:
 * a record whose delete is running disables its save without spinning it.
 */
export type PendingActionLabel =
  | 'save'
  | 'delete'
  | 'toggle'
  | 'test'
  | 'reset'
  | 'reconnect'
  | 'restart'
  | 'role'
  | 'settings'

/** Row ids of one domain that are gated, each mapped to its holding action. */
export type PendingRowActions = Readonly<Record<string, string>>

/**
 * A record key: one namespace, one record. A `null` id means the record does
 * not exist yet, which is a shared `create` slot rather than a per-record one.
 */
export function recordActionKey(prefix: string, id: string | null): string {
  return `${prefix}:${id ?? CREATE_KEY}`
}

/**
 * The per-domain row projection, as the surfaces receive it: presence is the
 * gate, the value is the spinner origin. Keys from other namespaces — a probe,
 * or a create — pass through untouched.
 */
export function pendingRowsById(
  pending: ReadonlyMap<string, string | undefined>,
  prefix: string,
): PendingRowActions {
  const head = `${prefix}:`
  const rows: Record<string, string> = {}
  for (const [key, label] of pending) {
    if (!key.startsWith(head)) continue
    rows[key.slice(head.length)] = label ?? ''
  }
  return rows
}

/**
 * A JSON round-trip is the faithful copy for these bodies: they are JSON on the
 * wire, and unlike `structuredClone` it also survives a Vue proxy handed in by
 * a caller. Callers capture before the first await, so an edit made while the
 * request is in flight cannot ride along with it.
 */
export function capturePayload<T>(body: T): T {
  return JSON.parse(JSON.stringify(body)) as T
}
