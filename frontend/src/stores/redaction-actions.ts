import type { Ref } from 'vue'
import type {
  RedactionConfigSchema,
  RedactionInputKindSchema,
  RedactionPreviewSchema,
  RedactionScopeSchema,
} from '@/generated/admin-api'
import { previewRedaction, setRedactionSetting } from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import { capturePayload } from '@/stores/record-action-types'
import type { normalizeRedactionConfig } from './redaction-editor'

export const REDACTION_RULES_KEY = 'redaction:rules'
export const REDACTION_PREVIEW_KEY = 'redaction:preview'

type RedactionQuery = {
  scope: RedactionScopeSchema
  user_id?: number
}

type RedactionActionDeps = {
  guard: AsyncActionGuard
  config: Ref<RedactionConfigSchema>
  previewText: Ref<string>
  previewInputKind: Ref<RedactionInputKindSchema>
  previewResult: Ref<RedactionPreviewSchema | null>
  getQuery: () => RedactionQuery
  getTargetToken: () => string
  hasInvalidCustomStringRule: () => boolean
  normalizeConfig: typeof normalizeRedactionConfig
  markSaved: () => void
  loadCustomStringPage: () => Promise<void>
}

export function createRedactionActions(deps: RedactionActionDeps) {
  const {
    config,
    getQuery,
    getTargetToken,
    guard,
    hasInvalidCustomStringRule,
    loadCustomStringPage,
    markSaved,
    normalizeConfig,
    previewInputKind,
    previewResult,
    previewText,
  } = deps

  function save(): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      REDACTION_RULES_KEY,
      async () => {
        if (hasInvalidCustomStringRule()) {
          throw new Error('invalid custom string regex')
        }
        const targetToken = getTargetToken()
        const query = getQuery()
        const submitted = normalizeConfig(capturePayload(config.value))
        config.value = capturePayload(submitted)
        const body = capturePayload(submitted)
        const response = expectData(
          await setRedactionSetting<true>(withData({ query, body })),
        )
        if (targetToken !== getTargetToken()) return
        if (JSON.stringify(config.value) === JSON.stringify(submitted)) {
          config.value = normalizeConfig(response.config)
          markSaved()
        }
        await loadCustomStringPage()
      },
      'save',
    )
  }

  function runPreview(): Promise<AsyncActionOutcome<void>> {
    return guard.run(
      REDACTION_PREVIEW_KEY,
      async () => {
        const targetToken = getTargetToken()
        const configSnapshot = capturePayload(config.value)
        const textSnapshot = previewText.value
        const kindSnapshot = previewInputKind.value
        const body = {
          enabled: configSnapshot.enabled,
          custom_strings: configSnapshot.custom_strings,
          input_kind: kindSnapshot,
          rules: configSnapshot.rules,
          text: textSnapshot,
        }
        previewResult.value = null
        const response = expectData(
          await previewRedaction<true>(withData({ body })),
        )
        if (
          targetToken === getTargetToken() &&
          JSON.stringify(config.value) === JSON.stringify(configSnapshot) &&
          previewText.value === textSnapshot &&
          previewInputKind.value === kindSnapshot
        ) {
          previewResult.value = response.preview
        }
      },
      'preview',
    )
  }

  return { runPreview, save }
}
