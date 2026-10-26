import type {
  ClientKey,
  CreateClientKeyRequest,
  CreateUserRequest,
  EndpointRequest,
  ModelRouteRequest,
  ProviderEndpoint,
  ResetPasswordRequest,
  UpdateClientKeyRequest,
  User,
  UserUpdate,
} from '../../../src/generated/admin-api'
import {
  error,
  nextId,
  ok,
  page,
  queryInt,
  queryString,
  readJson,
  requireAdmin,
  type FixtureContext,
} from '../http'
import { isoNow } from '../http'

const USER_PAGE = /^\/api\/v1\/admin\/users$/
const USER_OPTIONS = /^\/api\/v1\/admin\/users\/options$/
const USER_ONE = /^\/api\/v1\/admin\/users\/([^/]+)$/
const USER_KEYS = /^\/api\/v1\/admin\/users\/([^/]+)\/client-keys$/
const USER_KEY_ONE = /^\/api\/v1\/admin\/users\/([^/]+)\/client-keys\/([^/]+)$/
const USER_PASSWORD = /^\/api\/v1\/admin\/users\/([^/]+)\/reset-password$/
const ME_KEYS = /^\/api\/v1\/me\/client-keys$/
const ME_KEY_ONE = /^\/api\/v1\/me\/client-keys\/([^/]+)$/
const ME_MODELS = /^\/api\/v1\/me\/models$/
const ENDPOINTS = /^\/api\/v1\/admin\/endpoints$/
const ENDPOINT_ONE = /^\/api\/v1\/admin\/endpoints\/([^/]+)$/
const ENDPOINT_MODELS = /^\/api\/v1\/admin\/endpoints\/([^/]+)\/models$/
const ENDPOINT_TEST = /^\/api\/v1\/admin\/endpoints\/([^/]+)\/test$/
const ENDPOINT_TOKEN_PLAN =
  /^\/api\/v1\/admin\/endpoints\/([^/]+)\/token-plan-usage$/
const ENDPOINT_QUOTA_HISTORY =
  /^\/api\/v1\/admin\/endpoints\/([^/]+)\/quota-history$/
const ENDPOINT_QUOTA_OBS =
  /^\/api\/v1\/admin\/endpoints\/([^/]+)\/quota-observations$/
const ENDPOINT_QUOTA_TREND =
  /^\/api\/v1\/admin\/endpoints\/([^/]+)\/quota-observations\/trend$/
const ROUTES = /^\/api\/v1\/admin\/model-routes$/
const ROUTE_ONE = /^\/api\/v1\/admin\/model-routes\/([^/]+)$/
const ROUTE_TEST = /^\/api\/v1\/admin\/model-routes\/test$/
const MCP_PROVIDERS = /^\/api\/v1\/admin\/mcp-providers$/
const MCP_SERVERS = /^\/api\/v1\/admin\/mcp-servers(?:\/[^/]+)*$/

function userView(user: User): User {
  return { ...user }
}

function keyView(key: ClientKey): ClientKey {
  return { ...key, secret: null }
}

function newKey(ctx: FixtureContext, userId: number, label: string): ClientKey {
  const now = isoNow()
  return {
    created_at: now,
    enabled: true,
    key_id: nextId(ctx.state, 'key'),
    key_prefix: 'pf_fixture_',
    label,
    last_used_at: null,
    secret: null,
    user_id: userId,
  }
}

export async function handleCatalog(
  ctx: FixtureContext,
): Promise<Response | null> {
  const { method } = ctx.request
  const { pathname } = ctx.url

  if (ME_MODELS.test(pathname) && method === 'GET') {
    if (!ctx.account) return error('unauthorized', 'login required', 401)
    return ok(
      page(
        ctx.state.availableModels,
        queryInt(ctx.url, 'first', 0),
        queryInt(ctx.url, 'rows', 20),
        'models',
      ),
    )
  }

  if (ME_KEYS.test(pathname) || ME_KEY_ONE.test(pathname)) {
    if (!ctx.account) return error('unauthorized', 'login required', 401)
    const userId = ctx.account.user.user_id
    if (method === 'GET' && ME_KEYS.test(pathname)) {
      const keys = ctx.state.clientKeys.filter((key) => key.user_id === userId)
      return ok(
        page(
          keys,
          queryInt(ctx.url, 'first', 0),
          queryInt(ctx.url, 'rows', 10),
          'keys',
        ),
      )
    }
    if (method === 'POST' && ME_KEYS.test(pathname)) {
      const body = await readJson<CreateClientKeyRequest>(ctx)
      const key = newKey(ctx, userId, body.label?.trim() || 'fixture-key')
      key.secret = 'pf_fixture_secret_revealed_once'
      ctx.state.clientKeys.push(key)
      return ok({ ...key, secret: key.secret })
    }
    const keyId = ME_KEY_ONE.exec(pathname)?.[1]
    if (!keyId) return null
    const key = ctx.state.clientKeys.find(
      (candidate) => candidate.key_id === keyId && candidate.user_id === userId,
    )
    if (!key) return error('not_found', 'fixture key not found', 404)
    if (method === 'PATCH') {
      const body = await readJson<UpdateClientKeyRequest>(ctx)
      if (typeof body.label === 'string' && body.label.trim())
        key.label = body.label.trim()
      if (typeof body.enabled === 'boolean') key.enabled = body.enabled
      return ok(keyView(key))
    }
    if (method === 'DELETE') {
      ctx.state.clientKeys = ctx.state.clientKeys.filter(
        (candidate) => candidate !== key,
      )
      return new Response(null, { status: 204 })
    }
  }

  if (MCP_PROVIDERS.test(pathname) || MCP_SERVERS.test(pathname)) return null
  if (/^\/api\/v1\/settings\/redaction(?:\/[^/]+)*$/.test(pathname)) {
    return null
  }
  if (/^\/api\/v1\/admin\/billing(?:\/[^/]+)*$/.test(pathname)) {
    return null
  }
  // The request-records namespace owns its own role boundary (self-scoped
  // reads, admin-only mutations), so it must not be rejected by this generic
  // admin gate first. The conversation override routes stay admin-only.
  if (/^\/api\/v1\/admin\/request-records(?:\/[^/]+)*$/.test(pathname)) {
    return null
  }

  const adminMissing = requireAdmin(ctx)
  if (adminMissing) return adminMissing

  if (USER_OPTIONS.test(pathname) && method === 'GET') {
    return ok({
      users: ctx.state.accounts.map((account) => userView(account.user)),
    })
  }

  if (USER_PAGE.test(pathname)) {
    if (method === 'GET') {
      return ok(
        page(
          ctx.state.accounts.map((account) => userView(account.user)),
          queryInt(ctx.url, 'first', 0),
          queryInt(ctx.url, 'rows', 10),
          'users',
        ),
      )
    }
    if (method === 'POST') {
      const body = await readJson<CreateUserRequest>(ctx)
      const now = isoNow()
      const user: User = {
        created_at: now,
        display_name: body.display_name,
        is_active: true,
        is_admin: body.is_admin === true,
        login_name: body.login_name,
        updated_at: now,
        user_id: ctx.state.nextId++,
      }
      ctx.state.accounts.push({
        isAdmin: user.is_admin,
        password: body.password || 'fixture-password',
        user,
      })
      return ok(user)
    }
  }

  const userId = USER_ONE.exec(pathname)?.[1]
  if (userId && method === 'PATCH') {
    const account = ctx.state.accounts.find(
      (candidate) => String(candidate.user.user_id) === userId,
    )
    if (!account) return error('not_found', 'fixture user not found', 404)
    const body = await readJson<UserUpdate>(ctx)
    if (typeof body.display_name === 'string') {
      account.user.display_name = body.display_name
    }
    if (typeof body.is_active === 'boolean')
      account.user.is_active = body.is_active
    if (typeof body.is_admin === 'boolean') {
      account.user.is_admin = body.is_admin
      account.isAdmin = body.is_admin
    }
    account.user.updated_at = isoNow()
    return ok(userView(account.user))
  }
  if (userId && method === 'DELETE') {
    const account = ctx.state.accounts.find(
      (candidate) => String(candidate.user.user_id) === userId,
    )
    if (!account) return error('not_found', 'fixture user not found', 404)
    ctx.state.accounts = ctx.state.accounts.filter(
      (candidate) => candidate !== account,
    )
    return new Response(null, { status: 204 })
  }

  const passwordUserId = USER_PASSWORD.exec(pathname)?.[1]
  if (passwordUserId && method === 'POST') {
    const account = ctx.state.accounts.find(
      (candidate) => String(candidate.user.user_id) === passwordUserId,
    )
    if (!account) return error('not_found', 'fixture user not found', 404)
    const body = await readJson<ResetPasswordRequest>(ctx)
    account.password = body.password
    return new Response(null, { status: 204 })
  }

  const keysUserId = USER_KEYS.exec(pathname)?.[1]
  if (keysUserId && method === 'GET') {
    const keys = ctx.state.clientKeys.filter(
      (key) => String(key.user_id) === keysUserId,
    )
    return ok(
      page(
        keys.map(keyView),
        queryInt(ctx.url, 'first', 0),
        queryInt(ctx.url, 'rows', 10),
        'keys',
      ),
    )
  }
  if (keysUserId && method === 'POST') {
    const body = await readJson<CreateClientKeyRequest>(ctx)
    const key = newKey(
      ctx,
      Number.parseInt(keysUserId, 10),
      body.label?.trim() || 'fixture-key',
    )
    key.secret = 'pf_fixture_secret_revealed_once'
    ctx.state.clientKeys.push(key)
    return ok(key)
  }

  const keyMatch = USER_KEY_ONE.exec(pathname)
  if (keyMatch) {
    const [, ownerId, keyId] = keyMatch
    const key = ctx.state.clientKeys.find(
      (candidate) =>
        candidate.key_id === keyId && String(candidate.user_id) === ownerId,
    )
    if (!key) return error('not_found', 'fixture key not found', 404)
    if (method === 'PATCH') {
      const body = await readJson<UpdateClientKeyRequest>(ctx)
      if (typeof body.label === 'string' && body.label.trim())
        key.label = body.label.trim()
      if (typeof body.enabled === 'boolean') key.enabled = body.enabled
      return ok(keyView(key))
    }
    if (method === 'DELETE') {
      ctx.state.clientKeys = ctx.state.clientKeys.filter(
        (candidate) => candidate !== key,
      )
      return new Response(null, { status: 204 })
    }
  }

  if (ENDPOINTS.test(pathname)) {
    if (method === 'GET') {
      return ok(
        page(
          ctx.state.endpoints,
          queryInt(ctx.url, 'first', 0),
          queryInt(ctx.url, 'rows', 10),
          'endpoints',
        ),
      )
    }
    if (method === 'POST') {
      const body = await readJson<EndpointRequest>(ctx)
      const now = isoNow()
      const endpoint: ProviderEndpoint = {
        active_windows: body.active_windows ?? [],
        api_keys: (body.api_keys ?? []).map((key, index) => ({
          created_at: now,
          enabled: key.enabled !== false,
          endpoint_id: '',
          key_id: nextId(ctx.state, 'key'),
          key_label: key.key_label,
          position: index,
          updated_at: now,
        })),
        base_url: body.base_url,
        created_at: now,
        enabled: body.enabled !== false,
        endpoint_id: '',
        has_admin_api_key: Boolean(body.admin_api_key),
        has_oauth_token: false,
        has_proxy_url: Boolean(body.proxy_url),
        key_lb_enabled: body.key_lb_enabled !== false,
        mcp_enabled: body.mcp_enabled === true,
        name: body.name,
        native_api: 'chat',
        native_api_source: 'provider',
        owner_user_id: body.owner_user_id ?? null,
        plan: body.plan ?? 'platform_api_key',
        provider: body.provider ?? 'generic',
        provider_region: body.provider_region ?? null,
        scope: body.scope,
        service_tier: body.service_tier ?? null,
        updated_at: now,
      }
      endpoint.endpoint_id = nextId(ctx.state, 'ep')
      for (const key of endpoint.api_keys ?? [])
        key.endpoint_id = endpoint.endpoint_id
      ctx.state.endpoints.push(endpoint)
      return ok(endpoint)
    }
  }

  const endpointId = ENDPOINT_ONE.exec(pathname)?.[1]
  if (endpointId) {
    const endpoint = ctx.state.endpoints.find(
      (candidate) => candidate.endpoint_id === endpointId,
    )
    if (!endpoint) return error('not_found', 'fixture endpoint not found', 404)
    if (method === 'PATCH') {
      const body = await readJson<EndpointRequest>(ctx)
      endpoint.name = body.name
      endpoint.base_url = body.base_url
      endpoint.scope = body.scope
      endpoint.enabled = body.enabled !== false
      endpoint.key_lb_enabled = body.key_lb_enabled !== false
      endpoint.mcp_enabled = body.mcp_enabled === true
      endpoint.owner_user_id = body.owner_user_id ?? null
      endpoint.provider = body.provider ?? endpoint.provider
      endpoint.provider_region = body.provider_region ?? null
      endpoint.plan = body.plan ?? endpoint.plan
      endpoint.native_api = body.native_api_override ?? 'auto'
      endpoint.native_api_source =
        body.protocol_mode === 'manual' ? 'manual' : 'detected'
      if (body.admin_api_key !== undefined)
        endpoint.has_admin_api_key = Boolean(body.admin_api_key)
      endpoint.service_tier = body.service_tier ?? null
      endpoint.active_windows = body.active_windows ?? endpoint.active_windows
      if (body.proxy_url !== undefined && body.proxy_url !== null)
        endpoint.has_proxy_url = Boolean(body.proxy_url.trim())
      if (body.api_keys) {
        endpoint.api_keys = body.api_keys.map((key, index) => ({
          created_at: isoNow(),
          enabled: key.enabled !== false,
          endpoint_id: endpoint.endpoint_id,
          key_id: key.key_id ?? nextId(ctx.state, 'key'),
          key_label: key.key_label,
          position: index,
          updated_at: isoNow(),
        }))
      }
      endpoint.updated_at = isoNow()
      return ok(endpoint)
    }
    if (method === 'DELETE') {
      ctx.state.endpoints = ctx.state.endpoints.filter(
        (candidate) => candidate !== endpoint,
      )
      ctx.state.modelRoutes = ctx.state.modelRoutes.map((route) => ({
        ...route,
        targets: route.targets.filter(
          (target) => target.endpoint_id !== endpointId,
        ),
      }))
      return new Response(null, { status: 204 })
    }
  }

  const modelsId = ENDPOINT_MODELS.exec(pathname)?.[1]
  if (modelsId && method === 'GET') {
    return ok({
      models:
        queryString(ctx.url, 'native_api') === 'embeddings'
          ? ['text-embedding-3-large']
          : queryString(ctx.url, 'native_api') === 'chat'
            ? ['minimax-m2', 'minimax-m2.1']
            : ['minimax-m2', 'minimax-m2.1', 'text-embedding-3-large'],
    })
  }

  const testId = ENDPOINT_TEST.exec(pathname)?.[1]
  if (testId && method === 'POST') {
    return ok({
      duration_ms: 312,
      message: 'fixture upstream accepted the probe',
      model_count: 3,
      native_api: 'chat',
      native_api_source: 'provider',
      ok: true,
      status: 200,
    })
  }

  const tokenPlanId = ENDPOINT_TOKEN_PLAN.exec(pathname)?.[1]
  if (tokenPlanId && method === 'GET') {
    return ok({
      keys: [
        {
          balances: null,
          deepseek_balance: null,
          error_code: null,
          error_message: null,
          five_hour: {
            cap: 100,
            remaining_percent: 62,
            reset_at: '2026-10-23T17:00:00.000Z',
            used: 38,
            used_percent: 38,
          },
          glm_five_hour: null,
          glm_weekly: null,
          key_id: 'key-minimax-1',
          key_label: 'primary',
          model_remains: [
            {
              interval: {
                end_at: '2026-10-23T17:00:00.000Z',
                remaining_percent: 62,
                start_at: '2026-10-23T12:00:00.000Z',
                status: 1,
                total_count: 100,
                usage_count: 38,
              },
              model_name: 'minimax-m2',
              observation: {
                last_error_code: ctx.state.quotaStale
                  ? 'fixture_refresh_failed'
                  : null,
                next_retry_at: ctx.state.quotaStale
                  ? '2026-10-24T12:00:00.000Z'
                  : null,
                observed_at: '2026-10-23T12:00:00.000Z',
                refreshing: false,
                source: 'fixture',
                stale: ctx.state.quotaStale === true,
              },
              weekly: null,
              windows: [
                {
                  availability: 'known',
                  remaining_percent: 62,
                  reset_after_seconds: 18000,
                  reset_at: '2026-10-23T17:00:00.000Z',
                  source_window: 'primary',
                  used_percent: 38,
                  window_seconds: 18000,
                },
                {
                  availability: 'unknown',
                  remaining_percent: null,
                  reset_after_seconds: null,
                  reset_at: null,
                  source_window: 'secondary',
                  used_percent: null,
                  window_seconds: null,
                },
              ],
            },
          ],
          ok: true,
          opencodego_monthly: null,
          opencodego_rolling: null,
          opencodego_weekly: null,
          openrouter_balance: null,
          openrouter_spend: null,
          quota_status: {
            cooldown_until: null,
            observed_at: '2026-10-23T12:00:00.000Z',
            refresh_failed: false,
            scope: 'text_inference',
            state: 'available',
          },
          quota_windows: [
            {
              availability: 'known',
              current: 38,
              duration_seconds: 18000,
              is_default_scope: true,
              kind: 'five_hour',
              limit: 100,
              remaining: 62,
              remaining_percent: 62,
              reset_at: '2026-10-23T17:00:00.000Z',
              source_id: 'minimax:primary',
              source_model: 'minimax-m2',
              unit: 'requests',
            },
            {
              availability: 'unknown',
              current: null,
              duration_seconds: null,
              is_default_scope: true,
              kind: 'weekly',
              limit: null,
              remaining: null,
              remaining_percent: null,
              reset_at: null,
              source_id: 'minimax:weekly',
              source_model: 'minimax-m2',
              unit: null,
            },
          ],
          status: 200,
          weekly: {
            cap: 700,
            remaining_percent: 81,
            reset_at: '2026-10-27T00:00:00.000Z',
            used: 133,
            used_percent: 19,
          },
        },
      ],
      local_today_tokens: 4820,
      provider: 'minimax',
      provider_region: 'cn',
    })
  }

  const quotaHistoryId = ENDPOINT_QUOTA_HISTORY.exec(pathname)?.[1]
  if (quotaHistoryId && method === 'GET') {
    return ok({
      items: [
        {
          limit_reached: false,
          observed_at: '2026-10-23T12:00:00.000Z',
          plan_type: 'platform',
          snapshot_id: 'snapshot-1',
          source: 'fixture',
          windows: [
            {
              availability: 'known',
              remaining_percent: 62,
              reset_after_seconds: 18000,
              reset_at: '2026-10-23T17:00:00.000Z',
              source_window: 'primary',
              used_percent: 38,
              window_seconds: 18000,
            },
          ],
        },
      ],
      next_cursor: null,
      retention_days: 30,
    })
  }

  const quotaObsId = ENDPOINT_QUOTA_OBS.exec(pathname)?.[1]
  if (quotaObsId && method === 'GET') {
    return ok({
      items: [
        {
          error_code: null,
          key_id: 'key-minimax-1',
          key_label: 'primary',
          limit_reached: false,
          observation_id: 'observation-1',
          observed_at: '2026-10-23T12:00:00.000Z',
          ok: true,
          plan_type: 'platform',
          provider: 'minimax',
          source: 'fixture',
          status: '200',
          windows: [
            {
              availability: 'known',
              current: 38,
              duration_seconds: 18000,
              is_default_scope: true,
              kind: 'five_hour',
              limit: 100,
              remaining: 62,
              remaining_percent: 62,
              reset_at: '2026-10-23T17:00:00.000Z',
              source_id: 'minimax:primary',
              source_model: 'minimax-m2',
              unit: 'requests',
            },
          ],
        },
      ],
      next_cursor: null,
      retention_days: 30,
    })
  }

  const quotaTrendId = ENDPOINT_QUOTA_TREND.exec(pathname)?.[1]
  if (quotaTrendId && method === 'GET') {
    return ok({
      bucket_count: 2,
      bucket_seconds: 3600,
      failed_observations: 0,
      from: '2026-10-22T12:00:00.000Z',
      keys_truncated: false,
      range: queryString(ctx.url, 'range') ?? '24h',
      sampled_observations: 2,
      series: [
        {
          key_id: 'key-minimax-1',
          key_label: 'primary',
          kind: 'five_hour',
          metric: 'remaining_percent',
          points: [
            {
              at: '2026-10-22T12:00:00.000Z',
              observed_at: '2026-10-22T12:00:00.000Z',
              value: 74,
            },
            {
              at: '2026-10-23T12:00:00.000Z',
              observed_at: '2026-10-23T12:00:00.000Z',
              value: 62,
            },
          ],
          source_id: 'minimax:primary',
          unit: 'percent',
        },
      ],
      series_truncated: false,
      to: '2026-10-23T12:00:00.000Z',
    })
  }

  if (ROUTES.test(pathname)) {
    if (method === 'GET') {
      return ok(
        page(
          ctx.state.modelRoutes,
          queryInt(ctx.url, 'first', 0),
          queryInt(ctx.url, 'rows', 10),
          'routes',
        ),
      )
    }
    if (method === 'POST') {
      const body = await readJson<ModelRouteRequest>(ctx)
      const now = isoNow()
      const ruleId = nextId(ctx.state, 'rule')
      const targets = (body.targets ?? []).map((target, index) => {
        const endpoint = ctx.state.endpoints.find(
          (candidate) => candidate.endpoint_id === target.endpoint_id,
        )
        return {
          active_windows: target.active_windows ?? [],
          compact_mode:
            target.compact_mode === 'off'
              ? ('off' as const)
              : target.compact_mode === 'self_summarize'
                ? ('self_summarize' as const)
                : ('passthrough' as const),
          created_at: now,
          dev_system_normalize: target.dev_system_normalize === true,
          embedding_dimensions_override:
            target.embedding_dimensions_override ?? null,
          enabled: target.enabled !== false,
          endpoint_enabled: endpoint?.enabled ?? true,
          endpoint_id: target.endpoint_id,
          endpoint_name: endpoint?.name ?? null,
          has_proxy_url_override: Boolean(target.proxy_url_override),
          native_api: target.native_api ?? 'auto',
          position: index,
          rule_id: ruleId,
          service_tier: target.service_tier ?? null,
          target_id: nextId(ctx.state, 'target'),
          thinking_downgrade_enabled:
            target.thinking_downgrade_enabled === true,
          thinking_effort_override: target.thinking_effort_override ?? null,
          updated_at: now,
          upstream_model: target.upstream_model ?? null,
        }
      })
      ctx.state.modelRoutes.push({
        created_at: now,
        enabled: body.enabled !== false,
        model_pattern: body.model_pattern,
        owner_user_id: body.owner_user_id ?? null,
        routing_strategy: body.routing_strategy ?? 'responses_session_affinity',
        rule_id: ruleId,
        scope: body.scope,
        targets,
        updated_at: now,
      })
      return ok(ctx.state.modelRoutes[ctx.state.modelRoutes.length - 1])
    }
  }

  if (ROUTE_TEST.test(pathname) && method === 'POST') {
    const body = await readJson<{ rule_id: string }>(ctx)
    const rule = ctx.state.modelRoutes.find(
      (candidate) => candidate.rule_id === body.rule_id,
    )
    if (!rule) return error('not_found', 'fixture route not found', 404)
    const results = rule.targets.map((target, index) => {
      const endpoint = ctx.state.endpoints.find(
        (candidate) => candidate.endpoint_id === target.endpoint_id,
      )
      const ok = index % 3 !== 2
      return {
        duration_ms: 100 + index * 25,
        endpoint_id: target.endpoint_id,
        endpoint_name:
          target.endpoint_name ?? endpoint?.name ?? target.endpoint_id,
        message: ok
          ? `fixture probe passed for target ${index + 1}`
          : `fixture probe failed for target ${index + 1}`,
        model: target.upstream_model?.trim() || rule.model_pattern || '-',
        ok,
        preferred: index === 0,
        status: ok ? 200 : 503,
        target_id: target.target_id,
      }
    })
    const passed = results.filter((result) => result.ok).length
    const representative =
      results.find((result) => result.preferred) ?? results[0]
    return ok({
      duration_ms: results.reduce(
        (duration, result) => duration + result.duration_ms,
        0,
      ),
      endpoint_id: representative?.endpoint_id ?? null,
      endpoint_name: representative?.endpoint_name ?? null,
      message: representative?.message ?? 'No fixture targets',
      model: representative?.model ?? null,
      model_pattern: rule.model_pattern,
      ok: results.length > 0 && passed === results.length,
      passed,
      preferred_endpoint_id: representative?.endpoint_id ?? null,
      preferred_endpoint_name: representative?.endpoint_name ?? null,
      results,
      rule_id: rule.rule_id,
      status: representative?.status ?? null,
      total: results.length,
    })
  }

  const ruleId = ROUTE_ONE.exec(pathname)?.[1]
  if (ruleId) {
    const rule = ctx.state.modelRoutes.find(
      (candidate) => candidate.rule_id === ruleId,
    )
    if (!rule) return error('not_found', 'fixture route not found', 404)
    if (method === 'PATCH') {
      const body = await readJson<ModelRouteRequest>(ctx)
      rule.model_pattern = body.model_pattern
      rule.scope = body.scope
      rule.enabled = body.enabled !== false
      rule.owner_user_id = body.owner_user_id ?? null
      rule.routing_strategy = body.routing_strategy ?? rule.routing_strategy
      if (body.targets) {
        rule.targets = body.targets.map((target, index) => {
          const endpoint = ctx.state.endpoints.find(
            (candidate) => candidate.endpoint_id === target.endpoint_id,
          )
          return {
            active_windows: target.active_windows ?? [],
            compact_mode:
              target.compact_mode === 'off'
                ? ('off' as const)
                : target.compact_mode === 'self_summarize'
                  ? ('self_summarize' as const)
                  : ('passthrough' as const),
            created_at: isoNow(),
            dev_system_normalize: target.dev_system_normalize === true,
            embedding_dimensions_override:
              target.embedding_dimensions_override ?? null,
            enabled: target.enabled !== false,
            endpoint_enabled: endpoint?.enabled ?? true,
            endpoint_id: target.endpoint_id,
            endpoint_name: endpoint?.name ?? null,
            has_proxy_url_override: Boolean(target.proxy_url_override),
            native_api: target.native_api ?? 'auto',
            position: index,
            rule_id: ruleId,
            service_tier: target.service_tier ?? null,
            target_id: nextId(ctx.state, 'target'),
            thinking_downgrade_enabled:
              target.thinking_downgrade_enabled === true,
            thinking_effort_override: target.thinking_effort_override ?? null,
            updated_at: isoNow(),
            upstream_model: target.upstream_model ?? null,
          }
        })
      }
      rule.updated_at = isoNow()
      return ok(rule)
    }
    if (method === 'DELETE') {
      ctx.state.modelRoutes = ctx.state.modelRoutes.filter(
        (candidate) => candidate !== rule,
      )
      return new Response(null, { status: 204 })
    }
  }

  return null
}
