import type {
  EndpointOAuthStatusResponse,
  EndpointPlan,
  EndpointProvider,
} from '@/generated/admin-api'
import type { EndpointForm } from '@/models'
import type { AsyncActionOutcome } from '@/lib/async-action'

export type RunOAuthAction = <T>(
  endpointId: string,
  action: () => Promise<T>,
  label: string,
) => Promise<AsyncActionOutcome<T>>

export type OAuthStatusSnapshot = {
  endpointId: string
  dialogGeneration: number
  provider: EndpointProvider
  plan: EndpointPlan
  has_oauth_token: boolean
  userPlanRevision: number
}

export type OAuthStatusContext = Pick<
  OAuthStatusSnapshot,
  'endpointId' | 'dialogGeneration' | 'userPlanRevision'
>

export type OAuthStatusValue = Pick<
  EndpointOAuthStatusResponse,
  'has_oauth_token' | 'plan'
>

export function applyOAuthStatusToForm(
  form: EndpointForm,
  snapshot: OAuthStatusSnapshot,
  status: OAuthStatusValue,
  current: OAuthStatusContext,
): EndpointForm | null {
  if (
    snapshot.provider !== 'openai' ||
    form.provider !== 'openai' ||
    current.endpointId !== snapshot.endpointId ||
    current.dialogGeneration !== snapshot.dialogGeneration
  ) {
    return null
  }

  let plan = form.plan
  if (!status.has_oauth_token) {
    plan = 'platform_api_key'
  } else if (
    !snapshot.has_oauth_token &&
    current.userPlanRevision === snapshot.userPlanRevision &&
    form.plan === snapshot.plan
  ) {
    plan = status.plan
  }

  if (form.has_oauth_token === status.has_oauth_token && form.plan === plan) {
    return form
  }

  return { ...form, has_oauth_token: status.has_oauth_token, plan }
}

function parameterValues(value: string): string[] {
  const query = value.replace(/^[?#]/, '')
  return query.split('&').flatMap((pair) => {
    const separator = pair.indexOf('=')
    if (separator < 0) return [pair]
    const raw = pair.slice(separator + 1)
    if (!raw) return []
    try {
      return [raw, decodeURIComponent(raw.replace(/\+/g, ' '))]
    } catch {
      return [raw]
    }
  })
}

function oauthUrlValues(value: string): string[] {
  try {
    const url = new URL(value, 'http://oauth.invalid')
    return [
      value,
      url.href,
      ...Array.from(url.searchParams.values()),
      ...parameterValues(url.search),
      ...parameterValues(url.hash),
    ].filter(Boolean)
  } catch {
    return [value]
  }
}

export function sanitizeOAuthErrorMessage(
  message: string,
  sensitiveValues: readonly string[] = [],
  fallback = 'Request failed',
): string {
  let safe = message
  const values = new Set<string>()
  for (const value of sensitiveValues) {
    if (!value) continue
    for (const candidate of oauthUrlValues(value)) {
      values.add(candidate)
      values.add(encodeURIComponent(candidate))
    }
  }
  for (const candidate of [...values].sort((a, b) => b.length - a.length)) {
    safe = safe.split(candidate).join('[redacted]')
  }
  safe = safe.replace(
    /\b(code|state|authorization_code|auth_code)=([^&\s"'<>]+)/gi,
    '$1=[redacted]',
  )
  safe = safe.replace(/https?:\/\/[^\s<>"']+/gi, '[URL]')
  return safe.trim() || fallback
}
