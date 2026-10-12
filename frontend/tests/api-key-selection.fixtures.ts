import { mock } from 'bun:test'

const actualAdminApi = await import('../src/generated/admin-api')

export const createClientKey = mock(async () => ({
  data: { key: { enabled: true, key_id: 'k7', label: 'theirs' }, secret: 's7' },
}))
export const deleteClientKey = mock(async () => ({ data: null }))
export const listClientKeys = mock(async () => ({
  data: { first: 0, keys: [], rows: 10, total: 0 },
}))
export const meCreateClientKey = mock(async () => ({
  data: { key: { enabled: true, key_id: 'k', label: 'x' }, secret: 's' },
}))
export const meDeleteClientKey = mock(async () => ({ data: null }))
export const meListClientKeys = mock(async () => ({
  data: { first: 0, keys: [], rows: 10, total: 0 },
}))
export const meUpdateClientKey = mock(async () => ({
  data: { enabled: true, key_id: 'k', label: 'renamed' },
}))

const allMocks = [
  createClientKey,
  deleteClientKey,
  listClientKeys,
  meCreateClientKey,
  meDeleteClientKey,
  meListClientKeys,
  meUpdateClientKey,
]

mock.module('../src/generated/admin-api', () => ({
  ...actualAdminApi,
  createClientKey,
  deleteClientKey,
  listClientKeys,
  meCreateClientKey,
  meDeleteClientKey,
  meListClientKeys,
  meUpdateClientKey,
}))

const { createPinia, setActivePinia } = await import('pinia')
const { SELF_USER_ID, useApiKeysStore } = await import('../src/stores/api-keys')

export { SELF_USER_ID }

export type KeyListResponse = {
  data: {
    first: number
    keys: Array<{ enabled: boolean; key_id: string; label: string }>
    rows: number
    total: number
  }
}

export function keyListResponse(
  keyIds: string[],
  first = 0,
  rows = 10,
  total = keyIds.length,
): KeyListResponse {
  return {
    data: {
      first,
      keys: keyIds.map((key_id) => ({ enabled: true, key_id, label: key_id })),
      rows,
      total,
    },
  }
}

export function deferredResponse<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((settle, fail) => {
    resolve = settle
    reject = fail
  })
  return { promise, reject, resolve }
}

export function deferred() {
  let resolve!: () => void
  const promise = new Promise<void>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

export function fresh() {
  setActivePinia(createPinia())
  for (const item of allMocks) item.mockClear()
  return useApiKeysStore()
}
