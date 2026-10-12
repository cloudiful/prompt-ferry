import { expect, mock, test } from 'bun:test'
import { computed, ref } from 'vue'
import {
  createEmptyRelayHostForm,
  relayHostToForm,
} from '../src/admin-mappers/forms/relay-host'
import type { HostRole } from '../src/generated/admin-api'
import { loadActionFactory } from './relay-actions.fixtures'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

const actualAdminApi = await import('../src/generated/admin-api')
const roleResponse = deferred<{
  data: { role: HostRole; pending_role: HostRole; restart_required: boolean }
}>()
const restartResponse = deferred<{ data: undefined }>()
const relaySetHostRole = mock(() => roleResponse.promise)
const relaySetSettings = mock(async () => ({ data: undefined }))
const relayRequestRestart = mock(() => restartResponse.promise)
mock.module('../src/generated/admin-api', () => ({
  ...actualAdminApi,
  relayRequestRestart,
  relaySetHostRole,
  relaySetSettings,
}))

const { createPinia, setActivePinia } = await import('pinia')
const { useRelayHostStore } = await import('../src/stores/relay-host')
const errors: unknown[] = []
const successes: string[] = []
const createPageActions = await loadActionFactory<
  (options: {
    form: ReturnType<typeof ref>
    hostStore: unknown
    relaySession: unknown
    t: (key: string) => string
    tokenInput: ReturnType<typeof ref>
  }) => {
    bannerRestartPending: { value: boolean }
    hostPendingAction: { value: string | undefined }
    hostWritePending: { value: boolean }
    requestRestart: (origin: 'banner' | 'settings') => Promise<void>
    roleActionTarget: { value: HostRole | null }
    selectRole: (role: HostRole) => Promise<void>
    settingsRestartPending: { value: boolean }
  }
>('../src/composables/useRelayHostPageActions.ts', 'useRelayHostPageActions', {
  computed,
  ref,
  relayHostToForm,
  useNotifier: () => ({
    notifyApiError: (cause: unknown) => errors.push(cause),
    notifySuccess: (message: string) => successes.push(message),
  }),
})

test('role save preserves a newer bind edit and excludes other host writes', async () => {
  setActivePinia(createPinia())
  errors.length = 0
  successes.length = 0
  relaySetHostRole.mockClear()
  relaySetSettings.mockClear()
  relayRequestRestart.mockClear()

  const store = useRelayHostStore()
  const form = ref({
    ...createEmptyRelayHostForm(),
    admin_bind: '127.0.0.1:8790',
  })
  const actions = createPageActions({
    form,
    hostStore: store,
    relaySession: {},
    t: (key) => key,
    tokenInput: ref(''),
  })

  const saving = actions.selectRole('worker')
  expect(store.relayHostPendingActions.local).toBe('role')
  expect(actions.hostPendingAction.value).toBe('role')
  expect(actions.hostWritePending.value).toBe(true)
  expect(actions.roleActionTarget.value).toBe('worker')
  expect(await store.saveSettings('127.0.0.1:8801')).toEqual({
    executed: false,
  })
  expect(await store.requestRestart()).toEqual({ executed: false })
  expect(relaySetSettings).not.toHaveBeenCalled()
  expect(relayRequestRestart).not.toHaveBeenCalled()

  form.value.admin_bind = '127.0.0.1:8800'
  roleResponse.resolve({
    data: { role: 'worker', pending_role: 'worker', restart_required: true },
  })
  await saving

  expect(form.value.role).toBe('worker')
  expect(form.value.admin_bind).toBe('127.0.0.1:8800')
  expect(store.relayHostPendingActions.local).toBeUndefined()
  expect(actions.hostWritePending.value).toBe(false)
  expect(actions.roleActionTarget.value).toBeNull()
  expect(relaySetHostRole).toHaveBeenCalledTimes(1)
  expect(successes).toEqual(['relayRoleSaved'])
  expect(errors).toEqual([])
})

test('restart feedback spins only on the host control that started it', async () => {
  setActivePinia(createPinia())
  errors.length = 0
  successes.length = 0
  relayRequestRestart.mockClear()

  const store = useRelayHostStore()
  const actions = createPageActions({
    form: ref(createEmptyRelayHostForm()),
    hostStore: store,
    relaySession: {},
    t: (key) => key,
    tokenInput: ref(''),
  })

  const restarting = actions.requestRestart('banner')
  expect(store.relayHostPendingActions.local).toBe('restart')
  expect(actions.bannerRestartPending.value).toBe(true)
  expect(actions.settingsRestartPending.value).toBe(false)
  await actions.requestRestart('settings')
  expect(relayRequestRestart).toHaveBeenCalledTimes(1)

  restartResponse.resolve({ data: undefined })
  await restarting

  expect(actions.bannerRestartPending.value).toBe(false)
  expect(actions.settingsRestartPending.value).toBe(false)
  expect(store.relayHostPendingActions.local).toBeUndefined()
  expect(successes).toEqual(['relayRestartRequested'])
  expect(errors).toEqual([])
})
