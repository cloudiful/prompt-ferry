import { expect, test } from 'bun:test'

import {
  deferred,
  freshDetailActions,
  requestRecordResetSessionAffinity,
  setConversationEndpointOverride,
} from './request-record-actions.fixtures'

test('override save, clear, and affinity reset share the conversation gate', async () => {
  const h = await freshDetailActions()
  const gate = deferred<void>()
  setConversationEndpointOverride.mockImplementationOnce(async () => {
    await gate.promise
    return {
      data: {
        conversation_id: 'conversation-a',
        endpoint_id: 'endpoint-a',
        endpoint_key_id: 'key-a',
        endpoint_key_label: 'key A',
        endpoint_name: 'Endpoint A',
        created_at: '',
        updated_at: '',
        created_by_user_id: null,
      },
    }
  })

  const saving = h.actions.saveConversationOverride(
    'conversation-a',
    'endpoint-a',
    'key-a',
  )
  expect(h.actions.conversationAction('conversation-a')).toBe('save')
  expect(
    await h.actions.saveConversationOverride('conversation-a', 'b', null),
  ).toEqual({ executed: false })
  expect(await h.actions.clearConversationOverride('conversation-a')).toEqual({
    executed: false,
  })
  expect(await h.actions.guardedResetSessionAffinity(1)).toEqual({
    executed: false,
  })
  expect(requestRecordResetSessionAffinity).not.toHaveBeenCalled()

  h.detailRecord.value = null
  expect(
    await h.actions.saveConversationOverride('conversation-a', 'c', null),
  ).toEqual({ executed: false })
  expect(h.guard.isPending('conversation:conversation-a')).toBe(true)
  gate.resolve()
  await expect(saving).resolves.toMatchObject({ executed: true })
  expect(h.actions.conversationAction('conversation-a')).toBeUndefined()
})

test('a different conversation has an independent write key', async () => {
  const h = await freshDetailActions()
  const gate = deferred<void>()
  setConversationEndpointOverride.mockImplementationOnce(async () => {
    await gate.promise
    return {
      data: {
        conversation_id: 'conversation-a',
        endpoint_id: 'endpoint-a',
        endpoint_key_id: null,
        endpoint_key_label: null,
        endpoint_name: 'Endpoint A',
        created_at: '',
        updated_at: '',
        created_by_user_id: null,
      },
    }
  })
  const first = h.actions.saveConversationOverride('conversation-a', 'a', null)
  const other = await h.actions.clearConversationOverride('conversation-b')
  expect(other).toMatchObject({ executed: true })
  expect(h.guard.isPending('conversation:conversation-a')).toBe(true)
  expect(h.guard.isPending('conversation:conversation-b')).toBe(false)

  gate.resolve()
  await first
})
