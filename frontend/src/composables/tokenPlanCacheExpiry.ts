import { ref } from 'vue'

type TimerHandle = ReturnType<typeof setTimeout>

type TokenPlanCacheExpiryOptions = {
  now?: () => number
  setTimeout?: (callback: () => void, delay: number) => TimerHandle
  clearTimeout?: (handle: TimerHandle) => void
  canSchedule?: () => boolean
}

export function createTokenPlanCacheExpiry(
  options: TokenPlanCacheExpiryOptions = {},
) {
  const now = options.now ?? (() => Date.now())
  const setTimer =
    options.setTimeout ??
    ((callback: () => void, delay: number) =>
      globalThis.setTimeout(callback, delay))
  const clearTimer =
    options.clearTimeout ??
    ((handle: TimerHandle) => globalThis.clearTimeout(handle))
  const canSchedule =
    options.canSchedule ?? (() => typeof window !== 'undefined')
  const revision = ref(0)
  const deadlines = new Map<string, number>()
  let timer: TimerHandle | undefined
  let generation = 0

  function cancelTimer() {
    generation += 1
    if (timer !== undefined) clearTimer(timer)
    timer = undefined
  }

  function scheduleNext() {
    cancelTimer()
    if (!canSchedule() || deadlines.size === 0) return

    let deadline = Number.POSITIVE_INFINITY
    for (const value of deadlines.values()) {
      deadline = Math.min(deadline, value)
    }

    const scheduledGeneration = generation
    const scheduledTimer = setTimer(
      () => {
        if (scheduledGeneration !== generation) return
        timer = undefined
        const currentTime = now()
        let expired = false
        for (const [key, expiresAt] of deadlines) {
          if (expiresAt <= currentTime) {
            deadlines.delete(key)
            expired = true
          }
        }
        if (expired) revision.value += 1
        scheduleNext()
      },
      Math.max(0, deadline - now()),
    )
    timer = scheduledTimer
    const timerWithUnref = scheduledTimer as TimerHandle & {
      unref?: () => void
    }
    timerWithUnref.unref?.()
  }

  return {
    schedule(key: string, expiresAt: number) {
      deadlines.set(key, expiresAt)
      scheduleNext()
    },
    remove(key: string) {
      if (deadlines.delete(key)) scheduleNext()
    },
    clear() {
      deadlines.clear()
      cancelTimer()
    },
    readRevision() {
      return revision.value
    },
  }
}
