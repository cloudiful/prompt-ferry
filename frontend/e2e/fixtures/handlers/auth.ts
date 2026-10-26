import { isIP } from 'node:net'
import type {
  HostRole,
  LoginRequest,
  RelayHostRoleRequest,
  RelayLoginRequest,
  RelaySettingsUpdate,
} from '../../../src/generated/admin-api'
import {
  currentAccount,
  error,
  ok,
  readJson,
  requireRelay,
  sessionCookieValue,
  unauthorized,
  type FixtureContext,
} from '../http'
import {
  FIXTURE_PASSWORD,
  FIXTURE_RELAY_COOKIE,
  FIXTURE_RELAY_TOKEN,
} from '../state'

function sessionSetCookie(
  state: FixtureContext['state'],
  userId: number,
): string {
  return `pf_fixture_session=${encodeURIComponent(
    sessionCookieValue(state, userId),
  )}; Path=/; HttpOnly; SameSite=Lax`
}

function relaySetCookie(state: FixtureContext['state']): string {
  return `pf_fixture_relay=${encodeURIComponent(
    state.relayToken,
  )}; Path=/; HttpOnly; SameSite=Lax`
}

function isLoopbackAdminBind(value: string): boolean {
  const match = /^(?:\[([^\]]+)\]|([^:]+)):(\d+)$/.exec(value.trim())
  if (!match) return false
  const host = match[1] ?? match[2]
  const port = Number(match[3])
  if (!host || !Number.isInteger(port) || port < 0 || port > 65535) {
    return false
  }
  if (isIP(host) === 4) return Number(host.split('.')[0]) === 127
  if (isIP(host) !== 6) return false
  try {
    return new URL(`http://[${host}]/`).hostname.toLowerCase() === '[::1]'
  } catch {
    return false
  }
}

function syncRelayHostState(ctx: FixtureContext): void {
  const { relayRunningRole, relaySavedAdminBind, relaySavedRole } = ctx.state
  const role = relaySavedRole ?? relayRunningRole
  const pendingRole =
    relaySavedRole !== null && relaySavedRole !== relayRunningRole
      ? relaySavedRole
      : null
  const restartRequired =
    pendingRole !== null ||
    (relaySavedAdminBind !== null &&
      relaySavedAdminBind !== ctx.state.relayConfig.admin_bind)
  ctx.state.relayHost = {
    pending_role: pendingRole,
    restart_required: restartRequired,
    role,
  }
  ctx.state.relayStatus.role = role
  ctx.state.relayStatus.restart_required = restartRequired
}

export async function handleAuth(
  ctx: FixtureContext,
): Promise<Response | null> {
  const { method } = ctx.request
  const { pathname } = ctx.url

  if (method === 'POST' && pathname === '/api/v1/auth/login') {
    const body = await readJson<LoginRequest>(ctx)
    const account = ctx.state.accounts.find(
      (candidate) =>
        candidate.user.login_name === body.login_name?.trim() &&
        candidate.password === body.password,
    )
    if (!account || !account.user.is_active) {
      return error('invalid_credentials', 'invalid fixture credentials', 401)
    }
    return new Response(null, {
      headers: {
        'set-cookie': sessionSetCookie(ctx.state, account.user.user_id),
      },
      status: 204,
    })
  }

  if (method === 'POST' && pathname === '/api/v1/auth/logout') {
    return new Response(null, {
      headers: { 'set-cookie': 'pf_fixture_session=; Path=/; Max-Age=0' },
      status: 204,
    })
  }

  if (method === 'GET' && pathname === '/api/v1/auth/me') {
    const account = currentAccount(ctx.state, ctx.cookies)
    if (!account) return unauthorized()
    return ok({
      display_name: account.user.display_name,
      is_admin: account.user.is_admin,
      login_name: account.user.login_name,
      user_id: account.user.user_id,
    })
  }

  if (method === 'GET' && pathname === '/api/v1/bridge/status') {
    if (!currentAccount(ctx.state, ctx.cookies)) return unauthorized()
    return ok(ctx.state.bridge)
  }

  if (method === 'POST' && pathname === '/api/v1/relay/auth/login') {
    const body = await readJson<RelayLoginRequest>(ctx)
    if (body.admin_token !== FIXTURE_RELAY_TOKEN) {
      return error('unauthorized', 'invalid fixture relay token', 401)
    }
    return new Response(null, {
      headers: { 'set-cookie': relaySetCookie(ctx.state) },
      status: 204,
    })
  }

  if (method === 'POST' && pathname === '/api/v1/relay/auth/logout') {
    return new Response(null, {
      headers: { 'set-cookie': `${FIXTURE_RELAY_COOKIE}=; Path=/; Max-Age=0` },
      status: 204,
    })
  }

  if (method === 'GET' && pathname === '/api/v1/relay/auth/me') {
    return ok({ authenticated: ctx.relayAuthenticated })
  }

  if (pathname.startsWith('/api/v1/relay/')) {
    const missing = requireRelay(ctx)
    if (missing) return missing
  }

  if (method === 'GET' && pathname === '/api/v1/relay/host') {
    syncRelayHostState(ctx)
    return ok(ctx.state.relayHost)
  }

  if (method === 'GET' && pathname === '/api/v1/relay/status') {
    syncRelayHostState(ctx)
    return ok(ctx.state.relayStatus)
  }

  if (method === 'GET' && pathname === '/api/v1/relay/settings') {
    syncRelayHostState(ctx)
    return ok({
      relay: ctx.state.relayConfig,
      restart_required: ctx.state.relayHost.restart_required,
    })
  }

  if (method === 'PATCH' && pathname === '/api/v1/relay/settings') {
    const body = await readJson<RelaySettingsUpdate>(ctx)
    if (Object.hasOwn(body, 'admin_bind')) {
      if (
        typeof body.admin_bind !== 'string' ||
        (body.admin_bind.trim() !== '' &&
          !isLoopbackAdminBind(body.admin_bind.trim()))
      ) {
        return error(
          'invalid_admin_bind',
          'the relay management bind must use a loopback address',
          400,
        )
      }
      ctx.state.relaySavedAdminBind = body.admin_bind.trim()
    }
    syncRelayHostState(ctx)
    return ok({
      relay: ctx.state.relayConfig,
      restart_required: ctx.state.relayHost.restart_required,
    })
  }

  if (method === 'PUT' && pathname === '/api/v1/relay/host/role') {
    const body = await readJson<RelayHostRoleRequest>(ctx)
    const role: HostRole = body.role
    if (!['integrated', 'worker', 'relay'].includes(role)) {
      return error('invalid_role', 'unsupported host role', 400)
    }
    ctx.state.relaySavedRole = role
    syncRelayHostState(ctx)
    return ok(ctx.state.relayHost)
  }

  if (method === 'POST' && pathname === '/api/v1/relay/host/restart') {
    return ok(
      {
        message: 'this host is stopping so the saved settings take effect',
        restart_requested: true,
      },
      202,
    )
  }

  return null
}

export { FIXTURE_PASSWORD }
