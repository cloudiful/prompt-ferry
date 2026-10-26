export type AsyncActionOutcome<T> =
  { executed: true; value: T } | { executed: false; value?: never }

export type PendingActions = Readonly<Record<string, string>>

type Listener = () => void

let pendingActions: PendingActions = Object.freeze({})
const listeners = new Set<Listener>()

export function subscribeToAsyncActions(listener: Listener): () => void {
  listeners.add(listener)
  return () => listeners.delete(listener)
}

export function getPendingActions(): PendingActions {
  return pendingActions
}

function publish(next: PendingActions): void {
  pendingActions = Object.freeze(next)
  for (const listener of listeners) listener()
}

export async function runAsyncAction<T>(
  key: string,
  action: () => T | Promise<T>,
  origin: string,
): Promise<AsyncActionOutcome<T>> {
  if (Object.hasOwn(pendingActions, key)) return { executed: false }

  publish({ ...pendingActions, [key]: origin })
  try {
    return { executed: true, value: await action() }
  } finally {
    const { [key]: _origin, ...remaining } = pendingActions
    publish(remaining)
  }
}
