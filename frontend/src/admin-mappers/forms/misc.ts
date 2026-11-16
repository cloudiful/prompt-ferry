import type { RelayIpPolicy, RelayIpPolicyResponse } from '../../generated/admin-api'
import type { RequestRecordClearForm } from '../../models'

import { splitLines } from '../utils'

export function createDefaultRequestRecordClearForm(): RequestRecordClearForm {
  return {
    scope: 'current_user',
    user_id: null,
    start_at: '',
    end_at: '',
    delete_all: false,
  }
}

export function relayPolicyToForm(policy: RelayIpPolicyResponse): {
  allowed_cidrs_text: string
  trusted_proxy_cidrs_text: string
} {
  return {
    allowed_cidrs_text: (policy.allowed_cidrs ?? []).join('\n'),
    trusted_proxy_cidrs_text: (policy.trusted_proxy_cidrs ?? []).join('\n'),
  }
}

export function relayFormToRequest(form: {
  allowed_cidrs_text: string
  trusted_proxy_cidrs_text: string
}): RelayIpPolicy {
  return {
    allowed_cidrs: splitLines(form.allowed_cidrs_text),
    trusted_proxy_cidrs: splitLines(form.trusted_proxy_cidrs_text),
  }
}
