import {
  currentAccount,
  error,
  ok,
  parseCookies,
  type FixtureContext,
} from './http'
import { createState, type FixtureState } from './state'
import { handleAuth } from './handlers/auth'
import { handleCatalog } from './handlers/catalog'
import { handleOAuth } from './handlers/oauth'
import { handleMcpRelays } from './handlers/mcp-relays'
import { handleRecords } from './handlers/records'
import { handleMoneySettings } from './handlers/money-settings'
import { summarizeSyntheticArchiveRequest } from './config-archive'

declare const Bun: {
  serve: (options: {
    port: number
    hostname: string
    fetch: (request: Request) => Promise<Response>
  }) => { port: number; stop: () => void }
}

declare const process: {
  env: Record<string, string | undefined>
  on: (event: string, listener: () => void) => void
  exit: (code: number) => void
}

type Fault = {
  pathPrefix: string
  method?: string
  status?: number
  code?: string
  message?: string
  delayMs?: number
  count?: number | null
}

type RequestLog = {
  query?: Record<string, string>
  body?: unknown
  at: string
  method: string
  path: string
  status: number
  ms: number
}

const port = Number.parseInt(process.env.FIXTURE_PORT ?? '8789', 10)
let state: FixtureState = createState()
let faults: Fault[] = []
const requestLog: RequestLog[] = []

const CONTROL_HEADERS = {
  'access-control-allow-origin': '*',
  'access-control-allow-headers': 'content-type',
  'access-control-allow-methods': 'GET, POST, DELETE, OPTIONS',
  'content-type': 'application/json; charset=utf-8',
}

function control(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    headers: CONTROL_HEADERS,
    status,
  })
}

async function handleControl(
  request: Request,
  url: URL,
): Promise<Response | null> {
  if (!url.pathname.startsWith('/__fixtures/')) return null
  if (request.method === 'OPTIONS') {
    return new Response(null, { headers: CONTROL_HEADERS, status: 204 })
  }

  if (url.pathname === '/__fixtures/reset' && request.method === 'POST') {
    state = createState()
    faults = []
    requestLog.length = 0
    return control({ reset: true })
  }

  if (url.pathname === '/__fixtures/faults') {
    if (request.method === 'POST') {
      const fault = (await request.json()) as Fault
      if (!fault.pathPrefix) {
        return control({ error: 'pathPrefix is required' }, 400)
      }
      faults.push({ count: 1, ...fault })
      return control({ faults })
    }
    if (request.method === 'DELETE') {
      faults = []
      return control({ faults })
    }
    if (request.method === 'GET') {
      return control({ faults })
    }
  }

  if (url.pathname === '/__fixtures/log' && request.method === 'GET') {
    const limit = Number.parseInt(url.searchParams.get('limit') ?? '50', 10)
    return control({ requests: requestLog.slice(-limit) })
  }

  if (url.pathname === '/__fixtures/summary' && request.method === 'GET') {
    return control({
      endpoints: state.endpoints.length,
      modelRoutes: state.modelRoutes.length,
      mcpServers: state.mcpServers.length,
      relays: state.relays.length,
      aiRecords: state.aiRecords.length,
      mcpRecords: state.mcpRecords.length,
      approvals: state.approvals.length,
      charges: state.billingCharges.length,
      requests: requestLog.length,
    })
  }

  if (url.pathname === '/__fixtures/quota' && request.method === 'POST') {
    const body = (await request.json()) as { stale?: boolean }
    state.quotaStale = body.stale === true
    return control({ stale: state.quotaStale })
  }

  if (url.pathname === '/__fixtures/overview' && request.method === 'POST') {
    const body = (await request.json()) as {
      category?: 'ai' | 'mcp'
      empty?: boolean
      unknownTokens?: boolean
    }
    const category = body.category === 'mcp' ? 'mcp' : 'ai'
    const { buildOverview } = await import('./state')
    const overview = buildOverview(category, {
      empty: body.empty === true,
      unknownTokens: body.unknownTokens === true,
    })
    if (category === 'mcp') state.mcpOverview = overview
    else state.aiOverview = overview
    return control({ category, empty: body.empty === true })
  }

  return control({ error: 'unknown fixture control endpoint' }, 404)
}

function matchingFault(method: string, path: string): Fault | null {
  return (
    faults.find(
      (fault) =>
        path.startsWith(fault.pathPrefix) &&
        (!fault.method || fault.method === method) &&
        (fault.count === null || (fault.count ?? 0) > 0),
    ) ?? null
  )
}

async function dispatch(request: Request): Promise<{
  response: Response
  path: string
}> {
  const url = new URL(request.url)
  const method = request.method

  const controlResponse = await handleControl(request, url)
  if (controlResponse) return { response: controlResponse, path: url.pathname }

  if (!url.pathname.startsWith('/api/')) {
    return {
      response: new Response('fixtures serve /api only', { status: 404 }),
      path: url.pathname,
    }
  }

  const fault = matchingFault(method, url.pathname)
  if (fault) {
    if (fault.count !== null) fault.count = Math.max(0, (fault.count ?? 0) - 1)
    if (fault.delayMs) {
      await new Promise((resolve) => setTimeout(resolve, fault.delayMs))
    }
    if ((fault.status ?? 500) >= 400) {
      return {
        response: error(
          fault.code ?? 'fixture_injected_error',
          fault.message ?? 'fixture injected failure',
          fault.status ?? 500,
        ),
        path: url.pathname,
      }
    }
  }

  const cookies = parseCookies(request.headers.get('cookie'))
  const account = currentAccount(state, cookies)
  let bodyPromise: Promise<unknown> | null = null
  const ctx: FixtureContext = {
    request,
    url,
    state,
    cookies,
    body: () => {
      bodyPromise ??= request
        .clone()
        .json()
        .catch(() => ({}))
      return bodyPromise
    },
    account,
    relayAuthenticated: cookies.pf_fixture_relay === state.relayToken,
  }

  const handler = await handleAuth(ctx)
  if (handler) return { response: handler, path: url.pathname }

  const oauth = await handleOAuth(ctx)
  if (oauth) return { response: oauth, path: url.pathname }

  const catalog = await handleCatalog(ctx)
  if (catalog) return { response: catalog, path: url.pathname }

  const mcpRelays = await handleMcpRelays(ctx)
  if (mcpRelays) return { response: mcpRelays, path: url.pathname }

  const records = await handleRecords(ctx)
  if (records) return { response: records, path: url.pathname }

  const moneySettings = await handleMoneySettings(ctx)
  if (moneySettings) return { response: moneySettings, path: url.pathname }

  return {
    response: error(
      'unmocked_endpoint',
      `fixture does not implement ${method} ${url.pathname}`,
      404,
    ),
    path: url.pathname,
  }
}

const server = Bun.serve({
  hostname: '127.0.0.1',
  port,
  fetch: async (request) => {
    const started = performance.now()
    const url = new URL(request.url)
    // The archive routes carry a passphrase and an archive body, so their
    // logged shape is a summary: presence flags and lengths, never the value.
    const archiveRequest =
      url.pathname === '/api/v1/admin/config-export' ||
      url.pathname.startsWith('/api/v1/admin/config-import')
    const recordBody =
      archiveRequest ||
      url.pathname === '/api/v1/settings/redaction' ||
      /^\/api\/v1\/admin\/(?:model-routes|endpoints)(?:\/[^/]+)?$/.test(
        url.pathname,
      )
    const body =
      recordBody && request.method !== 'GET'
        ? await request
            .clone()
            .json()
            .catch(() => null)
        : undefined
    const { response, path } = await dispatch(request)
    const ms = Math.round(performance.now() - started)
    requestLog.push({
      at: new Date().toISOString(),
      method: request.method,
      path,
      query: Object.fromEntries(url.searchParams),
      body: archiveRequest
        ? summarizeSyntheticArchiveRequest(body)
        : (url.pathname.includes('/model-routes') ||
              url.pathname.includes('/endpoints')) &&
            body
          ? JSON.parse(
              JSON.stringify(body, (key, value) =>
                [
                  'api_key',
                  'admin_api_key',
                  'proxy_url',
                  'proxy_url_override',
                ].includes(key) && value
                  ? '[synthetic-secret-present]'
                  : value,
              ),
            )
          : body,
      status: response.status,
      ms,
    })
    if (requestLog.length > 500) requestLog.shift()
    if (!path.startsWith('/__fixtures/')) {
      console.log(
        `[fixture] ${request.method} ${path} -> ${response.status} ${ms}ms`,
      )
    }
    return response
  },
})

console.log(
  `[fixture] prompt-ferry stateful API fixture listening on http://127.0.0.1:${server.port}`,
)

for (const signal of ['SIGINT', 'SIGTERM'] as const) {
  process.on(signal as never, () => {
    server.stop()
    process.exit(0)
  })
}

export { handleControl, dispatch, ok }
