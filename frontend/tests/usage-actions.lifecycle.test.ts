import { expect, test } from 'bun:test'

import {
  deferred,
  freshDetailActions,
  setConversationEndpointOverride,
} from './request-record-actions.fixtures'

test('a stale override completion cannot replace the newly selected conversation', async () => {
  const h = await freshDetailActions()
  const gate = deferred<void>()
  const previous = { endpoint_id: 'previous' }
  h.conversationOverride.value = previous as never
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

  const saving = h.actions.saveConversationOverride('conversation-a', 'a', null)
  h.detailRecord.value = {
    conversation_id: 'conversation-b',
    record_id: 2,
  } as never
  h.sessionRouteOptions.value = { conversation_id: 'conversation-b' } as never
  gate.resolve()

  await expect(saving).resolves.toMatchObject({ executed: true })
  expect(h.conversationOverride.value).toMatchObject(previous)
  expect(h.reloads).toEqual([])
})

test('a failed override write releases the conversation gate for retry', async () => {
  const h = await freshDetailActions()
  setConversationEndpointOverride.mockImplementationOnce(async () => {
    throw new Error('rejected')
  })

  await expect(
    h.actions.saveConversationOverride('conversation-a', 'endpoint-a', null),
  ).rejects.toThrow('rejected')
  expect(h.guard.isPending('conversation:conversation-a')).toBe(false)

  await expect(
    h.actions.saveConversationOverride('conversation-a', 'endpoint-a', null),
  ).resolves.toMatchObject({ executed: true })
  expect(h.reloads).toEqual([1])
})
