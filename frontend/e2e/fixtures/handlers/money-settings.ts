import type {
  ApprovalRequest,
  BillingPriceRuleRequest,
  CacheAlertSettingsRequest,
  CacheAlertSettingsResponse,
  ConfigExportRequest,
  ConfigImportApplied,
  ConfigImportPreview,
  ConfigImportRequest,
  CustomStringRuleSchema,
  LlmReviewSettings,
  LlmReviewWebhookSettings,
  RawObjectStoreSecretPatch,
  RawObjectStoreSettingsRequest,
  RedactionConfigSchema,
  RedactionCustomStringRuleRowSchema,
  RedactionPreviewRequestSchema,
  RedactionScopeSchema,
  RelayIpPolicy,
  RequestContentLoggingRequest,
  StreamDeltaBatchingSettings,
  UsageRetentionSettings,
} from '../../../src/generated/admin-api'
import { isIP } from 'node:net'
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
import type { FixtureState } from '../state'
import {
  SYNTHETIC_ARCHIVE_BACKEND,
  decodeSyntheticArchive,
  decodeSyntheticArchiveBase64,
  diffSyntheticArchive,
  encodeSyntheticArchive,
  syntheticArchiveWarnings,
  type DecodedSyntheticArchive,
  type SyntheticArchiveDomain,
  type SyntheticArchiveFailure,
} from '../config-archive'
import {
  billingDetailsCsv,
  billingMonthlyCsv,
  chargeSummary,
  filteredCharges,
  sortedPriceRules,
} from '../billing'
import {
  approvalWaitDeadline,
  sortPendingApprovals,
  sortResolvedApprovals,
} from '../state'

const APPROVALS = /^\/api\/v1\/admin\/approvals$/
const APPROVAL_ONE = /^\/api\/v1\/admin\/approvals\/([^/]+)$/
const APPROVAL_DECIDE =
  /^\/api\/v1\/admin\/approvals\/([^/]+)\/(approve|reject)$/
const CHARGES = /^\/api\/v1\/admin\/billing\/charges$/
const CHARGE_ONE = /^\/api\/v1\/admin\/billing\/charges\/([^/]+)$/
const BILLING_EXPORT = /^\/api\/v1\/admin\/billing\/export$/
const PRICE_RULES = /^\/api\/v1\/admin\/billing\/price-rules$/
const PRICE_RULE_ONE = /^\/api\/v1\/admin\/billing\/price-rules\/([^/]+)$/
const REPRICE = /^\/api\/v1\/admin\/billing\/reprice-unpriced$/
const BILLING_SUMMARY = /^\/api\/v1\/admin\/billing\/summary$/
const CONFIG_AUDIT = /^\/api\/v1\/admin\/config-audit$/
const CONFIG_EXPORT = /^\/api\/v1\/admin\/config-export$/
const CONFIG_EXPORT_METADATA = /^\/api\/v1\/admin\/config-export\/metadata$/
const CONFIG_IMPORT = /^\/api\/v1\/admin\/config-import$/
const CONFIG_IMPORT_PREVIEW = /^\/api\/v1\/admin\/config-import\/preview$/

/** `normalize_prefix` from the raw payload store: trim, strip slashes, default. */
function normalizeStoragePrefix(prefix: string): string {
  const trimmed = prefix.trim().replace(/^\/+|\/+$/g, '')
  return trimmed === '' ? 'prompt-ferry/raw' : trimmed
}

/** An empty region becomes `auto` before validation runs. */
function normalizeStorageRegion(region: string): string {
  const trimmed = region.trim()
  return trimmed === '' ? 'auto' : trimmed
}

/**
 * The keep/replace/clear union from `resolve_raw_secret_patch`. A missing patch
 * or `keep` retains the stored value, `clear` removes it, and `replace` stores
 * the trimmed entry and rejects an empty one.
 */
function resolveStorageSecret(
  existing: string | null,
  patch: RawObjectStoreSecretPatch | null | undefined,
): { value: string | null } | { failure: string } {
  if (!patch || patch.mode === 'keep') return { value: existing }
  if (patch.mode === 'clear') return { value: null }
  const trimmed = String(patch.value ?? '').trim()
  if (trimmed === '') return { failure: 'secret value cannot be empty' }
  return { value: trimmed }
}

/**
 * The per-domain record counts the fixture exports and compares against.
 *
 * Counts and the two unrecoverable-secret flags only: no configuration value,
 * no credential and no secret ever enters the archive payload.
 */
function fixtureArchiveDomains(state: FixtureState): SyntheticArchiveDomain[] {
  return [
    {
      name: 'endpoints',
      records: state.endpoints.length,
      unrecoverable_secrets: 1,
    },
    {
      name: 'model_routes',
      records: state.modelRoutes.length,
      unrecoverable_secrets: 0,
    },
    {
      name: 'mcp_servers',
      records: state.mcpServers.length,
      unrecoverable_secrets: 1,
    },
  ]
}

/**
 * The next audit timestamp, one second after the previous one. Seed rows and
 * new rows therefore stay strictly ordered without depending on the clock.
 */
function nextArchiveTimestamp(state: FixtureState): string {
  const at = new Date(state.configAuditClockMs).toISOString()
  state.configAuditClockMs += 1000
  return at
}

type ArchiveAuditInput = {
  action: 'export' | 'import'
  archiveBytes?: number | null
  payloadFingerprint?: string | null
  domains?: Array<{
    name: string
    records: number
    unrecoverable_secrets: number
  }>
  error?: SyntheticArchiveFailure
  success: boolean
}

/**
 * One audit row per export or import attempt, newest first. The row carries
 * metadata only: the passphrase and the archive body never reach it.
 */
function recordArchiveAudit(ctx: FixtureContext, input: ArchiveAuditInput) {
  ctx.state.configAudit.unshift({
    action: input.action,
    actor_login_name: ctx.account?.user.login_name,
    actor_user_id: ctx.account?.user.user_id,
    archive_bytes: input.archiveBytes ?? null,
    audit_id: ++ctx.state.nextId,
    backend_kind: SYNTHETIC_ARCHIVE_BACKEND,
    created_at: nextArchiveTimestamp(ctx.state),
    domains: input.domains ?? [],
    error_code: input.error?.code ?? null,
    error_message: input.error?.message ?? null,
    format_version: input.success ? 1 : null,
    payload_fingerprint: input.payloadFingerprint ?? null,
    success: input.success,
  })
}

/** Open the request's archive, mirroring the import layer's base64 step. */
function decodeArchiveBytes(
  ctx: FixtureContext,
  body: Partial<ConfigImportRequest> | null,
  bytes: Uint8Array,
):
  | { ok: true; value: DecodedSyntheticArchive }
  | { ok: false; error: SyntheticArchiveFailure } {
  const passphrase = typeof body?.passphrase === 'string' ? body.passphrase : ''
  return decodeSyntheticArchive(passphrase, bytes, {
    expectedBackend: SYNTHETIC_ARCHIVE_BACKEND,
  })
}

/** Decode a base64 request body and then open it. */
function decodeArchiveRequest(
  ctx: FixtureContext,
  body: Partial<ConfigImportRequest> | null,
):
  | { ok: true; value: DecodedSyntheticArchive }
  | { ok: false; error: SyntheticArchiveFailure } {
  const archiveBase64 =
    typeof body?.archive_base64 === 'string' ? body.archive_base64 : ''
  const bytes = decodeSyntheticArchiveBase64(archiveBase64)
  if (!bytes.ok) return bytes
  return decodeArchiveBytes(ctx, body, bytes.value)
}

/** Backend serde defaults for the cache alert policy (`cache_alert.rs`). */
const CACHE_ALERT_DEFAULT = {
  enabled: false,
  window_minutes: 30,
  min_turns: 5,
  threshold: 0.2,
  cooldown_minutes: 60,
} as const

/** The SMTP channel's own serde defaults. */
const CACHE_ALERT_SMTP_DEFAULT = {
  enabled: false,
  host: '',
  port: 465,
  tls: 'implicit_tls',
  from: '',
  to: [] as string[],
  username: '',
  password: '',
  has_smtp_password: false,
} as const

/**
 * The backend's `CacheAlertSettings::validate`, message for message. The four
 * numeric fields are bounds-checked; the threshold is also required to be finite
 * so a non-numeric entry never reaches the stored record.
 */
function validateCacheAlert(settings: {
  window_minutes: number
  min_turns: number
  threshold: number
  cooldown_minutes: number
}): string | null {
  if (
    !Number.isInteger(settings.window_minutes) ||
    settings.window_minutes < 5 ||
    settings.window_minutes > 1440
  ) {
    return 'window_minutes must be between 5 and 1440'
  }
  if (
    !Number.isInteger(settings.min_turns) ||
    settings.min_turns < 2 ||
    settings.min_turns > 100
  ) {
    return 'min_turns must be between 2 and 100'
  }
  if (
    !Number.isFinite(settings.threshold) ||
    settings.threshold < 0 ||
    settings.threshold > 1
  ) {
    return 'threshold must be between 0 and 1'
  }
  if (
    !Number.isInteger(settings.cooldown_minutes) ||
    settings.cooldown_minutes < 5 ||
    settings.cooldown_minutes > 1440
  ) {
    return 'cooldown_minutes must be between 5 and 1440'
  }
  return null
}

/**
 * The redacted admin view: the stored secret never leaves the fixture state.
 */
function cacheAlertResponse(state: {
  settings: { cacheAlert: CacheAlertSettingsResponse }
  privateCacheAlertSecret: string
  privateCacheAlertSmtpPassword: string
}): CacheAlertSettingsResponse {
  return {
    ...state.settings.cacheAlert,
    dingtalk_secret: '',
    has_dingtalk_secret: state.privateCacheAlertSecret.trim() !== '',
    smtp: {
      ...CACHE_ALERT_SMTP_DEFAULT,
      ...(state.settings.cacheAlert.smtp ?? {}),
      password: '',
      has_smtp_password: state.privateCacheAlertSmtpPassword.trim() !== '',
    },
  }
}

/**
 * The backend's host rule: a name or an address, never a scheme or a path.
 */
function validCacheAlertSmtpHost(value: string): boolean {
  const trimmed = value.trim()
  if (trimmed === '' || trimmed.length > 253) return false
  if (
    trimmed.includes('://') ||
    trimmed.includes('/') ||
    trimmed.includes('@')
  ) {
    return false
  }
  return !/[\s\p{Cc}]/u.test(trimmed)
}

/**
 * The backend's address rule: one `@`, a dotted domain and no whitespace.
 */
function validCacheAlertEmail(value: string): boolean {
  const trimmed = value.trim()
  if (trimmed === '' || trimmed.length > 254) return false
  if (/[\s\p{Cc}]/u.test(trimmed)) return false
  const separator = trimmed.lastIndexOf('@')
  if (separator <= 0) return false
  const local = trimmed.slice(0, separator)
  const domain = trimmed.slice(separator + 1)
  if (local.length === 0 || local.length > 64 || local.includes('@'))
    return false
  if (
    domain.length === 0 ||
    !domain.includes('.') ||
    domain.startsWith('.') ||
    domain.endsWith('.') ||
    domain.includes('..')
  ) {
    return false
  }
  return /^[A-Za-z0-9.-]+$/.test(domain)
}

/**
 * The backend's `CacheAlertSmtpSettings::validate`, message for message. The
 * credentials stay paired, the host and the addresses are checked whenever
 * they carry a value, and the required ones only while the channel is on.
 */
function validateCacheAlertSmtp(smtp: {
  enabled: boolean
  host: string
  port: number
  from: string
  to: string[]
  username: string
  password: string
}): string | null {
  if (!Number.isInteger(smtp.port) || smtp.port < 1 || smtp.port > 65535) {
    return 'smtp.port must be between 1 and 65535'
  }
  const hasUsername = smtp.username.trim() !== ''
  const hasPassword = smtp.password.trim() !== ''
  if (hasUsername !== hasPassword) {
    return 'smtp.username and smtp.password must be set together'
  }
  if (smtp.host.trim() !== '' && !validCacheAlertSmtpHost(smtp.host)) {
    return 'smtp.host must be a host name or address without a scheme'
  }
  if (smtp.from.trim() !== '' && !validCacheAlertEmail(smtp.from)) {
    return 'smtp.from must be a valid email address'
  }
  if (!smtp.to.every((recipient) => validCacheAlertEmail(recipient))) {
    return 'smtp.to must contain only valid email addresses'
  }
  if (smtp.enabled) {
    if (smtp.host.trim() === '') {
      return 'smtp.host is required when the SMTP channel is enabled'
    }
    if (smtp.from.trim() === '') {
      return 'smtp.from is required when the SMTP channel is enabled'
    }
    if (smtp.to.length === 0) {
      return 'smtp.to must contain at least one email address'
    }
  }
  return null
}

/** Trim, drop empties and dedupe in order, as `ip_acl::normalize_entries` does. */
function normalizeRelayEntries(entries: unknown): string[] {
  const list = Array.isArray(entries) ? entries : []
  const seen = new Set<string>()
  const normalized: string[] = []
  for (const entry of list) {
    const text = String(entry).trim()
    if (text === '' || seen.has(text)) continue
    seen.add(text)
    normalized.push(text)
  }
  return normalized
}

/**
 * Whether an entry is an IP or a CIDR with a valid prefix, matching
 * `ip_acl::parse_entry`: a bare IP or `address/prefix` where the prefix fits the
 * address family.
 */
function isRelayEntry(entry: string): boolean {
  const slash = entry.indexOf('/')
  if (slash < 0) return isIP(entry) !== 0
  const address = entry.slice(0, slash)
  const prefix = entry.slice(slash + 1)
  const family = isIP(address)
  if (family === 0) return false
  if (!/^(0|[1-9]\d{0,2})$/.test(prefix)) return false
  return Number(prefix) <= (family === 4 ? 32 : 128)
}

/** The server default, with the request's own fields over it. */
function normalizeLlmReview(body: Record<string, unknown>): LlmReviewSettings {
  const webhook = (body.webhook ?? {}) as Partial<LlmReviewWebhookSettings>
  return {
    enabled: body.enabled === true,
    review_base_url: String(body.review_base_url ?? ''),
    review_api_key: String(body.review_api_key ?? ''),
    review_model: String(body.review_model ?? ''),
    review_timeout_ms: Number(body.review_timeout_ms ?? 3000),
    failure_policy:
      body.failure_policy === 'fail_open' ? 'fail_open' : 'fail_closed',
    pending_ttl_seconds: Number(body.pending_ttl_seconds ?? 300),
    custom_policy_text: String(body.custom_policy_text ?? ''),
    webhook: {
      enabled: webhook.enabled === true,
      url: String(webhook.url ?? ''),
      bearer_token: String(webhook.bearer_token ?? ''),
      extra_headers: Object.fromEntries(
        Object.entries(
          (webhook.extra_headers ?? {}) as Record<string, unknown>,
        ).map(([name, value]) => [name, String(value)]),
      ),
    },
  }
}

/**
 * `LlmReviewSettings::validate` from the backend: a disabled review accepts
 * anything, an enabled one has to be usable.
 */
function validateLlmReview(settings: LlmReviewSettings): string | null {
  if (!settings.enabled) return null
  if (settings.review_base_url.trim() === '') {
    return 'review_base_url is required when LLM review is enabled'
  }
  if (settings.review_base_url.trim().replace(/\/+$/, '').endsWith('/v1')) {
    return 'review_base_url must not include /v1'
  }
  if (settings.review_model.trim() === '') {
    return 'review_model is required when LLM review is enabled'
  }
  if (settings.review_timeout_ms < 100) {
    return 'review_timeout_ms must be at least 100'
  }
  if (settings.pending_ttl_seconds === 0) {
    return 'pending_ttl_seconds must be at least 1'
  }
  if (settings.webhook.enabled && settings.webhook.url.trim() === '') {
    return 'webhook.url is required when webhook is enabled'
  }
  for (const [name, value] of Object.entries(settings.webhook.extra_headers)) {
    if (name.trim() === '') {
      return 'webhook.extra_headers must not contain empty header names'
    }
    if (value.includes('\n') || value.includes('\r')) {
      return 'webhook.extra_headers values must be single-line'
    }
  }
  return null
}

const SETTING_PATHS: Array<{
  path: string
  admin: boolean
  read: (ctx: FixtureContext) => unknown
  write: (ctx: FixtureContext, body: Record<string, unknown>) => unknown
}> = [
  {
    path: '/api/v1/settings/request-content-logging',
    admin: true,
    read: (ctx) => ctx.state.settings.requestContentLogging,
    write: (ctx, body) => {
      // The backend PATCH reads the whole usage-retention record, overwrites
      // only the raw-retention days, and writes the record back.
      const request = body as unknown as RequestContentLoggingRequest
      ctx.state.settings.requestContentLogging = {
        ...ctx.state.settings.requestContentLogging,
        mode: request.mode,
        raw_retention_days: request.raw_retention_days,
      }
      ctx.state.settings.usageRetention = {
        ...ctx.state.settings.usageRetention,
        raw_retention_days: request.raw_retention_days,
      }
      return ctx.state.settings.requestContentLogging
    },
  },
  {
    path: '/api/v1/settings/usage-retention',
    admin: true,
    read: (ctx) => ctx.state.settings.usageRetention,
    write: (ctx, body) => {
      ctx.state.settings.usageRetention = {
        ...ctx.state.settings.usageRetention,
        ...(body as unknown as UsageRetentionSettings),
      }
      // Both forms read the same stored raw-retention days.
      ctx.state.settings.requestContentLogging = {
        ...ctx.state.settings.requestContentLogging,
        raw_retention_days:
          ctx.state.settings.usageRetention.raw_retention_days ??
          ctx.state.settings.requestContentLogging.raw_retention_days,
      }
      return ctx.state.settings.usageRetention
    },
  },
  {
    path: '/api/v1/settings/stream-delta-batching',
    admin: true,
    read: (ctx) => ctx.state.settings.streamDeltaBatching,
    write: (ctx, body) => {
      ctx.state.settings.streamDeltaBatching = {
        ...ctx.state.settings.streamDeltaBatching,
        ...(body as unknown as StreamDeltaBatchingSettings),
      }
      return ctx.state.settings.streamDeltaBatching
    },
  },
  {
    path: '/api/v1/settings/relay-ip-whitelist',
    admin: true,
    read: (ctx) => ctx.state.settings.relayIpWhitelist,
    write: (ctx, body) => {
      const request = body as unknown as RelayIpPolicy
      const fields: Array<[field: string, entries: string[]]> = [
        ['allowed_cidrs', normalizeRelayEntries(request.allowed_cidrs)],
        [
          'trusted_proxy_cidrs',
          normalizeRelayEntries(request.trusted_proxy_cidrs),
        ],
      ]
      for (const [field, entries] of fields) {
        for (const [index, entry] of entries.entries()) {
          if (!isRelayEntry(entry)) {
            return error(
              'bad_request',
              `invalid relay IP whitelist: ${field}[${index}]: invalid IP or CIDR: ${entry}`,
              400,
            )
          }
        }
      }
      // The backend replaces both lists with the submitted policy.
      ctx.state.settings.relayIpWhitelist = {
        allowed_cidrs: fields[0]?.[1] ?? [],
        trusted_proxy_cidrs: fields[1]?.[1] ?? [],
      }
      return ctx.state.settings.relayIpWhitelist
    },
  },
  {
    path: '/api/v1/settings/model-route-whitelist',
    admin: true,
    read: (ctx) => ctx.state.settings.modelRouteWhitelist,
    write: (ctx, body) => {
      ctx.state.settings.modelRouteWhitelist = {
        enabled: body.enabled === true,
      }
      return ctx.state.settings.modelRouteWhitelist
    },
  },
  {
    path: '/api/v1/settings/llm-review',
    admin: true,
    read: (ctx) => ctx.state.settings.llmReview,
    write: (ctx, body) => {
      const request = normalizeLlmReview(body)
      const validationError = validateLlmReview(request)
      if (validationError) return error('bad_request', validationError, 400)
      // The server stores the submitted object as the whole settings value.
      ctx.state.settings.llmReview = request
      return ctx.state.settings.llmReview
    },
  },
  {
    path: '/api/v1/settings/raw-object-store',
    admin: true,
    read: (ctx) => ctx.state.settings.rawObjectStore,
    write: (ctx, body) => {
      const request = body as unknown as RawObjectStoreSettingsRequest
      const accessKey = resolveStorageSecret(
        ctx.state.privateStorageSecrets.s3_access_key,
        request.s3_access_key,
      )
      if ('failure' in accessKey) {
        return error('bad_request', accessKey.failure, 400)
      }
      const secretKey = resolveStorageSecret(
        ctx.state.privateStorageSecrets.s3_secret_key,
        request.s3_secret_key,
      )
      if ('failure' in secretKey) {
        return error('bad_request', secretKey.failure, 400)
      }
      // The backend normalizes before it validates.
      const normalized = {
        backend: request.backend,
        local_dir: String(request.local_dir ?? '').trim(),
        s3_allow_http: request.s3_allow_http === true,
        s3_bucket: String(request.s3_bucket ?? '').trim(),
        s3_endpoint: String(request.s3_endpoint ?? '').trim(),
        s3_path_style: request.s3_path_style ?? true,
        s3_prefix: normalizeStoragePrefix(String(request.s3_prefix ?? '')),
        s3_region: normalizeStorageRegion(String(request.s3_region ?? '')),
      }
      if (normalized.backend === 's3' && normalized.s3_bucket === '') {
        return error('bad_request', 's3 bucket is required for s3 backend', 400)
      }
      ctx.state.privateStorageSecrets = {
        s3_access_key: accessKey.value,
        s3_secret_key: secretKey.value,
      }
      // The response is redacted: presence only, never the stored value.
      ctx.state.settings.rawObjectStore = {
        ...normalized,
        has_s3_access_key: accessKey.value !== null,
        has_s3_secret_key: secretKey.value !== null,
      }
      return ctx.state.settings.rawObjectStore
    },
  },
  {
    path: '/api/v1/settings/cache-alert',
    admin: true,
    // The response is always redacted: the stored secret stays private and only
    // its presence is reported.
    read: (ctx) => cacheAlertResponse(ctx.state),
    write: (ctx, body) => {
      const request = body as unknown as Partial<CacheAlertSettingsRequest>
      // The backend type carries serde defaults, so a partial payload fills every
      // missing field with the default instead of merging the previous record.
      const candidate = {
        enabled: request.enabled ?? CACHE_ALERT_DEFAULT.enabled,
        window_minutes:
          request.window_minutes ?? CACHE_ALERT_DEFAULT.window_minutes,
        min_turns: request.min_turns ?? CACHE_ALERT_DEFAULT.min_turns,
        threshold: request.threshold ?? CACHE_ALERT_DEFAULT.threshold,
        cooldown_minutes:
          request.cooldown_minutes ?? CACHE_ALERT_DEFAULT.cooldown_minutes,
        dingtalk_webhook_url: request.dingtalk_webhook_url ?? '',
        dingtalk_secret: request.dingtalk_secret ?? '',
      }
      // `validate` runs before `normalized`, so an out-of-range value is
      // rejected rather than clamped.
      const failure = validateCacheAlert(candidate)
      if (failure) return error('bad_request', failure, 400)
      const storedSecret = ctx.state.privateCacheAlertSecret
      const submittedSecret = candidate.dingtalk_secret.trim()
      // A blank value keeps the stored write-only secret.
      ctx.state.privateCacheAlertSecret =
        submittedSecret === '' ? storedSecret : submittedSecret
      // An omitted SMTP object keeps the stored configuration, exactly as the
      // server does for a client that never knew the channel.
      let smtp = ctx.state.settings.cacheAlert.smtp ?? null
      if (request.smtp !== undefined && request.smtp !== null) {
        const requested = request.smtp
        const requestedUsername = (requested.username ?? '').trim()
        const submittedPassword = (requested.password ?? '').trim()
        const storedUsername = (smtp?.username ?? '').trim()
        // A blank password keeps the stored one only while the username stays
        // the same; a changed username drops it instead of pairing it with
        // another account.
        const password =
          submittedPassword === ''
            ? requestedUsername === storedUsername
              ? ctx.state.privateCacheAlertSmtpPassword
              : ''
            : submittedPassword
        const smtpCandidate = {
          enabled: requested.enabled ?? false,
          host: (requested.host ?? '').trim(),
          port: requested.port ?? CACHE_ALERT_SMTP_DEFAULT.port,
          tls: requested.tls ?? CACHE_ALERT_SMTP_DEFAULT.tls,
          from: (requested.from ?? '').trim(),
          to: (requested.to ?? [])
            .map((recipient) => recipient.trim())
            .filter((recipient) => recipient !== ''),
          username: requestedUsername,
          password,
        }
        const smtpFailure = validateCacheAlertSmtp(smtpCandidate)
        if (smtpFailure) return error('bad_request', smtpFailure, 400)
        ctx.state.privateCacheAlertSmtpPassword = smtpCandidate.password
        smtp = {
          enabled: smtpCandidate.enabled,
          from: smtpCandidate.from,
          has_smtp_password: smtpCandidate.password !== '',
          host: smtpCandidate.host,
          password: '',
          port: smtpCandidate.port,
          tls: smtpCandidate.tls,
          to: smtpCandidate.to,
          username: smtpCandidate.username,
        }
      }
      ctx.state.settings.cacheAlert = {
        enabled: candidate.enabled,
        window_minutes: candidate.window_minutes,
        min_turns: candidate.min_turns,
        threshold: candidate.threshold,
        cooldown_minutes: candidate.cooldown_minutes,
        dingtalk_webhook_url: candidate.dingtalk_webhook_url.trim(),
        dingtalk_secret: '',
        has_dingtalk_secret: ctx.state.privateCacheAlertSecret.trim() !== '',
        smtp,
      }
      return cacheAlertResponse(ctx.state)
    },
  },
  {
    path: '/api/v1/settings/endpoint',
    admin: false,
    read: (ctx) => ctx.state.settings.endpointSetting,
    write: (ctx, body) => {
      ctx.state.settings.endpointSetting = {
        endpoint_id: (body.endpoint_id as string | null | undefined) ?? null,
      }
      return ctx.state.settings.endpointSetting
    },
  },
]

function redactionPreview(body: RedactionPreviewRequestSchema) {
  const matches: Array<{
    end: number
    kind: 'email' | 'secret'
    match_text: string
    replacement: '[EMAIL]' | '[SECRET]'
    start: number
  }> = []

  if (body.enabled && body.rules.email) {
    const email = /[\w.+-]+@[\w.-]+\.\w+/g
    for (
      let match = email.exec(body.text);
      match;
      match = email.exec(body.text)
    ) {
      matches.push({
        end: match.index + match[0].length,
        kind: 'email',
        match_text: match[0],
        replacement: '[EMAIL]',
        start: match.index,
      })
    }
  }
  if (body.enabled && body.rules.secret) {
    const secret = /\bsk_fixture_[A-Za-z0-9]+\b/g
    for (
      let match = secret.exec(body.text);
      match;
      match = secret.exec(body.text)
    ) {
      matches.push({
        end: match.index + match[0].length,
        kind: 'secret',
        match_text: match[0],
        replacement: '[SECRET]',
        start: match.index,
      })
    }
  }

  matches.sort(
    (left, right) => left.start - right.start || right.end - left.end,
  )
  const nonOverlapping: typeof matches = []
  for (const match of matches) {
    if (
      nonOverlapping.length === 0 ||
      match.start >= nonOverlapping[nonOverlapping.length - 1]!.end
    ) {
      nonOverlapping.push(match)
    }
  }

  let offset = 0
  let redactedText = ''
  const findings = nonOverlapping.map((match) => {
    redactedText += body.text.slice(offset, match.start) + match.replacement
    offset = match.end
    return {
      confidence: 0.98,
      end: match.end,
      kind: match.kind,
      match_text: match.match_text,
      normalized_key: match.kind,
      source: 'rule',
      start: match.start,
    }
  })
  redactedText += body.text.slice(offset)
  const appliedReplacements = nonOverlapping.map((match) => ({
    display_value: match.match_text,
    kind: match.kind,
    replacement: match.replacement,
    strategy: 'mask',
  }))

  return {
    applied_replacements: appliedReplacements,
    findings,
    redacted_text: redactedText,
    stats: {
      applied_replacements: appliedReplacements.length,
      dropped_findings: 0,
      llm_candidates_accepted: 0,
      llm_candidates_rejected: 0,
      llm_configured: false,
      llm_error: null,
      llm_request_failed: false,
      total_findings: findings.length,
    },
  }
}

type RedactionTarget = {
  scope: RedactionScopeSchema
  userId: number | null
  config: RedactionConfigSchema
}

function defaultUserRedactionConfig(userId: number): RedactionConfigSchema {
  return {
    custom_strings: [
      {
        match_type: 'contains',
        pattern: `private-fixture-${userId}`,
        scope: 'text',
      },
    ],
    enabled: userId % 2 === 0,
    rules: {
      cidr: true,
      domain: true,
      email: true,
      ip: true,
      organization: false,
      person: false,
      phone: true,
      secret: userId !== 2,
      url: true,
    },
  }
}

function resolveRedactionTarget(
  ctx: FixtureContext,
): RedactionTarget | Response {
  const account = ctx.account
  if (!account) return error('unauthorized', 'login required', 401)

  const requestedScope = queryString(ctx.url, 'scope')
  const scope = requestedScope ?? (account.isAdmin ? 'global' : 'user')
  if (scope !== 'global' && scope !== 'user') {
    return error('invalid_scope', 'invalid redaction scope', 400)
  }
  if (!account.isAdmin && scope !== 'user') {
    return error('forbidden', 'fixture account cannot read global rules', 403)
  }

  const rawUserId = ctx.url.searchParams.get('user_id')
  const userId =
    rawUserId === null || rawUserId === ''
      ? account.user.user_id
      : Number(rawUserId)
  if (!Number.isSafeInteger(userId) || userId <= 0) {
    return error('invalid_user_id', 'invalid redaction user id', 400)
  }
  if (!account.isAdmin && userId !== account.user.user_id) {
    return error(
      'forbidden',
      'fixture account cannot read another user rules',
      403,
    )
  }
  if (
    !ctx.state.accounts.some((candidate) => candidate.user.user_id === userId)
  ) {
    return error('not_found', 'fixture redaction user not found', 404)
  }

  if (scope === 'global') {
    return { scope, userId: null, config: ctx.state.redaction }
  }
  const config =
    ctx.state.redactionByUser[userId] ?? defaultUserRedactionConfig(userId)
  ctx.state.redactionByUser[userId] = config
  return { scope, userId, config }
}

function normalizeRedactionConfig(
  config: RedactionConfigSchema,
): RedactionConfigSchema {
  const seen = new Set<string>()
  const now = isoNow()
  const customStrings: CustomStringRuleSchema[] = []
  for (const rule of config.custom_strings) {
    const pattern = rule.pattern.trim()
    if (!pattern) continue
    const signature = `${pattern}::${rule.match_type}::${rule.scope}`
    if (seen.has(signature)) continue
    seen.add(signature)
    customStrings.push({
      ...rule,
      created_at: rule.created_at ?? now,
      pattern,
      updated_at: now,
    })
  }
  return {
    ...config,
    rules: { ...config.rules },
    custom_strings: customStrings,
  }
}

function redactionTargetResponse(target: RedactionTarget) {
  return {
    config: target.config,
    scope: target.scope,
    user_id: target.userId,
  }
}

function redactionCustomStringRows(
  config: RedactionConfigSchema,
): RedactionCustomStringRuleRowSchema[] {
  return config.custom_strings
    .map((rule, array_index) => ({
      array_index,
      created_at: rule.created_at ?? null,
      match_type: rule.match_type,
      pattern: rule.pattern,
      scope: rule.scope,
      updated_at: rule.updated_at ?? null,
    }))
    .sort((left, right) => right.array_index - left.array_index)
}

async function handleRedactionSettings(
  ctx: FixtureContext,
): Promise<Response | null> {
  const { method } = ctx.request
  const { pathname } = ctx.url

  if (pathname === '/api/v1/settings/redaction') {
    if (method !== 'GET' && method !== 'PATCH') return null
    const target = resolveRedactionTarget(ctx)
    if (target instanceof Response) return target
    if (method === 'GET') return ok(redactionTargetResponse(target))

    const config = normalizeRedactionConfig(
      await readJson<RedactionConfigSchema>(ctx),
    )
    if (target.scope === 'global') {
      ctx.state.redaction = config
      ctx.state.redactionCustomStrings = redactionCustomStringRows(config)
    } else if (target.userId !== null) {
      ctx.state.redactionByUser[target.userId] = config
    }
    return ok(redactionTargetResponse({ ...target, config }))
  }

  if (
    pathname === '/api/v1/settings/redaction/custom-strings' &&
    method === 'GET'
  ) {
    const target = resolveRedactionTarget(ctx)
    if (target instanceof Response) return target
    const search = (queryString(ctx.url, 'search') ?? '').trim().toLowerCase()
    const rows = redactionCustomStringRows(target.config).filter((row) =>
      row.pattern.toLowerCase().includes(search),
    )
    const first = Math.max(0, queryInt(ctx.url, 'first', 0))
    const size = Math.max(1, queryInt(ctx.url, 'rows', 10))
    return ok({
      first,
      items: rows.slice(first, first + size),
      rows: size,
      total: rows.length,
    })
  }

  return null
}

function scopedUserId(ctx: FixtureContext): number | null {
  if (!ctx.account) return null
  return ctx.account.isAdmin ? null : ctx.account.user.user_id
}

/**
 * A pending wait deadline is a live countdown, so every read serves the
 * deadline that is current for that approval instead of a stale stamp.
 */
function liveApproval(approval: ApprovalRequest): ApprovalRequest {
  return {
    ...approval,
    wait_deadline_unix_ms: approvalWaitDeadline(approval.approval_id),
  }
}

export async function handleMoneySettings(
  ctx: FixtureContext,
): Promise<Response | null> {
  const { method } = ctx.request
  const { pathname } = ctx.url

  if (
    pathname.startsWith('/api/v1/settings/') ||
    /^\/api\/v1\/admin\/billing(?:\/|$)/.test(pathname)
  ) {
    if (!ctx.account) return error('unauthorized', 'login required', 401)
  } else {
    const adminMissing = requireAdmin(ctx)
    if (adminMissing) return adminMissing
  }

  const redactionSettings = await handleRedactionSettings(ctx)
  if (redactionSettings) return redactionSettings

  if (APPROVALS.test(pathname) && method === 'GET') {
    const status = queryString(ctx.url, 'status')
    const live = ctx.state.approvals.map(liveApproval)
    const pending = live.filter(
      (approval) => approval.approval_status === 'pending',
    )
    const resolved = live.filter(
      (approval) => approval.approval_status !== 'pending',
    )
    const items =
      status === 'resolved'
        ? sortResolvedApprovals(resolved)
        : sortPendingApprovals(pending)
    return ok(
      page(
        items,
        queryInt(ctx.url, 'first', 0),
        queryInt(ctx.url, 'rows', 10),
        'approvals',
      ),
    )
  }

  const approvalId = APPROVAL_ONE.exec(pathname)?.[1]
  if (approvalId && method === 'GET') {
    const approval = ctx.state.approvals.find(
      (candidate) => candidate.approval_id === approvalId,
    )
    if (!approval) return error('not_found', 'fixture approval not found', 404)
    return ok(liveApproval(approval))
  }

  const decision = APPROVAL_DECIDE.exec(pathname)
  if (decision && method === 'POST') {
    const [, approvalId, action] = decision
    const approval = ctx.state.approvals.find(
      (candidate) => candidate.approval_id === approvalId,
    )
    if (!approval) return error('not_found', 'fixture approval not found', 404)
    if (approval.approval_status !== 'pending') {
      return error('invalid_state', 'fixture approval is not pending', 409)
    }
    approval.approval_status = action === 'approve' ? 'approved' : 'rejected'
    approval.request_payload_json = null
    approval.decided_at = isoNow()
    approval.decided_by_login_name = ctx.account?.user.login_name ?? null
    approval.decided_by_user_id = ctx.account?.user.user_id ?? null
    approval.updated_at = isoNow()
    return ok(liveApproval(approval))
  }

  if (CHARGES.test(pathname) && method === 'GET') {
    return ok(
      page(
        filteredCharges(ctx.state.billingCharges, ctx.url, {
          forcedUserId: scopedUserId(ctx),
        }),
        queryInt(ctx.url, 'first', 0),
        queryInt(ctx.url, 'rows', 10),
        'charges',
      ),
    )
  }

  const chargeId = CHARGE_ONE.exec(pathname)?.[1]
  if (chargeId && method === 'GET') {
    const charge = ctx.state.billingCharges.find(
      (candidate) =>
        String(candidate.charge_id) === chargeId &&
        (ctx.account?.isAdmin === true ||
          candidate.user_id === ctx.account?.user.user_id),
    )
    if (!charge) return error('not_found', 'fixture charge not found', 404)
    return ok({
      charge,
      lines: [
        {
          amount: '0.002400',
          line_id: 1,
          meter: 'input_tokens',
          price_rule_id: 'price-minimax-m2',
          token_count: charge.input_tokens,
          unit_rate: '0.00300',
        },
        {
          amount: '0.003120',
          line_id: 2,
          meter: 'output_tokens',
          price_rule_id: 'price-minimax-m2',
          token_count: charge.output_tokens,
          unit_rate: '0.01200',
        },
      ],
    })
  }

  if (BILLING_EXPORT.test(pathname) && method === 'GET') {
    const kind = queryString(ctx.url, 'kind') ?? 'details'
    const charges = filteredCharges(ctx.state.billingCharges, ctx.url, {
      forcedUserId: scopedUserId(ctx),
    })
    const csv =
      kind === 'monthly' || kind === 'summary'
        ? billingMonthlyCsv(charges)
        : billingDetailsCsv(charges, ctx.account?.isAdmin === true)
    return new Response(csv, {
      headers: { 'content-type': 'text/csv; charset=utf-8' },
      status: 200,
    })
  }

  if (PRICE_RULES.test(pathname)) {
    const adminMissing = requireAdmin(ctx)
    if (adminMissing) return adminMissing
    if (method === 'GET') {
      return ok(
        page(
          sortedPriceRules(ctx.state.billingPriceRules),
          queryInt(ctx.url, 'first', 0),
          queryInt(ctx.url, 'rows', 10),
          'rules',
        ),
      )
    }
    if (method === 'POST') {
      const body = await readJson<BillingPriceRuleRequest>(ctx)
      const now = isoNow()
      const rule = {
        ...body,
        created_at: now,
        created_by_user_id: ctx.account?.user.user_id ?? null,
        currency: 'CNY',
        effective_to: null,
        enabled: true,
        price_rule_id: nextId(ctx.state, 'price'),
        updated_at: now,
      }
      ctx.state.billingPriceRules.push(rule)
      return ok(rule)
    }
  }

  const priceRuleId = PRICE_RULE_ONE.exec(pathname)?.[1]
  if (priceRuleId) {
    const adminMissing = requireAdmin(ctx)
    if (adminMissing) return adminMissing
    const rule = ctx.state.billingPriceRules.find(
      (candidate) => candidate.price_rule_id === priceRuleId,
    )
    if (!rule) return error('not_found', 'fixture price rule not found', 404)
    if (method === 'PUT') {
      const body = await readJson<BillingPriceRuleRequest>(ctx)
      Object.assign(rule, body, { updated_at: isoNow() })
      return ok(rule)
    }
    if (method === 'PATCH') {
      const body = await readJson<{ enabled: boolean }>(ctx)
      rule.enabled = body.enabled
      rule.updated_at = isoNow()
      return ok(rule)
    }
    if (method === 'DELETE') {
      ctx.state.billingPriceRules = ctx.state.billingPriceRules.filter(
        (candidate) => candidate !== rule,
      )
      return new Response(null, { status: 204 })
    }
  }

  if (REPRICE.test(pathname) && method === 'POST') {
    const adminMissing = requireAdmin(ctx)
    if (adminMissing) return adminMissing
    return ok({
      repriced: ctx.state.billingCharges.filter(
        (charge) => charge.pricing_status !== 'priced',
      ).length,
    })
  }

  if (BILLING_SUMMARY.test(pathname) && method === 'GET') {
    return ok(
      chargeSummary(
        filteredCharges(ctx.state.billingCharges, ctx.url, {
          forcedUserId: scopedUserId(ctx),
        }),
      ),
    )
  }

  if (CONFIG_AUDIT.test(pathname) && method === 'GET') {
    const adminMissing = requireAdmin(ctx)
    if (adminMissing) return adminMissing
    return ok(
      page(
        ctx.state.configAudit,
        queryInt(ctx.url, 'first', 0),
        queryInt(ctx.url, 'rows', 10),
        'entries',
      ),
    )
  }

  if (CONFIG_EXPORT.test(pathname) && method === 'POST') {
    const adminMissing = requireAdmin(ctx)
    if (adminMissing) return adminMissing
    const body = await readJson<Partial<ConfigExportRequest> | null>(ctx)
    const passphrase =
      typeof body?.passphrase === 'string' ? body.passphrase : ''
    // The audit row exists for every attempt, so the failed ones are recorded
    // before the response goes out, exactly like the backend.
    const encoded = encodeSyntheticArchive(passphrase, {
      exported_at: nextArchiveTimestamp(ctx.state),
      domains: fixtureArchiveDomains(ctx.state),
    })
    if (!encoded.ok) {
      recordArchiveAudit(ctx, {
        action: 'export',
        error: encoded.error,
        success: false,
      })
      return error(
        encoded.error.code,
        encoded.error.message,
        encoded.error.status,
      )
    }
    recordArchiveAudit(ctx, {
      action: 'export',
      archiveBytes: encoded.value.bytes.length,
      payloadFingerprint: encoded.value.payload_fingerprint,
      domains: fixtureArchiveDomains(ctx.state),
      success: true,
    })
    // The three headers describe the bytes above, and nothing more.
    return new Response(encoded.value.bytes, {
      headers: {
        'cache-control': 'no-store, no-cache, must-revalidate, private',
        'content-type': 'application/octet-stream',
        'x-config-export-backend': encoded.value.backend,
        'x-config-export-format-version': String(encoded.value.format_version),
        'x-config-export-fingerprint': encoded.value.payload_fingerprint,
      },
      status: 200,
    })
  }

  if (CONFIG_EXPORT_METADATA.test(pathname) && method === 'POST') {
    return ok({
      backend_kind: 'local',
      domains: [
        { name: 'endpoints', records: 3, unrecoverable_secrets: 1 },
        { name: 'model_routes', records: 2, unrecoverable_secrets: 0 },
      ],
      format_version: 1,
      payload_fingerprint: 'c1b2c3d4e5f60718293a4b5c6d7e8f90',
    })
  }

  if (CONFIG_IMPORT_PREVIEW.test(pathname) && method === 'POST') {
    const adminMissing = requireAdmin(ctx)
    if (adminMissing) return adminMissing
    const body = await readJson<Partial<ConfigImportRequest> | null>(ctx)
    const decoded = decodeArchiveRequest(ctx, body)
    if (!decoded.ok) {
      return error(
        decoded.error.code,
        decoded.error.message,
        decoded.error.status,
      )
    }
    // The preview is read-only: it never writes an audit row and never applies
    // the archive.
    const preview: ConfigImportPreview = {
      backend_kind: decoded.value.backend,
      domains: diffSyntheticArchive(
        decoded.value.payload,
        fixtureArchiveDomains(ctx.state),
      ),
      exported_at: decoded.value.payload.exported_at,
      format_version: decoded.value.format_version,
      payload_fingerprint: decoded.value.payload_fingerprint,
      warnings: syntheticArchiveWarnings(decoded.value.payload),
    }
    return ok(preview)
  }

  if (CONFIG_IMPORT.test(pathname) && method === 'POST') {
    const adminMissing = requireAdmin(ctx)
    if (adminMissing) return adminMissing
    const body = await readJson<Partial<ConfigImportRequest> | null>(ctx)
    const archiveBase64 =
      typeof body?.archive_base64 === 'string' ? body.archive_base64 : ''
    const bytes = decodeSyntheticArchiveBase64(archiveBase64)
    if (!bytes.ok) {
      recordArchiveAudit(ctx, {
        action: 'import',
        error: bytes.error,
        success: false,
      })
      return error(bytes.error.code, bytes.error.message, bytes.error.status)
    }
    const decoded = decodeArchiveBytes(ctx, body, bytes.value)
    if (!decoded.ok) {
      recordArchiveAudit(ctx, {
        action: 'import',
        archiveBytes: bytes.value.length,
        error: decoded.error,
        success: false,
      })
      return error(
        decoded.error.code,
        decoded.error.message,
        decoded.error.status,
      )
    }
    recordArchiveAudit(ctx, {
      action: 'import',
      archiveBytes: bytes.value.length,
      payloadFingerprint: decoded.value.payload_fingerprint,
      domains: decoded.value.payload.domains,
      success: true,
    })
    const applied: ConfigImportApplied = {
      backend_kind: decoded.value.backend,
      domains: decoded.value.payload.domains.map((domain) => ({
        name: domain.name,
        records: domain.records,
        unrecoverable_secrets: domain.unrecoverable_secrets,
      })),
      format_version: decoded.value.format_version,
      payload_fingerprint: decoded.value.payload_fingerprint,
    }
    return ok(applied)
  }

  if (pathname === '/api/v1/settings/redaction/preview' && method === 'POST') {
    const body = await readJson<RedactionPreviewRequestSchema>(ctx)
    return ok({ preview: redactionPreview(body) })
  }

  const setting = SETTING_PATHS.find((candidate) => candidate.path === pathname)
  if (setting) {
    if (setting.admin) {
      const adminMissing = requireAdmin(ctx)
      if (adminMissing) return adminMissing
    }
    if (method === 'GET') return ok(setting.read(ctx))
    if (method === 'PATCH' || method === 'PUT') {
      const body = await readJson<Record<string, unknown>>(ctx)
      const written = setting.write(ctx, body)
      // A setting write may answer its own validation error.
      return written instanceof Response ? written : ok(written)
    }
  }

  return null
}

export type { ApprovalRequest }
