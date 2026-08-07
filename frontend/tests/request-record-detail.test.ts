import { expect, mock, test } from 'bun:test'

const resetAffinity = mock(async () => ({
  data: { cleared: true, cleared_count: 2 },
}))
const loadRouteOptions = mock(async () => ({
  data: {
    affinity: {
      endpoint_id: null,
      endpoint_name: null,
      key_id: null,
      key_label: null,
      rule_id: null,
      state: 'unbound',
    },
    conversation_id: 'conv-1',
    current_endpoint_id: 'endpoint-1',
    current_endpoint_key_id: null,
    current_endpoint_key_label: null,
    options: [],
    override_endpoint_id: null,
    override_endpoint_key_id: null,
    override_endpoint_key_label: null,
  },
}))

mock.module('../src/generated/admin-api', () => ({
  deleteConversationEndpointOverride: mock(),
  requestRecordDetail: mock(),
  requestRecordFull: mock(),
  requestRecordResetSessionAffinity: resetAffinity,
  requestRecordSessionRouteOptions: loadRouteOptions,
  setConversationEndpointOverride: mock(),
}))

mock.module('../src/composables/useLocale', () => ({
  useLocale: () => ({ t: (key: string) => key }),
}))

const { createRequestRecordDetailState } =
  await import('../src/stores/request-record-detail')

function detailStateWithRecord(recordId: number) {
  const state = createRequestRecordDetailState()
  state.detailRecord.value = { record_id: recordId } as never
  return state
}

test('reset session affinity uses the current record id and reloads realtime state', async () => {
  resetAffinity.mockClear()
  loadRouteOptions.mockClear()
  const state = detailStateWithRecord(42)

  const result = await state.resetSessionAffinity(42)

  expect(resetAffinity).toHaveBeenCalledTimes(1)
  expect(resetAffinity).toHaveBeenCalledWith({
    path: { record_id: 42 },
    responseStyle: 'data',
  })
  expect(result.cleared).toBe(true)
  expect(result.cleared_count).toBe(2)
  expect(loadRouteOptions).toHaveBeenCalledTimes(1)
  expect(state.sessionRouteOptions.value?.affinity.state).toBe('unbound')
  expect(state.affinityResetting.value).toBe(false)
})

test('reset session affinity propagates API errors and clears loading', async () => {
  resetAffinity.mockImplementation(async () => {
    throw new Error('backend unavailable')
  })
  const state = detailStateWithRecord(7)

  await expect(state.resetSessionAffinity(7)).rejects.toThrow(
    'backend unavailable',
  )
  expect(state.affinityResetting.value).toBe(false)
})

test('reset session affinity parses idempotent empty results', async () => {
  resetAffinity.mockImplementation(async () => ({
    data: { cleared: false, cleared_count: 0 },
  }))
  const state = detailStateWithRecord(9)

  const result = await state.resetSessionAffinity(9)

  expect(result.cleared).toBe(false)
  expect(result.cleared_count).toBe(0)
})
