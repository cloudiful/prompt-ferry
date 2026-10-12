import { ref, type Ref } from 'vue'

/**
 * What a guarded call reports to its caller.
 *
 * `executed: false` means the guard refused the call because the same key was
 * already in flight — not that the work failed. A caller must therefore check
 * the flag before it reports success, closes a dialog, or opens a notification,
 * or a duplicate trigger will toast and close a second time.
 */
export type AsyncActionOutcome<T> =
  { executed: true; value: T } | { executed: false }

/** Keyed pending state: which keys are held, and which action holds each one. */
export type PendingActions = ReadonlyMap<string, string | undefined>

export type AsyncActionGuard = {
  isPending: (key: string) => boolean
  pendingAction: (key: string) => string | undefined
  pendingActions: Readonly<Ref<PendingActions>>
  run: <T>(
    key: string,
    action: () => Promise<T>,
    label?: string,
  ) => Promise<AsyncActionOutcome<T>>
}

/**
 * A keyed guard for user-triggered writes.
 *
 * Claiming a key is synchronous, so two triggers in the same tick — a held
 * Enter key and a click — cannot both reach the request. The key stays claimed
 * for the whole action, which is how a write keeps its spinner on through the
 * refresh that follows it, and is released in a `finally`, so a rejection
 * clears it just as a success does.
 *
 * Keys are the caller's unit of exclusion. Related operations that must not run
 * concurrently — a save and a delete of the same record — share one key;
 * operations that may run together use different ones.
 *
 * The published map is replaced on every change rather than mutated, so a
 * consumer holding the previous value never sees it change underneath, and the
 * pending state stays read-only from the outside.
 */
export function useAsyncAction(): AsyncActionGuard {
  const pending = ref<PendingActions>(new Map())

  function isPending(key: string): boolean {
    return pending.value.has(key)
  }

  function pendingAction(key: string): string | undefined {
    return pending.value.get(key)
  }

  async function run<T>(
    key: string,
    action: () => Promise<T>,
    label?: string,
  ): Promise<AsyncActionOutcome<T>> {
    if (isPending(key)) return { executed: false }

    const claimed = new Map(pending.value)
    claimed.set(key, label)
    pending.value = claimed

    try {
      return { executed: true, value: await action() }
    } finally {
      // Only this key is released: a rejection never clears another key's busy
      // state, and a settled key never lingers in the map.
      const settled = new Map(pending.value)
      settled.delete(key)
      pending.value = settled
    }
  }

  return {
    isPending,
    pendingAction,
    pendingActions: pending,
    run,
  }
}
