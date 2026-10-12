import { expect, test } from 'bun:test'
import {
  browserStart,
  deferred,
  freshOAuthHarness,
  loadOAuthActions,
  loginResponse,
  oauthStatus,
  completeEndpointBrowserLogin,
  getEndpointOAuthStatus,
  getEndpointTokenPlanUsage,
} from './endpoint-oauth-actions.fixtures'

const { useEndpointOAuthActions } = await loadOAuthActions()

test('browser start releases the endpoint gate while the operator authenticates', async () => {
  const h = freshOAuthHarness(useEndpointOAuthActions)
  await h.settle()
  await h.actions.browserFlow.startBrowser()

  expect(h.guard.isPending('endpoint:endpoint-1')).toBe(false)
  expect(h.actions.browserFlow.browser.value?.flow_id).toBe('browser-flow-1')
  expect(h.actions.browserFlow.busy.value).toBe(false)
  h.scope.stop()
})

test('browser completion captures its inputs and remains pending through refresh', async () => {
  const h = freshOAuthHarness(useEndpointOAuthActions)
  await h.settle()
  await h.actions.browserFlow.startBrowser()
  h.actions.browserFlow.redirectUrl.value =
    '  https://example.test/callback?code=abc  '
  const quota =
    deferred<Awaited<ReturnType<typeof getEndpointTokenPlanUsage>>>()
  completeEndpointBrowserLogin.mockResolvedValueOnce(
    loginResponse({ flow_id: 'browser-flow-1', status: 'complete' }),
  )
  getEndpointOAuthStatus.mockResolvedValueOnce(
    oauthStatus({ has_oauth_token: true, plan: 'chatgpt_subscription' }),
  )
  getEndpointTokenPlanUsage.mockImplementationOnce(() => quota.promise)

  const completing = h.actions.browserFlow.completeBrowser()
  await h.settle()
  expect(completeEndpointBrowserLogin).toHaveBeenCalledWith(
    'endpoint-1',
    'browser-flow-1',
    'https://example.test/callback?code=abc',
  )
  expect(h.guard.pendingAction('endpoint:endpoint-1')).toBe(
    'oauth-browser-complete',
  )
  expect(h.actions.browserFlow.busy.value).toBe(true)

  quota.resolve({ keys: [] } as Awaited<
    ReturnType<typeof getEndpointTokenPlanUsage>
  >)
  await completing
  expect(h.guard.isPending('endpoint:endpoint-1')).toBe(false)
  expect(h.actions.browserFlow.browser.value).toBeNull()
  expect(h.actions.browserFlow.redirectUrl.value).toBe('')
  h.scope.stop()
})

test('a pending browser response keeps the approval flow available', async () => {
  const h = freshOAuthHarness(useEndpointOAuthActions)
  await h.settle()
  await h.actions.browserFlow.startBrowser()
  h.actions.browserFlow.redirectUrl.value =
    'https://example.test/callback?code=pending'
  completeEndpointBrowserLogin.mockResolvedValueOnce(
    loginResponse({ flow_id: 'browser-flow-1', status: 'pending' }),
  )

  await h.actions.browserFlow.completeBrowser()
  expect(h.actions.browserFlow.browser.value?.flow_id).toBe('browser-flow-1')
  expect(h.actions.browserFlow.redirectUrl.value).toBe(
    'https://example.test/callback?code=pending',
  )
  expect(h.guard.isPending('endpoint:endpoint-1')).toBe(false)
  h.scope.stop()
})

test('a completion for the previous endpoint cannot clear a new browser flow', async () => {
  const h = freshOAuthHarness(useEndpointOAuthActions)
  await h.settle()
  await h.actions.browserFlow.startBrowser()
  h.actions.browserFlow.redirectUrl.value =
    'https://example.test/callback?code=old'
  const oldCompletion = deferred<ReturnType<typeof loginResponse>>()
  completeEndpointBrowserLogin.mockImplementationOnce(
    () => oldCompletion.promise,
  )

  const completing = h.actions.browserFlow.completeBrowser()
  await h.settle()
  h.id.value = 'endpoint-2'
  await h.settle()
  await h.actions.browserFlow.startBrowser()
  oldCompletion.resolve(
    loginResponse({ flow_id: 'browser-flow-1', status: 'complete' }),
  )
  await completing

  expect(h.actions.browserFlow.browser.value).toEqual(browserStart())
  expect(h.guard.isPending('endpoint:endpoint-1')).toBe(false)
  expect(h.guard.isPending('endpoint:endpoint-2')).toBe(false)
  h.scope.stop()
})
