import type {
  TokenPlanKeyUsage,
  TokenPlanQuotaState,
} from '@/generated/admin-api'
import type { TokenPlanFetchState } from '@/models/endpoints/quota'

export type EffectiveTokenPlanKey = {
  key: TokenPlanKeyUsage
  state: TokenPlanQuotaState
  usable: boolean
  legacy: boolean
}

export function tokenPlanEffectiveKeys(
  keys: readonly TokenPlanKeyUsage[],
  fetchState: TokenPlanFetchState,
): EffectiveTokenPlanKey[] {
  return keys.map((key) => {
    const metadata = key.quota_status
    const legacy = metadata == null
    if (fetchState !== 'ready') {
      return { key, state: 'unknown', usable: false, legacy }
    }
    if (metadata?.scope === 'text_inference') {
      return {
        key,
        state: metadata.state,
        usable: metadata.state === 'available',
        legacy: false,
      }
    }
    return {
      key,
      state: 'unknown',
      usable: legacy && key.ok,
      legacy,
    }
  })
}
