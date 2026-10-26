/**
 * Pure model for the admin Network settings tab: the relay IP whitelist form.
 *
 * The tab is two plain textareas, one button save and one scoped refresh. The
 * only server contract that matters here is the policy itself: entries are
 * trimmed, blank lines dropped and duplicates removed by the server, and an
 * entry that is not an IP or a CIDR is rejected with a 400. The tab therefore
 * keeps no local CIDR parser — it sends what the operator typed and shows the
 * server's answer.
 *
 * One legacy behavior is a defect and is deliberately not reproduced: the old
 * form was replaced wholesale by the response, so a draft typed while the save
 * was in flight was silently lost. The merge below applies the server's value to
 * a textarea only while that textarea still holds the submitted text.
 */

import type {
  RelayIpPolicy,
  RelayIpPolicyResponse,
} from '@/generated/admin-api'
import {
  relayFormToRequest,
  relayPolicyToForm,
} from '@/admin-mappers/forms/misc'
import type { PendingActions } from './async-action'
import { settingKey } from './settings-keys'

/** The two lists, as the operator types them. */
export type RelayIpWhitelistForm = {
  allowed_cidrs_text: string
  trusted_proxy_cidrs_text: string
}

export type RelayIpWhitelistField = keyof RelayIpWhitelistForm

export const RELAY_IP_WHITELIST_FIELDS: ReadonlyArray<RelayIpWhitelistField> = [
  'allowed_cidrs_text',
  'trusted_proxy_cidrs_text',
]

export const RELAY_IP_WHITELIST_KEY = settingKey('relayIpWhitelist')

export function createRelayIpWhitelistForm(): RelayIpWhitelistForm {
  return { allowed_cidrs_text: '', trusted_proxy_cidrs_text: '' }
}

/** The server policy as the form shows it. */
export function relayPolicyToWhitelistForm(
  policy: RelayIpPolicyResponse | null | undefined,
): RelayIpWhitelistForm {
  if (!policy) return createRelayIpWhitelistForm()
  return relayPolicyToForm(policy)
}

/** The PATCH body: both arrays, always, with blank lines removed. */
export function relayWhitelistFormToRequest(
  form: RelayIpWhitelistForm,
): RelayIpPolicy {
  return relayFormToRequest(form)
}

/**
 * Apply a saved policy to the textareas the operator has not changed since the
 * submit. A field that already differs from the submitted text is a newer edit
 * and is never overwritten.
 */
export function mergeRelayIpWhitelistResponse(
  current: RelayIpWhitelistForm,
  submitted: RelayIpWhitelistForm,
  response: RelayIpPolicyResponse,
): RelayIpWhitelistForm {
  const normalized = relayPolicyToWhitelistForm(response)
  const merged: RelayIpWhitelistForm = { ...current }
  for (const field of RELAY_IP_WHITELIST_FIELDS) {
    if (Object.is(current[field], submitted[field])) {
      merged[field] = normalized[field]
    }
  }
  return merged
}

/** The fields whose text differs from the last accepted form. */
export function relayWhitelistDirtyFields(
  current: RelayIpWhitelistForm,
  baseline: RelayIpWhitelistForm,
): RelayIpWhitelistField[] {
  return RELAY_IP_WHITELIST_FIELDS.filter(
    (field) => current[field] !== baseline[field],
  )
}

/**
 * Whether a scoped refresh is safe.
 *
 * A refresh replaces the loaded policy, so it must not run while a save is in
 * flight, while either textarea holds an unsaved draft, or while a previous save
 * or read failed and the operator has not retried yet.
 */
export function relayWhitelistRefreshBlocked(input: {
  pendingActions: PendingActions
  dirty: boolean
  failed: boolean
  loading: boolean
  loaded: boolean
}): boolean {
  return (
    !input.loaded ||
    input.loading ||
    input.dirty ||
    input.failed ||
    Object.hasOwn(input.pendingActions, RELAY_IP_WHITELIST_KEY)
  )
}
