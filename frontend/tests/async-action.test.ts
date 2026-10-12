import { expect, test } from 'bun:test'

import { useAsyncAction } from '../src/composables/useAsyncAction'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (cause: unknown) => void
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle
    reject = fail
  })
  return { promise, resolve, reject }
}

test('a first call runs and reports its value', async () => {
  const guard = useAsyncAction()

  const result = await guard.run('endpoint:a', async () => 'saved')

  expect(result).toEqual({ executed: true, value: 'saved' })
  expect(guard.isPending('endpoint:a')).toBe(false)
})

test('the key is held for the whole action, not just its request', async () => {
  const guard = useAsyncAction()
  const gate = deferred<string>()

  const running = guard.run('endpoint:a', () => gate.promise, 'save')
  expect(guard.isPending('endpoint:a')).toBe(true)
  expect(guard.pendingAction('endpoint:a')).toBe('save')

  gate.resolve('saved')
  await running

  expect(guard.isPending('endpoint:a')).toBe(false)
})

test('a second call on a held key is refused and never runs', async () => {
  const guard = useAsyncAction()
  const gate = deferred<string>()
  let runs = 0
  const action = () => {
    runs += 1
    return gate.promise
  }

  const first = guard.run('endpoint:a', action, 'save')
  const second = await guard.run('endpoint:a', action, 'save')

  expect(second).toEqual({ executed: false })
  expect(runs).toBe(1)

  gate.resolve('saved')
  await expect(first).resolves.toEqual({ executed: true, value: 'saved' })
})

test('two triggers in the same tick reach only one request', async () => {
  // A held Enter and a click arrive without an await between them, so the claim
  // has to happen before the first suspension point.
  const guard = useAsyncAction()
  let runs = 0
  const action = async () => {
    runs += 1
    await Promise.resolve()
    return runs
  }

  const first = guard.run('endpoint:a', action)
  const second = await guard.run('endpoint:a', action)

  expect(second).toEqual({ executed: false })
  expect(await first).toEqual({ executed: true, value: 1 })
  expect(runs).toBe(1)
})

test('different keys run together', async () => {
  const guard = useAsyncAction()
  const gate = deferred<string>()

  const held = guard.run('endpoint:a', () => gate.promise)
  const other = await guard.run('endpoint:b', async () => 'other')

  expect(other).toEqual({ executed: true, value: 'other' })
  expect(guard.isPending('endpoint:a')).toBe(true)
  expect(guard.isPending('endpoint:b')).toBe(false)

  gate.resolve('a')
  await held
})

test('a rejection clears its own key and rethrows', async () => {
  const guard = useAsyncAction()
  const boom = new Error('server said no')

  await expect(
    guard.run('endpoint:a', async () => {
      throw boom
    }),
  ).rejects.toBe(boom)

  expect(guard.isPending('endpoint:a')).toBe(false)
  // A refused-after-failure retry still works, so the key really was released.
  expect(await guard.run('endpoint:a', async () => 'retried')).toEqual({
    executed: true,
    value: 'retried',
  })
})

test('a failure leaves every other key busy', async () => {
  const guard = useAsyncAction()
  const gate = deferred<string>()
  const held = guard.run('endpoint:a', () => gate.promise, 'save')

  await expect(
    guard.run('endpoint:b', async () => {
      throw new Error('failed')
    }),
  ).rejects.toThrow('failed')

  expect(guard.isPending('endpoint:a')).toBe(true)
  expect(guard.isPending('endpoint:b')).toBe(false)

  gate.resolve('a')
  await held
})

test('a settled key never lingers in the published map', async () => {
  const guard = useAsyncAction()
  await guard.run('endpoint:a', async () => 'ok', 'save')
  await guard
    .run('endpoint:b', async () => {
      throw new Error('failed')
    })
    .catch(() => {})

  expect([...guard.pendingActions.value.keys()]).toEqual([])
  expect(guard.pendingAction('endpoint:a')).toBeUndefined()
})

test('the published map is replaced, never mutated in place', async () => {
  const guard = useAsyncAction()
  const before = guard.pendingActions.value
  const gate = deferred<string>()

  const running = guard.run('endpoint:a', () => gate.promise, 'save')
  const during = guard.pendingActions.value

  expect(during).not.toBe(before)
  expect(before.size).toBe(0)
  expect(during.has('endpoint:a')).toBe(true)

  gate.resolve('ok')
  await running

  expect(guard.pendingActions.value).not.toBe(during)
  expect(during.has('endpoint:a')).toBe(true)
})

test('a key is reusable once it settles, including after a refusal', async () => {
  const guard = useAsyncAction()
  const gate = deferred<string>()
  const held = guard.run('endpoint:a', () => gate.promise, 'delete')

  await expect(guard.run('endpoint:a', async () => 'nope')).resolves.toEqual({
    executed: false,
  })

  gate.resolve('done')
  await held

  expect(await guard.run('endpoint:a', async () => 'later')).toEqual({
    executed: true,
    value: 'later',
  })
})
