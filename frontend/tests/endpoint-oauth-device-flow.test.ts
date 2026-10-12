import { expect, test } from 'bun:test'
import {
  deferred,
  freshOAuthHarness,
  loadOAuthActions,
  loginResponse,
  oauthStatus,
  getEndpointOAuthStatus,
  getEndpointTokenPlanUsage,
  pollEndpointDeviceLogin,
  startEndpointDeviceLogin,
  deviceStart,
} from './endpoint-oauth-actions.fixtures'

const { useEndpointOAuthActions } = await loadOAuthActions()

test('device start releases the endpoint gate during human approval', async () => {
  const h = freshOAuthHarness(useEndpointOAuthActions)
  await h.settle()
  const startRequest = deferred<ReturnType<typeof deviceStart>>()
  startEndpointDeviceLogin.mockImplementationOnce(() => startRequest.promise)

  const starting = h.actions.deviceFlow.startDevice()
  await h.settle()
  expect(h.guard.pendingAction('endpoint:endpoint-1')).toBe(
    'oauth-device-start',
  )
  startRequest.resolve(deviceStart())
  await starting

  expect(h.guard.isPending('endpoint:endpoint-1')).toBe(false)
  expect(h.actions.deviceFlow.device.value?.flow_id).toBe('device-flow-1')
  expect(h.actions.deviceFlow.starting.value).toBe(false)
  h.scope.stop()
})

test('device poll holds the endpoint gate through completion status and quota reads', async () => {
  const h = freshOAuthHarness(useEndpointOAuthActions)
  await h.settle()
  await h.actions.deviceFlow.startDevice()
  const quota =
    deferred<Awaited<ReturnType<typeof getEndpointTokenPlanUsage>>>()
  pollEndpointDeviceLogin.mockResolvedValueOnce(
    loginResponse({ flow_id: 'device-flow-1', status: 'complete' }),
  )
  getEndpointOAuthStatus.mockResolvedValueOnce(
    oauthStatus({ has_oauth_token: true, plan: 'chatgpt_subscription' }),
  )
  getEndpointTokenPlanUsage.mockImplementationOnce(() => quota.promise)

  const polling = h.actions.deviceFlow.pollDevice()
  await h.settle()
  expect(h.guard.pendingAction('endpoint:endpoint-1')).toBe('oauth-device-poll')
  expect(pollEndpointDeviceLogin).toHaveBeenCalledWith(
    'endpoint-1',
    'device-flow-1',
  )

  quota.resolve({ keys: [] } as Awaited<
    ReturnType<typeof getEndpointTokenPlanUsage>
  >)
  await polling
  expect(h.guard.isPending('endpoint:endpoint-1')).toBe(false)
  expect(h.actions.deviceFlow.device.value).toBeNull()
  expect(h.actions.status.value?.has_oauth_token).toBe(true)
  h.scope.stop()
})

test('a poll skipped by a conflicting endpoint write keeps the human flow', async () => {
  const h = freshOAuthHarness(useEndpointOAuthActions)
  await h.settle()
  await h.actions.deviceFlow.startDevice()
  const held = deferred()
  const saving = h.runAction('endpoint-1', () => held.promise, 'save')

  await h.actions.deviceFlow.pollDevice()
  expect(pollEndpointDeviceLogin).not.toHaveBeenCalled()
  expect(h.actions.deviceFlow.device.value?.flow_id).toBe('device-flow-1')
  expect(h.guard.pendingAction('endpoint:endpoint-1')).toBe('save')

  held.resolve()
  await saving
  h.scope.stop()
})

test('a late device start cannot install a flow on a changed endpoint', async () => {
  const h = freshOAuthHarness(useEndpointOAuthActions)
  await h.settle()
  const startRequest = deferred<ReturnType<typeof deviceStart>>()
  startEndpointDeviceLogin.mockImplementationOnce(() => startRequest.promise)

  const starting = h.actions.deviceFlow.startDevice()
  await h.settle()
  h.id.value = 'endpoint-2'
  startRequest.resolve(deviceStart({ flow_id: 'old-flow' }))
  await starting
  await h.settle()

  expect(startEndpointDeviceLogin).toHaveBeenCalledWith('endpoint-1')
  expect(h.actions.deviceFlow.device.value).toBeNull()
  expect(h.guard.isPending('endpoint:endpoint-1')).toBe(false)
  h.scope.stop()
})
