import { computed, ref, type Ref } from 'vue'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '@/table-pagination'
import { listRedactionCustomStrings } from '@/generated/admin-api'
import type {
  RedactionConfigSchema,
  RedactionCustomStringRuleRowSchema,
  RedactionScopeSchema,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { createRowRevealState } from '@/redaction-reveal'
import {
  type CustomStringRuleWarning,
  findRuleWarnings,
  validateCustomStringRule,
} from '@/redaction-rules'

function normalizeCustomStrings(
  customStrings: RedactionConfigSchema['custom_strings'],
): RedactionConfigSchema['custom_strings'] {
  const seen = new Set<string>()
  const normalized: RedactionConfigSchema['custom_strings'] = []
  for (const rule of customStrings) {
    const pattern = rule.pattern.trim()
    if (!pattern) continue
    const dedupeKey = `${pattern}::${rule.match_type}::${rule.scope}`
    if (seen.has(dedupeKey)) continue
    seen.add(dedupeKey)
    normalized.push({ ...rule, pattern })
  }
  return normalized
}

export function normalizeRedactionConfig(
  nextConfig: RedactionConfigSchema,
): RedactionConfigSchema {
  return {
    ...nextConfig,
    custom_strings: normalizeCustomStrings(nextConfig.custom_strings),
  }
}

type RedactionQuery = {
  scope: RedactionScopeSchema
  user_id?: number
}

type RedactionEditorDeps = {
  config: Ref<RedactionConfigSchema>
  loading: Ref<boolean>
  getQuery: () => RedactionQuery
  getTargetToken: () => string
}

export function createRedactionEditor(deps: RedactionEditorDeps) {
  const { config, loading, getQuery, getTargetToken } = deps
  const customStringRows = useStoredPageSize(
    'redaction-custom-strings',
    10,
    STANDARD_PAGE_SIZE_OPTIONS,
  )
  const customStringFirst = ref(0)
  const customStringTotal = ref(0)
  const customStringSearch = ref('')
  const customStringPage = ref<RedactionCustomStringRuleRowSchema[]>([])
  const customStringDirty = ref(false)
  const customStringReveal = createRowRevealState()
  let pageRequest = 0

  function paginateCustomStrings(
    first: number,
    rows: number,
    search: string,
  ): { items: RedactionCustomStringRuleRowSchema[]; total: number } {
    const query = search.trim().toLowerCase()
    const all = config.value.custom_strings
      .map((rule, array_index) => ({
        array_index,
        pattern: rule.pattern,
        match_type: rule.match_type,
        scope: rule.scope,
        created_at: rule.created_at ?? null,
        updated_at: rule.updated_at ?? null,
      }))
      .filter((rule) =>
        !query ? true : rule.pattern.toLowerCase().includes(query),
      )
      .sort((left, right) => right.array_index - left.array_index)
    return { items: all.slice(first, first + rows), total: all.length }
  }

  const visibleCustomStrings = computed(() => {
    if (!customStringDirty.value) return customStringPage.value
    return paginateCustomStrings(
      customStringFirst.value,
      customStringRows.value,
      customStringSearch.value,
    ).items
  })
  const visibleCustomStringTotal = computed(() => {
    if (!customStringDirty.value) return customStringTotal.value
    return paginateCustomStrings(
      0,
      Number.MAX_SAFE_INTEGER,
      customStringSearch.value,
    ).total
  })
  const customStringWarnings = computed<CustomStringRuleWarning[]>(() =>
    findRuleWarnings(config.value.custom_strings),
  )
  const isDirty = computed(() => customStringDirty.value)

  function hasInvalidCustomStringRule(): boolean {
    return customStringWarnings.value.some((warning) => warning.invalidRegex)
  }

  function reflowCustomStringPage(): void {
    const total = paginateCustomStrings(
      0,
      Number.MAX_SAFE_INTEGER,
      customStringSearch.value,
    ).total
    if (total === 0) {
      customStringFirst.value = 0
    } else if (customStringFirst.value >= total) {
      customStringFirst.value =
        Math.floor((total - 1) / customStringRows.value) *
        customStringRows.value
    }
    customStringPage.value = paginateCustomStrings(
      customStringFirst.value,
      customStringRows.value,
      customStringSearch.value,
    ).items
    customStringTotal.value = total
  }

  async function loadCustomStringPage(): Promise<void> {
    if (customStringDirty.value) {
      customStringPage.value = visibleCustomStrings.value
      customStringTotal.value = visibleCustomStringTotal.value
      return
    }
    const request = ++pageRequest
    const targetToken = getTargetToken()
    const query = {
      ...getQuery(),
      first: customStringFirst.value,
      rows: customStringRows.value,
      search: customStringSearch.value || undefined,
    }
    const response = expectData(
      await listRedactionCustomStrings<true>(withData({ query })),
    )
    if (
      request !== pageRequest ||
      targetToken !== getTargetToken() ||
      query.first !== customStringFirst.value ||
      query.rows !== customStringRows.value ||
      (query.search ?? '') !== customStringSearch.value
    ) {
      return
    }
    customStringPage.value = response.items
    customStringTotal.value = response.total
    customStringFirst.value = response.first
    customStringRows.value = response.rows
    if (
      response.items.length === 0 &&
      response.total > 0 &&
      response.first >= response.total
    ) {
      customStringFirst.value =
        Math.floor((response.total - 1) / response.rows) * response.rows
      await loadCustomStringPage()
    }
  }

  async function withPageLoading(action: () => Promise<void>): Promise<void> {
    loading.value = true
    try {
      await action()
    } finally {
      loading.value = false
    }
  }

  function addCustomStringRule(): void {
    config.value.custom_strings.push({
      pattern: '',
      match_type: 'contains',
      scope: 'text',
    })
    customStringReveal.reveal(config.value.custom_strings.length - 1)
    customStringDirty.value = true
    customStringFirst.value = 0
    reflowCustomStringPage()
  }

  function updateCustomStringRule(
    arrayIndex: number,
    patch: Partial<RedactionConfigSchema['custom_strings'][number]>,
  ): void {
    if (!config.value.custom_strings[arrayIndex]) return
    config.value.custom_strings[arrayIndex] = {
      ...config.value.custom_strings[arrayIndex],
      ...patch,
    }
    customStringDirty.value = true
    reflowCustomStringPage()
  }

  function removeCustomStringRule(arrayIndex: number): void {
    customStringReveal.remove(arrayIndex)
    config.value.custom_strings.splice(arrayIndex, 1)
    customStringDirty.value = true
    reflowCustomStringPage()
  }

  function markSaved(): void {
    customStringDirty.value = false
    customStringReveal.clear()
  }

  async function setCustomStringPage(
    first: number,
    rows: number,
  ): Promise<void> {
    customStringFirst.value = first
    customStringRows.value = rows
    if (customStringDirty.value) {
      reflowCustomStringPage()
      return
    }
    await withPageLoading(loadCustomStringPage)
  }

  async function setCustomStringSearch(search: string): Promise<void> {
    customStringSearch.value = search
    customStringFirst.value = 0
    if (customStringDirty.value) {
      reflowCustomStringPage()
      return
    }
    await withPageLoading(loadCustomStringPage)
  }

  return {
    addCustomStringRule,
    customStringFirst,
    customStringRows,
    customStringSearch,
    customStringTotal: visibleCustomStringTotal,
    customStringWarnings,
    customStrings: visibleCustomStrings,
    hasInvalidCustomStringRule,
    isCustomStringRevealed: customStringReveal.isRevealed,
    isDirty,
    loadCustomStringPage,
    markSaved,
    normalizeConfig: normalizeRedactionConfig,
    removeCustomStringRule,
    setCustomStringPage,
    setCustomStringSearch,
    toggleCustomStringRevealed: customStringReveal.toggle,
    updateCustomStringRule,
  }
}

export { findRuleWarnings, validateCustomStringRule }
