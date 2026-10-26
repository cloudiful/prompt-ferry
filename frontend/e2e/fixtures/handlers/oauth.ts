import type {
  OAuthBrowserStartResponse,
  OAuthDeviceStartResponse,
  OAuthLoginResponse,
  OAuthFlowRequest,
  OAuthBrowserCompleteRequest,
  EndpointOAuthStatusResponse,
} from '../../../src/generated/admin-api'
import { error, ok, readJson, requireAdmin, type FixtureContext } from '../http'

const flows = new WeakMap<object, Map<string, number>>()
export async function handleOAuth(
  ctx: FixtureContext,
): Promise<Response | null> {
  const match = /^\/api\/v1\/admin\/endpoints\/([^/]+)\/oauth(?:\/(.*))?$/.exec(
    ctx.url.pathname,
  )
  if (!match) return null
  const denied = requireAdmin(ctx)
  if (denied) return denied
  const endpoint = ctx.state.endpoints.find((e) => e.endpoint_id === match[1])
  if (!endpoint) return error('not_found', 'fixture endpoint missing', 404)
  const suffix = match[2] ?? ''
  const method = ctx.request.method
  let state = flows.get(ctx.state)
  if (!state) {
    state = new Map()
    flows.set(ctx.state, state)
  }
  const flowId = `fixture-${endpoint.endpoint_id}`
  if (method === 'GET' && !suffix) {
    const response: EndpointOAuthStatusResponse = {
      endpoint_id: endpoint.endpoint_id,
      has_oauth_token: endpoint.has_oauth_token === true,
      expired: false,
      expires_at: endpoint.has_oauth_token ? '2026-11-23T12:00:00Z' : null,
      plan: endpoint.plan ?? 'platform_api_key',
    }
    return ok(response)
  }
  if (method === 'DELETE' && !suffix) {
    endpoint.has_oauth_token = false
    endpoint.plan = 'platform_api_key'
    return new Response(null, { status: 204 })
  }
  if (method !== 'POST') return null
  if (suffix === 'device') {
    state.set(flowId, 0)
    const response: OAuthDeviceStartResponse = {
      flow_id: flowId,
      expires_in_seconds: 60,
      interval_seconds: 1,
      user_code: 'FIXTURE-CODE',
      verification_uri: 'http://127.0.0.1:8789/__fixtures/never-open',
    }
    return ok(response)
  }
  if (suffix === 'browser') {
    state.set(flowId, 0)
    const response: OAuthBrowserStartResponse = {
      flow_id: flowId,
      expires_in_seconds: 60,
      authorize_url: 'http://127.0.0.1:8789/__fixtures/never-open',
      redirect_uri: 'http://127.0.0.1:1455/auth/callback',
    }
    return ok(response)
  }
  if (suffix === 'device/poll' || suffix === 'browser/complete') {
    const body = await readJson<OAuthFlowRequest | OAuthBrowserCompleteRequest>(
      ctx,
    )
    const polls = state.get(body.flow_id)
    if (polls === undefined)
      return error('not_found', 'fixture flow expired', 404)
    state.set(body.flow_id, polls + 1)
    const complete = suffix === 'browser/complete' || polls > 0
    if (complete) {
      endpoint.has_oauth_token = true
      endpoint.plan = 'chatgpt_subscription'
    }
    const response: OAuthLoginResponse = {
      flow_id: body.flow_id,
      status: complete ? 'complete' : 'pending',
      endpoint: complete ? endpoint : null,
    }
    return ok(response)
  }
  if (suffix === 'refresh') {
    endpoint.has_oauth_token = true
    endpoint.plan = 'chatgpt_subscription'
    const response: EndpointOAuthStatusResponse = {
      endpoint_id: endpoint.endpoint_id,
      has_oauth_token: true,
      expired: false,
      expires_at: '2026-11-23T12:00:00Z',
      plan: 'chatgpt_subscription',
    }
    return ok(response)
  }
  return null
}
