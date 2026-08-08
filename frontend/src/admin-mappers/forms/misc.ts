import type {
  LlmReviewSettings,
  RelayIpPolicy,
  RelayIpPolicyResponse,
} from '../../generated/admin-api'
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

export function webhookHeadersToText(headers?: Record<string, string>): string {
  return Object.entries(headers ?? {})
    .map(([key, value]) => `${key}: ${value}`)
    .join('\n')
}

export function webhookHeadersFromText(text: string): Record<string, string> {
  const record: Record<string, string> = {}
  for (const line of splitLines(text)) {
    const index = line.indexOf(':')
    if (index <= 0) {
      throw new Error('Header format must be Name: value')
    }
    record[line.slice(0, index).trim()] = line.slice(index + 1).trim()
  }
  return record
}

export function ensureLlmReviewDefaults(
  settings: LlmReviewSettings,
): LlmReviewSettings {
  return {
    ...settings,
    webhook: {
      enabled: settings.webhook?.enabled ?? false,
      url: settings.webhook?.url ?? '',
      bearer_token: settings.webhook?.bearer_token ?? '',
      extra_headers: settings.webhook?.extra_headers ?? {},
    },
  }
}
