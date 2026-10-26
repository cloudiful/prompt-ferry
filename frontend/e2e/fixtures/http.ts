import type { FixtureAccount, FixtureState } from './state'

export type FixtureContext = {
  request: Request
  url: URL
  state: FixtureState
  cookies: Record<string, string>
  body: () => Promise<unknown>
  account: FixtureAccount | null
  relayAuthenticated: boolean
}

export const JSON_HEADERS = {
  'content-type': 'application/json; charset=utf-8',
}

export function ok(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    headers: JSON_HEADERS,
    status,
  })
}

export function error(code: string, message: string, status = 400): Response {
  return new Response(JSON.stringify({ error: { code, message } }), {
    headers: JSON_HEADERS,
    status,
  })
}

export function notFound(): Response {
  return error('not_found', 'fixture does not implement this endpoint', 404)
}

export function unauthorized(): Response {
  return error('unauthorized', 'fixture session is missing or expired', 401)
}

export function forbidden(): Response {
  return error('forbidden', 'fixture account lacks admin scope', 403)
}

export function page<T>(
  items: T[],
  first: number,
  rows: number,
  key: string,
): Record<string, unknown> {
  const start = Math.max(0, first)
  const size = Math.max(1, rows)
  return {
    [key]: items.slice(start, start + size),
    first: start,
    rows: size,
    total: items.length,
  }
}

export function queryInt(url: URL, name: string, fallback: number): number {
  const raw = url.searchParams.get(name)
  if (raw === null) return fallback
  const parsed = Number.parseInt(raw, 10)
  return Number.isFinite(parsed) ? parsed : fallback
}

export function queryString(url: URL, name: string): string | undefined {
  const value = url.searchParams.get(name)
  return value === null || value === '' ? undefined : value
}

export async function readJson<T>(ctx: FixtureContext): Promise<T> {
  return (await ctx.body()) as T
}

export function requireAdmin(ctx: FixtureContext): Response | null {
  if (!ctx.account) return unauthorized()
  if (!ctx.account.isAdmin) return forbidden()
  return null
}

export function requireRelay(ctx: FixtureContext): Response | null {
  if (!ctx.relayAuthenticated) return unauthorized()
  return null
}

export function parseCookies(header: string | null): Record<string, string> {
  const result: Record<string, string> = {}
  if (!header) return result
  for (const part of header.split(';')) {
    const index = part.indexOf('=')
    if (index <= 0) continue
    const name = part.slice(0, index).trim()
    const value = part.slice(index + 1).trim()
    if (name) result[name] = decodeURIComponent(value)
  }
  return result
}

export function sessionCookieValue(
  state: FixtureState,
  userId: number,
): string {
  return `${state.sessionToken}:${userId}`
}

export function currentAccount(
  state: FixtureState,
  cookies: Record<string, string>,
): FixtureAccount | null {
  const raw = cookies.pf_fixture_session
  if (!raw) return null
  const [token, userId] = raw.split(':')
  if (token !== state.sessionToken) return null
  const parsed = Number.parseInt(userId ?? '', 10)
  return (
    state.accounts.find((account) => account.user.user_id === parsed) ?? null
  )
}

export function accountForLogin(
  state: FixtureState,
  login: string,
): FixtureAccount | null {
  return (
    state.accounts.find((account) => account.user.login_name === login) ?? null
  )
}

export function nextId(state: FixtureState, prefix: string): string {
  state.nextId += 1
  return `${prefix}-${state.nextId}`
}

export function isoNow(): string {
  return new Date().toISOString()
}
