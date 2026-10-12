import { computed, ref } from 'vue'
import { defineStore } from 'pinia'
import { getRedactionSetting } from '../generated/admin-api'
import type {
  RedactionConfigSchema,
  RedactionInputKindSchema,
  RedactionPreviewSchema,
  RedactionScopeSchema,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import { createRedactionDefaults } from '../admin-mappers'
import { useAsyncAction } from '../composables/useAsyncAction'
import {
  createRedactionActions,
  REDACTION_PREVIEW_KEY,
  REDACTION_RULES_KEY,
} from './redaction-actions'
import { createRedactionEditor } from './redaction-editor'

export type { CustomStringRuleWarning } from '../redaction-rules'
export { findRuleWarnings, validateCustomStringRule } from '../redaction-rules'

export const useRedactionStore = defineStore('redaction', () => {
  const loading = ref(false)
  const config = ref<RedactionConfigSchema>(createRedactionDefaults())
  const scope = ref<RedactionScopeSchema>('user')
  const targetUserId = ref<number | null>(null)
  const previewText = ref('')
  const previewInputKind = ref<RedactionInputKindSchema>('text')
  const previewResult = ref<RedactionPreviewSchema | null>(null)
  const targetGeneration = ref(0)

  function getQuery() {
    return {
      scope: scope.value,
      ...(scope.value === 'user' && targetUserId.value
        ? { user_id: targetUserId.value }
        : {}),
    }
  }

  function getTargetToken(): string {
    return `${targetGeneration.value}:${scope.value}:${targetUserId.value ?? ''}`
  }

  const editor = createRedactionEditor({
    config,
    getQuery,
    getTargetToken,
    loading,
  })
  const guard = useAsyncAction()
  const actions = createRedactionActions({
    config,
    getQuery,
    getTargetToken,
    guard,
    hasInvalidCustomStringRule: editor.hasInvalidCustomStringRule,
    loadCustomStringPage: editor.loadCustomStringPage,
    markSaved: editor.markSaved,
    normalizeConfig: editor.normalizeConfig,
    previewInputKind,
    previewResult,
    previewText,
  })

  const saving = computed(() => guard.isPending(REDACTION_RULES_KEY))
  const previewing = computed(() => guard.isPending(REDACTION_PREVIEW_KEY))

  function setTarget(nextScope: RedactionScopeSchema, userId?: number | null) {
    const nextUserId = userId ?? null
    if (scope.value !== nextScope || targetUserId.value !== nextUserId) {
      targetGeneration.value += 1
      previewResult.value = null
    }
    scope.value = nextScope
    targetUserId.value = nextUserId
  }

  async function refresh(): Promise<void> {
    const targetToken = getTargetToken()
    const query = getQuery()
    loading.value = true
    try {
      const response = expectData(
        await getRedactionSetting<true>(withData({ query })),
      )
      if (targetToken !== getTargetToken()) return
      scope.value = response.scope
      targetUserId.value = response.user_id ?? targetUserId.value
      config.value = editor.normalizeConfig(
        createRedactionDefaults(response.config),
      )
      editor.markSaved()
      await editor.loadCustomStringPage()
    } finally {
      loading.value = false
    }
  }

  return {
    ...actions,
    ...editor,
    config,
    isPending: guard.isPending,
    loading,
    pendingActions: guard.pendingActions,
    previewInputKind,
    previewResult,
    previewText,
    refresh,
    saving,
    scope,
    setTarget,
    targetUserId,
    previewing,
  }
})
