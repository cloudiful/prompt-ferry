import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import {
  STANDARD_PAGE_SIZE_OPTIONS,
  useStoredPageSize,
} from '../table-pagination'
import {
  getRedactionSetting,
  listRedactionCustomStrings,
  previewRedaction,
  setRedactionSetting,
} from '../generated/admin-api'
import type {
  RedactionConfigSchema,
  RedactionCustomStringRuleRowSchema,
  RedactionInputKindSchema,
  RedactionPreviewSchema,
  RedactionScopeSchema,
} from '../generated/admin-api'
import { expectData, withData } from '../api'
import { createRedactionDefaults } from '../admin-mappers'

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
    normalized.push({
      pattern,
      match_type: rule.match_type,
      scope: rule.scope,
    })
  }
  return normalized
}

function normalizeConfig(
  nextConfig: RedactionConfigSchema,
): RedactionConfigSchema {
  return {
    ...nextConfig,
    custom_strings: normalizeCustomStrings(nextConfig.custom_strings),
  }
}

type LocalCustomStringRow = RedactionCustomStringRuleRowSchema

function paginateCustomStrings(
  config: RedactionConfigSchema,
  first: number,
  rows: number,
  search: string,
): { items: LocalCustomStringRow[]; total: number } {
  const query = search.trim().toLowerCase()
  const all = config.custom_strings
    .map((rule, array_index) => ({
      array_index,
      pattern: rule.pattern,
      match_type: rule.match_type,
      scope: rule.scope,
    }))
    .filter((rule) =>
      !query ? true : rule.pattern.toLowerCase().includes(query),
    )
    .sort((left, right) => right.array_index - left.array_index)
  return {
    items: all.slice(first, first + rows),
    total: all.length,
  }
}

export const useRedactionStore = defineStore('redaction', () => {
  const loading = ref(false)
  const config = ref<RedactionConfigSchema>(createRedactionDefaults())
  const scope = ref<RedactionScopeSchema>('user')
  const targetUserId = ref<number | null>(null)
  const previewText = ref('')
  const previewInputKind = ref<RedactionInputKindSchema>('text')
  const previewResult = ref<RedactionPreviewSchema | null>(null)
  const customStringRows = useStoredPageSize(
    'redaction-custom-strings',
    10,
    STANDARD_PAGE_SIZE_OPTIONS,
  )
  const customStringFirst = ref(0)
  const customStringTotal = ref(0)
  const customStringUpdatedAt = ref<string | null>(null)
  const customStringSearch = ref('')
  const customStringPage = ref<RedactionCustomStringRuleRowSchema[]>([])
  const customStringDirty = ref(false)

  function query() {
    return {
      scope: scope.value,
      ...(scope.value === 'user' && targetUserId.value
        ? { user_id: targetUserId.value }
        : {}),
    }
  }

  const visibleCustomStrings = computed(() => {
    if (!customStringDirty.value) return customStringPage.value
    return paginateCustomStrings(
      config.value,
      customStringFirst.value,
      customStringRows.value,
      customStringSearch.value,
    ).items
  })

  const visibleCustomStringTotal = computed(() => {
    if (!customStringDirty.value) return customStringTotal.value
    return paginateCustomStrings(
      config.value,
      0,
      Number.MAX_SAFE_INTEGER,
      customStringSearch.value,
    ).total
  })

  function setTarget(nextScope: RedactionScopeSchema, userId?: number | null) {
    scope.value = nextScope
    targetUserId.value = userId ?? null
  }

  async function loadCustomStringPage(): Promise<void> {
    if (customStringDirty.value) {
      customStringPage.value = visibleCustomStrings.value
      customStringTotal.value = visibleCustomStringTotal.value
      return
    }
    const response = expectData(
      await listRedactionCustomStrings<true>(
        withData({
          query: {
            ...query(),
            first: customStringFirst.value,
            rows: customStringRows.value,
            search: customStringSearch.value || undefined,
          },
        }),
      ),
    )
    customStringPage.value = response.items
    customStringTotal.value = response.total
    customStringFirst.value = response.first
    customStringRows.value = response.rows
    customStringUpdatedAt.value = response.updated_at ?? null
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

  async function refresh(): Promise<void> {
    loading.value = true
    try {
      const response = expectData(
        await getRedactionSetting<true>(withData({ query: query() })),
      )
      scope.value = response.scope
      targetUserId.value = response.user_id ?? targetUserId.value
      config.value = normalizeConfig(createRedactionDefaults(response.config))
      customStringDirty.value = false
      await loadCustomStringPage()
    } finally {
      loading.value = false
    }
  }

  async function save(): Promise<void> {
    loading.value = true
    try {
      config.value = normalizeConfig(config.value)
      const response = expectData(
        await setRedactionSetting<true>(
          withData({ query: query(), body: config.value }),
        ),
      )
      scope.value = response.scope
      targetUserId.value = response.user_id ?? targetUserId.value
      config.value = normalizeConfig(createRedactionDefaults(response.config))
      customStringDirty.value = false
      await loadCustomStringPage()
    } finally {
      loading.value = false
    }
  }

  async function runPreview(): Promise<void> {
    loading.value = true
    try {
      config.value = normalizeConfig(config.value)
      const response = expectData(
        await previewRedaction<true>(
          withData({
            body: {
              enabled: config.value.enabled,
              custom_strings: config.value.custom_strings,
              input_kind: previewInputKind.value,
              rules: config.value.rules,
              text: previewText.value,
            },
          }),
        ),
      )
      previewResult.value = response.preview
    } finally {
      loading.value = false
    }
  }

  function reflowCustomStringPage(): void {
    const total = paginateCustomStrings(
      config.value,
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
      config.value,
      customStringFirst.value,
      customStringRows.value,
      customStringSearch.value,
    ).items
    customStringTotal.value = total
  }

  function addCustomStringRule(): void {
    config.value.custom_strings.push({
      pattern: '',
      match_type: 'contains',
      scope: 'text',
    })
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
    config.value.custom_strings.splice(arrayIndex, 1)
    customStringDirty.value = true
    reflowCustomStringPage()
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
    loading.value = true
    try {
      await loadCustomStringPage()
    } finally {
      loading.value = false
    }
  }

  async function setCustomStringSearch(search: string): Promise<void> {
    customStringSearch.value = search
    customStringFirst.value = 0
    if (customStringDirty.value) {
      reflowCustomStringPage()
      return
    }
    loading.value = true
    try {
      await loadCustomStringPage()
    } finally {
      loading.value = false
    }
  }

  return {
    addCustomStringRule,
    config,
    customStringFirst,
    customStringRows,
    customStringSearch,
    customStringTotal: visibleCustomStringTotal,
    customStringUpdatedAt,
    customStrings: visibleCustomStrings,
    loading,
    previewInputKind,
    previewResult,
    previewText,
    refresh,
    removeCustomStringRule,
    runPreview,
    save,
    scope,
    setCustomStringPage,
    setCustomStringSearch,
    setTarget,
    targetUserId,
    updateCustomStringRule,
  }
})
