import { expect, test } from 'bun:test'
import { parse } from 'vue/compiler-sfc'
import {
  deferred,
  freshOAuthHarness,
  loadOAuthActions,
  oauthStatus,
  clearEndpointOAuthToken,
  getEndpointOAuthStatus,
  getEndpointTokenPlanUsage,
  refreshEndpointOAuthToken,
} from './endpoint-oauth-actions.fixtures'

const { useEndpointOAuthActions } = await loadOAuthActions()

test('OAuth action entry points share the endpoint gate with saves', async () => {
  const h = freshOAuthHarness(useEndpointOAuthActions)
  await h.settle()
  const held = deferred()
  const saving = h.runAction('endpoint-1', () => held.promise, 'save')

  await h.actions.refreshToken()
  expect(refreshEndpointOAuthToken).not.toHaveBeenCalled()
  expect(h.guard.pendingAction('endpoint:endpoint-1')).toBe('save')
  expect(h.actions.refreshing.value).toBe(false)

  held.resolve()
  await saving
  h.scope.stop()
})

test('token refresh holds the shared key until its quota read settles', async () => {
  const h = freshOAuthHarness(useEndpointOAuthActions)
  await h.settle()
  const quota =
    deferred<Awaited<ReturnType<typeof getEndpointTokenPlanUsage>>>()
  refreshEndpointOAuthToken.mockResolvedValueOnce(
    oauthStatus({ has_oauth_token: true, plan: 'chatgpt_subscription' }),
  )
  getEndpointTokenPlanUsage.mockImplementationOnce(() => quota.promise)

  const refreshing = h.actions.refreshToken()
  await h.settle()
  expect(h.guard.isPending('endpoint:endpoint-1')).toBe(true)
  expect(h.guard.pendingAction('endpoint:endpoint-1')).toBe('oauth-refresh')
  expect(h.actions.refreshing.value).toBe(true)
  expect(
    await h.runAction('endpoint-1', async () => 'blocked', 'save'),
  ).toEqual({ executed: false })

  quota.resolve({ keys: [] } as Awaited<
    ReturnType<typeof getEndpointTokenPlanUsage>
  >)
  await refreshing
  expect(h.guard.isPending('endpoint:endpoint-1')).toBe(false)
  expect(h.actions.status.value?.plan).toBe('chatgpt_subscription')
  expect(h.actions.refreshing.value).toBe(false)
  h.scope.stop()
})

test('clear remains pending through its status refresh', async () => {
  const h = freshOAuthHarness(useEndpointOAuthActions)
  await h.settle()
  const status = deferred<Awaited<ReturnType<typeof getEndpointOAuthStatus>>>()
  getEndpointOAuthStatus.mockImplementationOnce(() => status.promise)

  const clearing = h.actions.clearToken()
  await h.settle()
  expect(h.guard.pendingAction('endpoint:endpoint-1')).toBe('oauth-clear')
  expect(h.actions.clearing.value).toBe(true)

  status.resolve(oauthStatus({ endpoint_id: 'endpoint-1' }))
  await clearing
  expect(h.guard.isPending('endpoint:endpoint-1')).toBe(false)
  expect(h.actions.clearing.value).toBe(false)
  h.scope.stop()
})

test('a failed OAuth clear reports the error and permits retry', async () => {
  const h = freshOAuthHarness(useEndpointOAuthActions)
  await h.settle()
  clearEndpointOAuthToken.mockRejectedValueOnce(new Error('clear failed'))

  await h.actions.clearToken()
  expect(h.actions.statusError.value).toBe('clear failed')
  expect(h.actions.clearing.value).toBe(false)
  expect(h.guard.isPending('endpoint:endpoint-1')).toBe(false)
  h.scope.stop()
})

test('OAuth actions reach both login flows through the endpoint owner', async () => {
  const paths = [
    '../src/pages/EndpointsPage.vue',
    '../src/composables/useEndpointsPage.ts',
    '../src/components/endpoints/EndpointDialog.vue',
    '../src/components/endpoints/EndpointOAuthSection.vue',
    '../src/components/endpoints/EndpointOAuthDeviceLogin.vue',
    '../src/components/endpoints/EndpointOAuthBrowserLogin.vue',
  ]
  const sources = await Promise.all(
    paths.map((path) => Bun.file(new URL(path, import.meta.url)).text()),
  )
  const pageTemplate = parse(sources[0]!, { filename: 'EndpointsPage.vue' })
    .descriptor.template!.content
  const dialogTemplate = parse(sources[2]!, { filename: 'EndpointDialog.vue' })
    .descriptor.template!.content
  const oauthTemplate = parse(sources[3]!, {
    filename: 'EndpointOAuthSection.vue',
  }).descriptor.template!.content

  expect(sources[1]).toContain(
    'runEndpointOAuthAction: endpointsStore.runEndpointOAuthAction',
  )
  expect(pageTemplate).toContain(':run-oauth-action="runEndpointOAuthAction"')
  expect(pageTemplate).toContain(':pending-action="endpointPendingAction"')
  expect(dialogTemplate).toContain(':run-action="runOAuthAction"')
  expect(dialogTemplate).toContain(':dismissible="!savePending"')
  expect(dialogTemplate).toContain("pendingAction === 'save'")
  expect(oauthTemplate).toContain('@start="startDevice"')
  expect(oauthTemplate).toContain('@complete="completeBrowser"')
  expect(oauthTemplate).toContain(':actions-enabled="actionsEnabled"')
  for (const source of sources.slice(4)) {
    expect(source).toContain("$emit('start')")
    expect(source).toContain('actionsEnabled')
  }
})
