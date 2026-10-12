import { expect, test } from 'bun:test'
import {
  deferred,
  freshOAuthHarness,
  loadOAuthActions,
  oauthStatus,
  getEndpointOAuthStatus,
  getEndpointTokenPlanUsage,
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

test('a late quota response is ignored after the endpoint changes', async () => {
  const oldQuota =
    deferred<Awaited<ReturnType<typeof getEndpointTokenPlanUsage>>>()
  const h = freshOAuthHarness(useEndpointOAuthActions, 'endpoint-1', () => {
    getEndpointOAuthStatus.mockImplementation(async (id) =>
      oauthStatus({ endpoint_id: id, has_oauth_token: id === 'endpoint-1' }),
    )
    getEndpointTokenPlanUsage.mockImplementationOnce(() => oldQuota.promise)
  })
  await h.settle()

  h.id.value = 'endpoint-2'
  await h.settle()
  oldQuota.resolve({ keys: [] } as Awaited<
    ReturnType<typeof getEndpointTokenPlanUsage>
  >)
  await h.settle()

  expect(h.actions.status.value?.endpoint_id).toBe('endpoint-2')
  expect(h.actions.status.value?.has_oauth_token).toBe(false)
  expect(h.actions.quota.value).toBeNull()
  h.scope.stop()
})
