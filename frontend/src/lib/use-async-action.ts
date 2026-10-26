import { useSyncExternalStore } from 'react'
import {
  getPendingActions,
  runAsyncAction,
  subscribeToAsyncActions,
} from './async-action'

export function useAsyncAction() {
  const pendingActions = useSyncExternalStore(
    subscribeToAsyncActions,
    getPendingActions,
    getPendingActions,
  )

  return {
    pendingActions,
    isPending: (key: string) => Object.hasOwn(pendingActions, key),
    pendingOrigin: (key: string) => pendingActions[key] ?? null,
    run: runAsyncAction,
  }
}
