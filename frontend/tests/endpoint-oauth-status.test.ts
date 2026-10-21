import { expect, test } from 'bun:test'
import {
  deferred,
  freshOAuthHarness,
  loadOAuthActions,
  oauthStatus,
  getEndpointOAuthStatus,
} from './endpoint-oauth-actions.fixtures'

const { useEndpointOAuthActions } = await loadOAuthActions()

test('a late status response cannot replace the newly selected endpoint', async () => {
  const first = deferred<Awaited<ReturnType<typeof getEndpointOAuthStatus>>>()
  const h = freshOAuthHarness(useEndpointOAuthActions, 'endpoint-1', () => {
    getEndpointOAuthStatus.mockImplementation((id) =>
      id === 'endpoint-1'
        ? first.promise
        : Promise.resolve(
            oauthStatus({
              endpoint_id: id,
              has_oauth_token: true,
              plan: 'chatgpt_subscription',
            }),
          ),
    )
  })

  h.id.value = 'endpoint-2'
  await h.settle()
  expect(h.actions.status.value?.endpoint_id).toBe('endpoint-2')
  first.resolve(oauthStatus({ endpoint_id: 'endpoint-1' }))
  await h.settle()

  expect(h.actions.status.value?.endpoint_id).toBe('endpoint-2')
  expect(h.actions.status.value?.has_oauth_token).toBe(true)
  expect(h.statusEvents.at(-1)).toMatchObject({
    has_oauth_token: true,
    plan: 'chatgpt_subscription',
  })
  h.scope.stop()
})

test('the editor reads the login state only and never the subscription quota', async () => {
  const h = freshOAuthHarness(useEndpointOAuthActions, 'endpoint-1', () => {
    getEndpointOAuthStatus.mockImplementation(async (id) =>
      oauthStatus({ endpoint_id: id, has_oauth_token: id === 'endpoint-1' }),
    )
  })
  await h.settle()

  expect(h.actions.status.value?.endpoint_id).toBe('endpoint-1')
  expect(h.actions.status.value?.has_oauth_token).toBe(true)
  expect('quota' in h.actions).toBe(false)
  expect('quotaError' in h.actions).toBe(false)
  h.scope.stop()
})
