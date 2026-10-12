import { expect, mock, test } from 'bun:test'

const actualAdminApi = await import('../src/generated/admin-api')

const approveApproval = mock(async () => ({ data: null }))
const getApproval = mock(async () => ({ data: { approval_id: 'a' } }))
const listApprovals = mock(async () => ({
  data: { approvals: [], first: 0, rows: 10, total: 0 },
}))
const rejectApproval = mock(async () => ({ data: null }))

// Keep every other generated export intact: `mock.module` is process-global and
// would otherwise break suites that import sibling SDK functions.
mock.module('../src/generated/admin-api', () => ({
  ...actualAdminApi,
  approveApproval,
  getApproval,
  listApprovals,
  rejectApproval,
}))

const { createPinia, setActivePinia } = await import('pinia')
const { useApprovalsStore } = await import('../src/stores/approvals')

/** A gate the tests open by hand, so "in flight" is really in flight. */
function deferred() {
  let resolve!: () => void
  const promise = new Promise<void>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

const allMocks = [approveApproval, getApproval, listApprovals, rejectApproval]

function fresh() {
  setActivePinia(createPinia())
  for (const item of allMocks) item.mockClear()
  return useApprovalsStore()
}

test('approve and reject of one request are mutually exclusive', async () => {
  const store = fresh()
  const gate = deferred()
  approveApproval.mockImplementationOnce(async () => {
    await gate.promise
    return { data: null }
  })

  const approving = store.approve('a')
  expect(store.isPending('approval:a')).toBe(true)
  expect(store.approvalPendingActions).toEqual({ a: 'approve' })

  // The two answers are to the same question: only one may be sent.
  expect(await store.reject('a')).toEqual({ executed: false })
  expect(await store.approve('a')).toEqual({ executed: false })
  expect(rejectApproval).not.toHaveBeenCalled()

  gate.resolve()
  await approving
  expect(store.isPending('approval:a')).toBe(false)
})

test('another request stays answerable while one is being answered', async () => {
  const store = fresh()
  const gate = deferred()
  approveApproval.mockImplementationOnce(async () => {
    await gate.promise
    return { data: null }
  })

  const approving = store.approve('a')
  await expect(store.approve('b')).resolves.toMatchObject({ executed: true })
  expect(store.approvalPendingActions).toEqual({ a: 'approve' })

  gate.resolve()
  await approving
})

test('the gate is held through the refresh that follows the answer', async () => {
  const store = fresh()
  await store.refresh()
  const before = listApprovals.mock.calls.length

  const answering = store.approve('a')
  // The request is sent; the list it would change has not landed yet.
  expect(approveApproval).toHaveBeenCalledTimes(1)
  expect(store.isPending('approval:a')).toBe(true)

  await answering
  expect(listApprovals.mock.calls.length).toBeGreaterThan(before)
  expect(store.isPending('approval:a')).toBe(false)
})

test('answering one request no longer blanks the whole page', async () => {
  const store = fresh()
  const gate = deferred()
  approveApproval.mockImplementationOnce(async () => {
    await gate.promise
    return { data: null }
  })

  const answering = store.approve('a')
  // `loading` stays the fetch-only flag; the write is reported per row instead.
  expect(store.loading).toBe(false)
  expect(store.approvalPendingActions).toEqual({ a: 'approve' })

  gate.resolve()
  await answering
})

test('the open detail reloads only for the request that was answered', async () => {
  const store = fresh()
  await store.loadDetail('a')
  getApproval.mockClear()

  await store.approve('a')
  expect(getApproval).toHaveBeenCalledTimes(1)

  getApproval.mockClear()
  await store.approve('b')
  // `b` is not the open detail, so the operator's panel is left alone.
  expect(getApproval).not.toHaveBeenCalled()
})

test('a failed answer releases its key so the retry is not blocked', async () => {
  const store = fresh()
  approveApproval.mockImplementationOnce(async () => {
    throw new Error('rejected')
  })

  await expect(store.approve('a')).rejects.toThrow('rejected')
  expect(store.isPending('approval:a')).toBe(false)

  await expect(store.approve('a')).resolves.toMatchObject({ executed: true })
})
