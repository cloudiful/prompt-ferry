import { act, renderHook } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'
import { getPendingActions, runAsyncAction } from '@/lib/async-action'
import { useAsyncAction } from '@/lib/use-async-action'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}

describe('keyed async actions', () => {
  it('claims synchronously and holds the key through the required refresh', async () => {
    const mutation = deferred<void>()
    const refresh = deferred<void>()
    let refreshStarted!: () => void
    const refreshStartedPromise = new Promise<void>((resolve) => {
      refreshStarted = resolve
    })
    const duplicateAction = vi.fn()

    const first = runAsyncAction(
      'clientkey:one',
      async () => {
        await mutation.promise
        refreshStarted()
        await refresh.promise
        return 'refreshed'
      },
      'save',
    )

    expect(getPendingActions()['clientkey:one']).toBe('save')
    const duplicate = await runAsyncAction(
      'clientkey:one',
      duplicateAction,
      'delete',
    )
    expect(duplicate).toEqual({ executed: false })
    expect(duplicateAction).not.toHaveBeenCalled()

    mutation.resolve()
    await refreshStartedPromise
    expect(getPendingActions()['clientkey:one']).toBe('save')

    refresh.resolve()
    await expect(first).resolves.toEqual({
      executed: true,
      value: 'refreshed',
    })
    expect(getPendingActions()['clientkey:one']).toBeUndefined()
  })

  it('releases the key after failure', async () => {
    await expect(
      runAsyncAction(
        'session:authentication',
        async () => {
          throw new Error('login failed')
        },
        'login',
      ),
    ).rejects.toThrow('login failed')

    expect(getPendingActions()['session:authentication']).toBeUndefined()
  })

  it('notifies React subscribers and only runs effects for executed outcomes', async () => {
    const { result } = renderHook(() => useAsyncAction())
    const gate = deferred<void>()
    const effect = vi.fn()
    let pendingAction!: Promise<
      Awaited<ReturnType<typeof runAsyncAction<void>>>
    >

    act(() => {
      pendingAction = runAsyncAction(
        'host:local',
        () => gate.promise,
        'restart',
      )
    })
    expect(result.current.isPending('host:local')).toBe(true)
    expect(result.current.pendingOrigin('host:local')).toBe('restart')

    const duplicate = await runAsyncAction(
      'host:local',
      () => undefined,
      'role',
    )
    if (duplicate.executed) effect()
    expect(effect).not.toHaveBeenCalled()

    await act(async () => {
      gate.resolve()
      await pendingAction
    })
    if ((await pendingAction).executed) effect()
    expect(effect).toHaveBeenCalledTimes(1)
    expect(result.current.isPending('host:local')).toBe(false)
  })

  it('runs different keys together', async () => {
    const gate = deferred<string>()
    const held = runAsyncAction('endpoint:a', () => gate.promise, 'save')

    const other = await runAsyncAction(
      'endpoint:b',
      async () => 'other',
      'probe',
    )

    expect(other).toEqual({ executed: true, value: 'other' })
    expect(getPendingActions()['endpoint:a']).toBe('save')
    expect(getPendingActions()['endpoint:b']).toBeUndefined()

    gate.resolve('a')
    await expect(held).resolves.toEqual({ executed: true, value: 'a' })
  })

  it('leaves every other key busy when one action fails', async () => {
    const gate = deferred<string>()
    const held = runAsyncAction('endpoint:a', () => gate.promise, 'save')

    await expect(
      runAsyncAction(
        'endpoint:b',
        async () => {
          throw new Error('failed')
        },
        'save',
      ),
    ).rejects.toThrow('failed')

    expect(getPendingActions()['endpoint:a']).toBe('save')
    expect(getPendingActions()['endpoint:b']).toBeUndefined()

    gate.resolve('a')
    await held
  })

  it('replaces the published pending map instead of mutating it', async () => {
    const before = getPendingActions()
    const gate = deferred<string>()

    const running = runAsyncAction('endpoint:a', () => gate.promise, 'save')
    const during = getPendingActions()

    expect(during).not.toBe(before)
    expect(Object.keys(before)).toEqual([])
    expect(during['endpoint:a']).toBe('save')
    expect(Object.isFrozen(during)).toBe(true)

    gate.resolve('ok')
    await running

    const after = getPendingActions()
    expect(after).not.toBe(during)
    expect(during['endpoint:a']).toBe('save')
    expect(Object.keys(after)).toEqual([])
  })

  it('removes a settled key from the published map immediately', async () => {
    await expect(
      runAsyncAction('endpoint:a', async () => 'ok', 'save'),
    ).resolves.toEqual({ executed: true, value: 'ok' })
    expect(getPendingActions()['endpoint:a']).toBeUndefined()

    await expect(
      runAsyncAction(
        'endpoint:b',
        async () => {
          throw new Error('failed')
        },
        'save',
      ),
    ).rejects.toThrow('failed')
    expect(getPendingActions()['endpoint:b']).toBeUndefined()
    expect(Object.keys(getPendingActions())).toEqual([])
  })

  it('reuses a key after success, failure and refusal without a false origin', async () => {
    const gate = deferred<string>()
    const running = runAsyncAction('endpoint:a', () => gate.promise, 'delete')

    const refused = await runAsyncAction(
      'endpoint:a',
      async () => 'nope',
      'save',
    )
    expect(refused).toEqual({ executed: false })
    expect(getPendingActions()['endpoint:a']).toBe('delete')

    gate.resolve('done')
    await expect(running).resolves.toEqual({ executed: true, value: 'done' })
    expect(getPendingActions()['endpoint:a']).toBeUndefined()

    await expect(
      runAsyncAction('endpoint:a', async () => 'after success', 'save'),
    ).resolves.toEqual({ executed: true, value: 'after success' })
    expect(getPendingActions()['endpoint:a']).toBeUndefined()

    await expect(
      runAsyncAction(
        'endpoint:a',
        async () => {
          throw new Error('boom')
        },
        'save',
      ),
    ).rejects.toThrow('boom')
    expect(getPendingActions()['endpoint:a']).toBeUndefined()

    await expect(
      runAsyncAction('endpoint:a', async () => 'after failure', 'save'),
    ).resolves.toEqual({ executed: true, value: 'after failure' })
    expect(getPendingActions()['endpoint:a']).toBeUndefined()
  })
})
