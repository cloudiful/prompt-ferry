import type {
  RequestRecordDetail,
  RequestRecordFullResponse,
  SessionAffinityState,
  SessionRouteOptionsResponse,
} from '@/generated/admin-api'
import {
  deriveModelDisplay,
  deriveReasoningEffortDisplay,
  upstreamLabel,
} from '@/admin-mappers/request-records-core'

export type UsageConversationOverrideView = {
  conversation_id: string
  endpoint_id: string
  endpoint_key_id: string | null
  endpoint_key_label: string | null
  endpoint_name: string | null
}

export type UsageRouteOptionsView = SessionRouteOptionsResponse & {
  current_upstream_label: string
}

export type UsageDetailView = RequestRecordDetail & {
  actor: string
  conversation_source_label: string
  is_first_turn: boolean
  is_session_recognized: boolean
  is_sub_session: boolean
  model_key: string
  model_display: string
  session_short_id: string
  target: string
  upstream_label: string
  user_key: string
}

/**
 * The detail row's display labels.
 *
 * Dates are not derived here: the React surfaces format them at render time
 * from the current locale and display time zone.
 */
export function createUsageDetailView(
  record: RequestRecordDetail,
): UsageDetailView {
  const label = upstreamLabel(record)
  const sessionRecognized = Boolean(record.conversation_id)
  const sessionParentId = (record.session_parent_id ?? '').trim()
  return {
    ...record,
    actor: record.user_login_name || '-',
    conversation_source_label: record.conversation_source || 'none',
    is_first_turn: sessionRecognized && (record.conversation_seq ?? 1) <= 1,
    is_session_recognized: sessionRecognized,
    is_sub_session: sessionParentId.length > 0,
    model_key: record.model || '-',
    model_display: deriveModelDisplay(record),
    session_short_id: record.conversation_id?.slice(0, 8) || '-',
    target: label,
    upstream_label: label,
    user_key: record.user_login_name || '-',
  }
}

/** The route-options view with the current upstream label fallback chain. */
export function createUsageRouteOptionsView(
  response: SessionRouteOptionsResponse,
): UsageRouteOptionsView {
  const current = response.options.find(
    (option) => option.endpoint_id === response.current_endpoint_id,
  )
  const fallback = response.options.find((option) => option.is_preferred)
  return {
    ...response,
    current_upstream_label:
      current?.endpoint_name ||
      fallback?.endpoint_name ||
      response.current_endpoint_id ||
      '-',
  }
}

/** The current override a route-options response carries, if any. */
export function createUsageOverrideView(
  response: SessionRouteOptionsResponse,
): UsageConversationOverrideView | null {
  if (!response.override_endpoint_id) return null
  const option = response.options.find(
    (candidate) => candidate.endpoint_id === response.override_endpoint_id,
  )
  return {
    conversation_id: response.conversation_id,
    endpoint_id: response.override_endpoint_id,
    endpoint_key_id: response.override_endpoint_key_id ?? null,
    endpoint_key_label: response.override_endpoint_key_label ?? null,
    endpoint_name: option?.endpoint_name ?? null,
  }
}

type ReasoningCarrier = {
  request_raw_json?: unknown
}

/** The caller reasoning effort stored in a raw request payload. */
export function extractReasoningEffort(value: unknown): string | null {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return null
  const record = value as Record<string, unknown>
  const direct = record['reasoning_effort']
  if (typeof direct === 'string' && direct.trim().length > 0) {
    return direct.trim()
  }
  const reasoning = record['reasoning']
  if (reasoning && typeof reasoning === 'object' && !Array.isArray(reasoning)) {
    const effort = (reasoning as Record<string, unknown>)['effort']
    if (typeof effort === 'string' && effort.trim().length > 0) {
      return effort.trim()
    }
  }
  return null
}

/**
 * The reasoning-effort label, or `null` when neither payload names it.
 *
 * The caller value is shown as stored; the applied route-target override is
 * rendered as `caller → override` by the shared pure helper, which also keeps
 * the Anthropic `/v1/messages` path untouched.
 */
export function usageReasoningEffortDisplay(
  detail: ReasoningCarrier | null,
  requestFull: ReasoningCarrier | null,
): string | null {
  if (!detail) return null
  const effort =
    extractReasoningEffort(detail.request_raw_json) ??
    extractReasoningEffort(requestFull?.request_raw_json)
  if (!effort) return null
  const withOverride = detail as ReasoningCarrier & {
    applied_thinking_effort_override?: string | null
    path?: string | null
  }
  return deriveReasoningEffortDisplay(
    effort,
    withOverride.applied_thinking_effort_override,
    withOverride.path,
  )
}

/** Affinity state to its localized label key. */
export function usageAffinityStateKey(
  state: SessionAffinityState | null | undefined,
): 'bindingActive' | 'bindingStale' | 'bindingUnbound' {
  if (state === 'active') return 'bindingActive'
  if (state === 'stale_endpoint' || state === 'stale_key') return 'bindingStale'
  return 'bindingUnbound'
}

/**
 * The notice a completed affinity reset stands for: a cleared binding is a
 * success, a record that had no binding is informational rather than a failure.
 */
export function usageAffinityNoticeKey(response: {
  cleared: boolean
  cleared_count: number
}): 'affinityReset' | 'affinityNotBound' {
  return response.cleared || response.cleared_count > 0
    ? 'affinityReset'
    : 'affinityNotBound'
}

/** Abort reason to its localized label key. */
export function usageAbortReasonKey(
  reason: string | null | undefined,
):
  | 'abortReasonDownstreamClosed'
  | 'abortReasonBridgeBackpressureFull'
  | 'abortReasonBridgeBackpressureBytesLimit'
  | 'abortReasonWorkerLeaseExpired'
  | 'abortReasonValkeyLeaseMissing'
  | 'abortReasonRelayUnknown'
  | 'notAvailable' {
  switch (reason) {
    case 'downstream_closed':
      return 'abortReasonDownstreamClosed'
    case 'bridge_backpressure_full':
      return 'abortReasonBridgeBackpressureFull'
    case 'bridge_backpressure_bytes_limit':
      return 'abortReasonBridgeBackpressureBytesLimit'
    case 'worker_lease_expired':
      return 'abortReasonWorkerLeaseExpired'
    case 'valkey_lease_missing':
      return 'abortReasonValkeyLeaseMissing'
    case 'relay_unknown':
      return 'abortReasonRelayUnknown'
    default:
      return 'notAvailable'
  }
}

/** The abort origin state to its localized label key. */
export function usageAbortFromStateKey(
  state: string | null | undefined,
):
  | 'requestStateReceived'
  | 'requestStateAwaitingApproval'
  | 'requestStateUpstreamProcessing'
  | 'notAvailable' {
  switch (state) {
    case 'received':
      return 'requestStateReceived'
    case 'awaiting_approval':
      return 'requestStateAwaitingApproval'
    case 'upstream_processing':
      return 'requestStateUpstreamProcessing'
    default:
      return 'notAvailable'
  }
}

/** Whether the response had started when the request aborted. */
export function usageResponseStartedKey(
  value: boolean | null | undefined,
): 'responseStartedYes' | 'responseStartedNo' | 'notAvailable' {
  if (value === true) return 'responseStartedYes'
  if (value === false) return 'responseStartedNo'
  return 'notAvailable'
}

/**
 * A stored MCP response payload.
 *
 * Only a trimmed object/array text is parsed, inside a try/catch; anything else
 * stays the raw string so a non-JSON payload is never lost.
 */
export function parseUsageResponsePayload(value: string | null): unknown {
  const raw = value ?? ''
  if (raw.trim().length === 0) return null
  const trimmed = raw.trim()
  if (trimmed.startsWith('{') || trimmed.startsWith('[')) {
    try {
      return JSON.parse(trimmed) as unknown
    } catch {
      return raw
    }
  }
  return raw
}

/** Pretty JSON text for a payload, including `null`. */
export function usageJsonText(value: unknown): string {
  return JSON.stringify(value ?? null, null, 2) ?? 'null'
}

export type UsageTranscriptItem = {
  id: string
  kind: 'request' | 'assistant_message' | 'output_items' | 'response_prompt'
  label: string
  meta: string
  value: unknown
}

export type UsageTranscriptLabels = {
  requestRawJson: string
  sameAsTurn: string
  responsePromptMeta: string
}

/**
 * The transcript timeline items.
 *
 * A message becomes one request item; a raw request payload appears only when
 * there are no messages, and the assistant items are appended only when the
 * detail carries them. The response prompt is the last fallback.
 */
export function createUsageTranscriptItems(
  detail: ReasoningCarrier | null,
  requestFull: RequestRecordFullResponse | null,
  labels: UsageTranscriptLabels,
): UsageTranscriptItem[] {
  const items: UsageTranscriptItem[] = []
  const messages = requestFull?.messages ?? []
  messages.forEach((message, index) => {
    items.push({
      id: `request-${message.block_hash || index}`,
      kind: 'request',
      label: `${message.role || 'message'} · ${message.block_hash || index + 1}`,
      meta:
        message.same_as_turn != null
          ? `${labels.sameAsTurn} ${message.same_as_turn}`
          : message.preview_text?.trim() || `#${index + 1}`,
      value: message.content_json ?? message.preview_text ?? null,
    })
  })

  const rawJson =
    requestFull?.request_raw_json ??
    (detail as { request_raw_json?: unknown } | null)?.request_raw_json
  if (messages.length === 0 && rawJson != null) {
    const carrier = detail as {
      request_storage_mode?: string | null
    } | null
    items.push({
      id: 'request-raw-json',
      kind: 'request',
      label: labels.requestRawJson,
      meta:
        requestFull?.request_storage_mode ||
        carrier?.request_storage_mode ||
        'request',
      value: rawJson,
    })
  }

  const assistant = detail as RequestRecordDetail | null
  if (assistant?.assistant_message_json != null) {
    items.push({
      id: 'assistant-message',
      kind: 'assistant_message',
      label: 'assistant · message',
      meta: assistant.provider_response_id || 'assistant_message',
      value: assistant.assistant_message_json,
    })
  }
  if (assistant?.assistant_output_items_json != null) {
    items.push({
      id: 'assistant-output-items',
      kind: 'output_items',
      label: 'assistant · output_items',
      meta: Array.isArray(assistant.assistant_output_items_json)
        ? `${assistant.assistant_output_items_json.length} items`
        : 'output_items',
      value: assistant.assistant_output_items_json,
    })
  }
  if (
    items.length === 0 &&
    assistant?.response_prompt &&
    assistant.response_prompt.trim().length > 0
  ) {
    items.push({
      id: 'response-prompt',
      kind: 'response_prompt',
      label: 'assistant · response_prompt',
      meta: labels.responsePromptMeta,
      value: { response_prompt: assistant.response_prompt },
    })
  }
  return items
}

/** Append a request-full page onto the already-loaded transcript. */
export function mergeUsageRequestFull(
  base: RequestRecordFullResponse,
  page: RequestRecordFullResponse,
): RequestRecordFullResponse {
  return {
    ...base,
    ...page,
    messages: [...base.messages, ...page.messages],
    rendered_text: [base.rendered_text, page.rendered_text]
      .filter((text) => text && text.length > 0)
      .join('\n'),
  }
}
