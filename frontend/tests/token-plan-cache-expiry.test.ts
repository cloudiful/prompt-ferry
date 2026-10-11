import { expect, test } from 'bun:test'
import { createTokenPlanCacheExpiry } from '../src/composables/tokenPlanCacheExpiry'

type ScheduledTimer = {
  handle: number
  deadline: number
  callback: () => void
}

function fakeClock() {
  let currentTime = 0
  let nextHandle = 0
  const pending = new Map<number, ScheduledTimer>()
  const callbacks = new Map<number, () => void>()
  const setTimeout = (
    callback: () => void,
    delay: number,
  ): ReturnType<typeof globalThis.setTimeout> => {
    const handle = ++nextHandle
    const timer = { handle, deadline: currentTime + delay, callback }
    pending.set(handle, timer)
    callbacks.set(handle, callback)
    return handle as unknown as ReturnType<typeof globalThis.setTimeout>
  }
  const clearTimeout = (handle: ReturnType<typeof globalThis.setTimeout>) => {
    pending.delete(handle as unknown as number)
  }

  return {
    now: () => currentTime,
    setTimeout,
    clearTimeout,
    pending: () => [...pending.values()],
    invoke(handle: number) {
      callbacks.get(handle)?.()
    },
    advance(milliseconds: number) {
      const target = currentTime + milliseconds
      while (true) {
        const next = [...pending.values()]
          .filter((timer) => timer.deadline <= target)
          .sort((left, right) => left.deadline - right.deadline)[0]
        if (!next) break
        pending.delete(next.handle)
        currentTime = next.deadline
        next.callback()
      }
      currentTime = target
    },
  }
}

test('one nearest-deadline timer invalidates each expired entry once', () => {
  const clock = fakeClock()
  const expiry = createTokenPlanCacheExpiry({
    now: clock.now,
    setTimeout: clock.setTimeout,
    clearTimeout: clock.clearTimeout,
    canSchedule: () => true,
  })

  expiry.schedule('later', 3_000)
  const oldTimer = clock.pending()[0]
  expiry.schedule('earlier', 1_000)
  expect(clock.pending()).toHaveLength(1)

  clock.invoke(oldTimer!.handle)
  expect(expiry.readRevision()).toBe(0)
  expect(clock.pending()).toHaveLength(1)

  clock.advance(999)
  expect(expiry.readRevision()).toBe(0)
  clock.advance(1)
  expect(expiry.readRevision()).toBe(1)
  expect(clock.pending()).toHaveLength(1)

  clock.advance(1_999)
  expect(expiry.readRevision()).toBe(1)
  clock.advance(1)
  expect(expiry.readRevision()).toBe(2)
  expect(clock.pending()).toHaveLength(0)
})

test('removing or clearing cache entries cancels their expiry timer', () => {
  const clock = fakeClock()
  const expiry = createTokenPlanCacheExpiry({
    now: clock.now,
    setTimeout: clock.setTimeout,
    clearTimeout: clock.clearTimeout,
    canSchedule: () => true,
  })

  expiry.schedule('remove', 1_000)
  const canceledTimer = clock.pending()[0]
  expiry.schedule('keep', 2_000)
  expiry.remove('remove')
  expect(clock.pending()).toHaveLength(1)

  clock.invoke(canceledTimer!.handle)
  expect(expiry.readRevision()).toBe(0)
  expiry.clear()
  expect(clock.pending()).toHaveLength(0)
  clock.advance(3_000)
  expect(expiry.readRevision()).toBe(0)
})

test('nonbrowser ownership does not create autonomous timers', () => {
  const clock = fakeClock()
  const expiry = createTokenPlanCacheExpiry({
    now: clock.now,
    setTimeout: clock.setTimeout,
    clearTimeout: clock.clearTimeout,
    canSchedule: () => false,
  })

  expiry.schedule('server-render', 1_000)

  expect(clock.pending()).toHaveLength(0)
  clock.advance(2_000)
  expect(expiry.readRevision()).toBe(0)
})
