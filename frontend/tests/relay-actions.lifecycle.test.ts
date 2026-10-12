import { expect, mock, test } from 'bun:test'
import {
  createEmptyRelayForm,
  createRelayPatchRequest,
  createRelayRequest,
  relayToForm,
  type RelayForm,
} from '../src/admin-mappers/forms/relay'
import { loadActionFactory } from './relay-actions.fixtures'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

function relayListResponse() {
  return {
    data: {
      relays: [],
      total: 0,
      first: 0,
      rows: 10,
      connected_count: 0,
      enabled_count: 0,
    },
  }
}

const actualAdminApi = await import('../src/generated/admin-api')
const refreshStarted = deferred<void>()
const refreshResult = deferred<ReturnType<typeof relayListResponse>>()
const listRelays = mock(async () => {
  refreshStarted.resolve()
  return refreshResult.promise
})
const updateRelay = mock(async () => ({ data: { relay_id: 'relay-a' } }))
const deleteRelay = mock(async () => ({ data: undefined }))
const reconnectRelay = mock(async () => ({ data: { relay_id: 'relay-a' } }))
mock.module('../src/generated/admin-api', () => ({
  ...actualAdminApi,
  deleteRelay,
  listRelays,
  reconnectRelay,
  updateRelay,
}))

const { createPinia, setActivePinia } = await import('pinia')
const { useRelaysStore } = await import('../src/stores/relays')
const { computed: vueComputed, ref: vueRef } = await import('vue')
const { CREATE_KEY } = await import('../src/stores/record-action-types')
const errors: unknown[] = []
const successes: string[] = []
const createPageActions = await loadActionFactory<
  (options: { relayStore: unknown; t: (key: string) => string }) => {
    dialogForm: { value: RelayForm }
    dialogVisible: { value: boolean }
    dialogPending: { value: boolean }
    dialogSavePending: { value: boolean }
    openCreateDialog: () => void
    saveRelay: () => Promise<void>
  }
>('../src/composables/useRelaysPageActions.ts', 'useRelaysPageActions', {
  computed: vueComputed,
  ref: vueRef,
  createEmptyRelayForm,
  createRelayPatchRequest,
  createRelayRequest,
  relayToForm,
  useNotifier: () => ({
    notifyApiError: (cause: unknown) => errors.push(cause),
    notifySuccess: (message: string) => successes.push(message),
  }),
  CREATE_KEY,
})

test('relay save feedback spans HTTP and refresh, gates one row, and releases', async () => {
  setActivePinia(createPinia())
  errors.length = 0
  successes.length = 0
  listRelays.mockClear()
  updateRelay.mockClear()
  deleteRelay.mockClear()
  reconnectRelay.mockClear()

  const store = useRelaysStore()
  const page = createPageActions({ relayStore: store, t: (key) => key })
  page.dialogForm.value = { ...createEmptyRelayForm(), relay_id: 'relay-a' }
  page.dialogForm.value.name = 'relay A draft'
  page.dialogVisible.value = true

  const saving = page.saveRelay()
  expect(store.relayPendingActions['relay-a']).toBe('save')
  expect(page.dialogPending.value).toBe(true)
  expect(page.dialogSavePending.value).toBe(true)
  await refreshStarted.promise

  expect(updateRelay).toHaveBeenCalledTimes(1)
  expect(await store.removeRelay('relay-a')).toEqual({ executed: false })
  expect(await store.reconnectRelay('relay-a')).toEqual({ executed: false })
  const otherRelaySave = store.saveRelay('relay-b', { enabled: false })
  expect(updateRelay).toHaveBeenCalledTimes(2)
  expect(store.relayPendingActions['relay-b']).toBe('save')

  refreshResult.resolve(relayListResponse())
  await Promise.all([saving, otherRelaySave])

  expect(store.relayPendingActions['relay-a']).toBeUndefined()
  expect(store.relayPendingActions['relay-b']).toBeUndefined()
  expect(page.dialogPending.value).toBe(false)
  expect(page.dialogVisible.value).toBe(false)
  expect(successes).toEqual(['saved'])
  expect(errors).toEqual([])
  expect(deleteRelay).not.toHaveBeenCalled()
  expect(reconnectRelay).not.toHaveBeenCalled()
})

test('relay save failure retains the draft for a successful retry', async () => {
  setActivePinia(createPinia())
  errors.length = 0
  successes.length = 0
  listRelays.mockImplementation(async () => relayListResponse())
  updateRelay.mockReset()
  updateRelay.mockRejectedValueOnce(new Error('relay write failed'))
  updateRelay.mockImplementation(async () => ({
    data: { relay_id: 'relay-a' },
  }))

  const store = useRelaysStore()
  const page = createPageActions({ relayStore: store, t: (key) => key })
  page.dialogForm.value = { ...createEmptyRelayForm(), relay_id: 'relay-a' }
  page.dialogForm.value.name = 'retryable relay draft'
  page.dialogVisible.value = true

  await page.saveRelay()

  expect(errors).toHaveLength(1)
  expect(page.dialogVisible.value).toBe(true)
  expect(page.dialogForm.value.name).toBe('retryable relay draft')
  expect(store.relayPendingActions['relay-a']).toBeUndefined()

  await page.saveRelay()

  expect(updateRelay).toHaveBeenCalledTimes(2)
  expect(page.dialogVisible.value).toBe(false)
  expect(successes).toEqual(['saved'])
})
