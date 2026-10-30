import { create } from 'zustand'
import type {
  CustomStringRuleSchema,
  MeResponse,
  RedactionConfigSchema,
  RedactionCustomStringRulePageResponseSchema,
  RedactionCustomStringRuleRowSchema,
  RedactionInputKindSchema,
  RedactionPreviewSchema,
  RedactionScopeSchema,
  User,
} from '@/generated/admin-api'
import {
  getRedactionSetting,
  listRedactionCustomStrings,
  listUserOptions,
  previewRedaction,
  setRedactionSetting,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { createRedactionDefaults } from '@/admin-mappers/redaction'
import {
  findRuleWarnings,
  type CustomStringRuleWarning,
} from '@/redaction-rules'
import {
  normalizePageSize,
  pageSizeStorageKey,
  STANDARD_PAGE_SIZE_OPTIONS,
} from '@/models/pagination'
import { readStorage, writeStorage } from '@/storage'
import {
  getPendingActions,
  runAsyncAction,
  type AsyncActionOutcome,
} from './async-action'
import { useSessionStore } from './session'

export const REDACTION_RULES_KEY = 'redaction:rules'
export const REDACTION_PREVIEW_KEY = 'redaction:preview'
export const REDACTION_PAGE_SIZE_KEY = 'redaction-custom-strings'

export type RedactionRuleKey = keyof RedactionConfigSchema['rules']
export type RedactionTargetQuery = {
  scope: RedactionScopeSchema
  user_id?: number
}

type RedactionIdentity = Pick<MeResponse, 'user_id' | 'login_name' | 'is_admin'>

type RedactionState = {
  identityKey: string | null
  identityGeneration: number
  identityUserId: number | null
  isAdmin: boolean
  scope: RedactionScopeSchema
  targetUserId: number | null
  targetGeneration: number
  userOptions: User[]
  userOptionsLoading: boolean
  config: RedactionConfigSchema
  loadedTargetToken: string | null
  loading: boolean
  saving: boolean
  loadError: boolean
  pageError: boolean
  customStringPage: RedactionCustomStringRuleRowSchema[]
  customStringTotal: number
  customStringFirst: number
  customStringRows: number
  customStringSearch: string
  customStringDirty: boolean
  customStringRevealAll: boolean
  validationError: boolean
  previewText: string
  previewInputKind: RedactionInputKindSchema
  previewResult: RedactionPreviewSchema | null
  previewing: boolean
  initializeIdentity: (identity: RedactionIdentity) => void
  resetIdentity: (identity: RedactionIdentity | null) => void
  loadUserOptions: () => Promise<void>
  setTarget: (scope: RedactionScopeSchema, userId?: number | null) => void
  refresh: () => Promise<void>
  loadCustomStringPage: () => Promise<void>
  setCustomStringPage: (first: number, rows: number) => Promise<void>
  setCustomStringSearch: (search: string) => Promise<void>
  addCustomStringRule: () => void
  updateCustomStringRule: (
    arrayIndex: number,
    patch: Partial<CustomStringRuleSchema>,
  ) => void
  removeCustomStringRule: (arrayIndex: number) => void
  setCustomStringRevealAll: (revealed: boolean) => void
  setEnabled: (enabled: boolean) => void
  setBuiltinRule: (rule: RedactionRuleKey, enabled: boolean) => void
  save: () => Promise<AsyncActionOutcome<RedactionConfigSchema>>
  setPreviewText: (text: string) => void
  setPreviewInputKind: (kind: RedactionInputKindSchema) => void
  setPreviewResult: (result: RedactionPreviewSchema | null) => void
  runPreview: () => Promise<AsyncActionOutcome<RedactionPreviewSchema>>
}

function cloneConfig(config: RedactionConfigSchema): RedactionConfigSchema {
  return {
    ...config,
    rules: { ...config.rules },
    custom_strings: config.custom_strings.map((rule) => ({ ...rule })),
  }
}

export function normalizeRedactionConfig(
  config: RedactionConfigSchema,
): RedactionConfigSchema {
  const seen = new Set<string>()
  const customStrings: CustomStringRuleSchema[] = []
  for (const rule of config.custom_strings) {
    const pattern = rule.pattern.trim()
    if (!pattern) continue
    const signature = `${pattern}::${rule.match_type}::${rule.scope}`
    if (seen.has(signature)) continue
    seen.add(signature)
    customStrings.push({ ...rule, pattern })
  }
  return {
    ...config,
    rules: { ...config.rules },
    custom_strings: customStrings,
  }
}

export function redactionIdentityKey(
  identity: RedactionIdentity | null,
): string | null {
  if (!identity) return null
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin ? 'admin' : 'user'}`
}

export function redactionTargetToken(
  state: Pick<
    RedactionState,
    | 'identityKey'
    | 'identityGeneration'
    | 'scope'
    | 'targetUserId'
    | 'targetGeneration'
  >,
): string {
  return `${state.identityKey ?? ''}:${state.identityGeneration}:${state.targetGeneration}:${state.scope}:${state.targetUserId ?? ''}`
}

export function redactionTargetQuery(
  scope: RedactionScopeSchema,
  targetUserId: number | null,
): RedactionTargetQuery {
  return {
    scope,
    ...(scope === 'user' && targetUserId !== null
      ? { user_id: targetUserId }
      : {}),
  }
}

export function customStringRuleWarnings(
  config: RedactionConfigSchema,
): CustomStringRuleWarning[] {
  return findRuleWarnings(config.custom_strings)
}

export function localRedactionPage(options: {
  config: RedactionConfigSchema
  first: number
  rows: number
  search: string
}): {
  items: RedactionCustomStringRuleRowSchema[]
  first: number
  total: number
} {
  const query = options.search.trim().toLowerCase()
  const all = options.config.custom_strings
    .map((rule, array_index) => ({
      array_index,
      created_at: rule.created_at ?? null,
      match_type: rule.match_type,
      pattern: rule.pattern,
      scope: rule.scope,
      updated_at: rule.updated_at ?? null,
    }))
    .filter((rule) => !query || rule.pattern.toLowerCase().includes(query))
    .sort((left, right) => right.array_index - left.array_index)
  const first = clampFirst(options.first, options.rows, all.length)
  return {
    items: all.slice(first, first + options.rows),
    first,
    total: all.length,
  }
}

function clampFirst(first: number, rows: number, total: number): number {
  if (total === 0) return 0
  if (first >= total) return Math.floor((total - 1) / rows) * rows
  return Math.max(0, Math.floor(first / rows) * rows)
}

function sameConfig(
  left: RedactionConfigSchema,
  right: RedactionConfigSchema,
): boolean {
  if (left.enabled !== right.enabled) return false
  if (
    Object.keys(left.rules).some(
      (key) =>
        left.rules[key as keyof RedactionConfigSchema['rules']] !==
        right.rules[key as keyof RedactionConfigSchema['rules']],
    )
  ) {
    return false
  }
  if (left.custom_strings.length !== right.custom_strings.length) return false
  return left.custom_strings.every((rule, index) => {
    const other = right.custom_strings[index]
    return Boolean(
      other &&
      rule.pattern === other.pattern &&
      rule.match_type === other.match_type &&
      rule.scope === other.scope &&
      rule.created_at === other.created_at &&
      rule.updated_at === other.updated_at,
    )
  })
}

const pageSizeKey = pageSizeStorageKey(REDACTION_PAGE_SIZE_KEY)

function initialPageSize(): number {
  return normalizePageSize(
    readStorage(pageSizeKey),
    10,
    STANDARD_PAGE_SIZE_OPTIONS,
  )
}

function emptyDraft() {
  return createRedactionDefaults()
}

let settingsRequestId = 0
let pageRequestId = 0
let userOptionsRequestId = 0

function pageMatchesCurrent(
  token: string,
  first: number,
  rows: number,
  search: string,
): boolean {
  const state = useRedactionStore.getState()
  return (
    token === redactionTargetToken(state) &&
    first === state.customStringFirst &&
    rows === state.customStringRows &&
    search === state.customStringSearch.trim()
  )
}

async function fetchCustomStringPage(token: string): Promise<void> {
  const state = useRedactionStore.getState()
  const requestId = ++pageRequestId
  const first = state.customStringFirst
  const rows = state.customStringRows
  const search = state.customStringSearch.trim()
  const query = {
    ...redactionTargetQuery(state.scope, state.targetUserId),
    first,
    rows,
    ...(search ? { search } : {}),
  }

  useRedactionStore.setState({ loading: true, pageError: false })
  try {
    const response = expectData(
      await listRedactionCustomStrings<true>(withData({ query })),
    ) as RedactionCustomStringRulePageResponseSchema
    const latest = useRedactionStore.getState()
    if (
      requestId !== pageRequestId ||
      !pageMatchesCurrent(token, first, rows, search) ||
      latest.customStringDirty
    ) {
      return
    }

    if (
      response.items.length === 0 &&
      response.total > 0 &&
      response.first >= response.total
    ) {
      const correctedFirst = clampFirst(
        response.first,
        response.rows,
        response.total,
      )
      if (correctedFirst !== response.first) {
        useRedactionStore.setState({ customStringFirst: correctedFirst })
        await fetchCustomStringPage(token)
        return
      }
    }

    useRedactionStore.setState({
      customStringFirst: response.first,
      customStringRows: response.rows,
      customStringPage: response.items,
      customStringTotal: response.total,
    })
  } catch (cause) {
    if (
      requestId === pageRequestId &&
      pageMatchesCurrent(token, first, rows, search)
    ) {
      useRedactionStore.setState({ pageError: true })
    }
    throw cause
  } finally {
    if (requestId === pageRequestId) {
      const latest = useRedactionStore.getState()
      if (token === redactionTargetToken(latest)) {
        useRedactionStore.setState({ loading: false })
      }
    }
  }
}

export const useRedactionStore = create<RedactionState>((set, get) => ({
  identityKey: null,
  identityGeneration: 0,
  identityUserId: null,
  isAdmin: false,
  scope: 'user',
  targetUserId: null,
  targetGeneration: 0,
  userOptions: [],
  userOptionsLoading: false,
  config: emptyDraft(),
  loadedTargetToken: null,
  loading: false,
  saving: false,
  loadError: false,
  pageError: false,
  customStringPage: [],
  customStringTotal: 0,
  customStringFirst: 0,
  customStringRows: initialPageSize(),
  customStringSearch: '',
  customStringDirty: false,
  customStringRevealAll: false,
  validationError: false,
  previewText: '',
  previewInputKind: 'text',
  previewResult: null,
  previewing: false,

  initializeIdentity(identity) {
    const key = redactionIdentityKey(identity)
    if (get().identityKey === key) return
    get().resetIdentity(identity)
  },

  resetIdentity(identity) {
    const key = redactionIdentityKey(identity)
    const generation = get().identityGeneration + 1
    ++settingsRequestId
    ++pageRequestId
    ++userOptionsRequestId
    set({
      identityKey: key,
      identityGeneration: generation,
      identityUserId: identity?.user_id ?? null,
      isAdmin: identity?.is_admin === true,
      scope: identity?.is_admin ? 'global' : 'user',
      targetUserId: identity?.is_admin ? null : (identity?.user_id ?? null),
      targetGeneration: get().targetGeneration + 1,
      userOptions: [],
      userOptionsLoading: false,
      config: emptyDraft(),
      loadedTargetToken: null,
      loading: identity !== null,
      saving: false,
      loadError: false,
      pageError: false,
      customStringPage: [],
      customStringTotal: 0,
      customStringFirst: 0,
      customStringRows: initialPageSize(),
      customStringSearch: '',
      customStringDirty: false,
      customStringRevealAll: false,
      validationError: false,
      previewText: '',
      previewInputKind: 'text',
      previewResult: null,
      previewing: false,
    })
  },

  async loadUserOptions() {
    const state = get()
    if (!state.isAdmin || state.identityKey === null) return
    const identityKey = state.identityKey
    const identityGeneration = state.identityGeneration
    const requestId = ++userOptionsRequestId
    set({ userOptionsLoading: true })
    try {
      const response = expectData(await listUserOptions<true>(withData()))
      const latest = get()
      if (
        requestId !== userOptionsRequestId ||
        latest.identityKey !== identityKey ||
        latest.identityGeneration !== identityGeneration ||
        !latest.isAdmin
      ) {
        return
      }
      const users = response.users
      const currentTarget = latest.targetUserId
      const selectedTarget = users.some(
        (user) => user.user_id === currentTarget,
      )
        ? currentTarget
        : (users[0]?.user_id ?? latest.identityUserId)
      set({ userOptions: users })
      if (
        latest.scope === 'user' &&
        selectedTarget !== null &&
        selectedTarget !== currentTarget
      ) {
        get().setTarget('user', selectedTarget)
      }
    } finally {
      if (requestId === userOptionsRequestId) set({ userOptionsLoading: false })
    }
  },

  setTarget(scope, userId) {
    const state = get()
    if (state.identityKey === null) return
    if (!state.isAdmin && scope !== 'user') return
    const nextUserId =
      scope === 'user'
        ? state.isAdmin
          ? (userId ?? state.targetUserId ?? state.identityUserId)
          : state.identityUserId
        : null
    if (scope === 'user' && nextUserId === null) return
    if (state.scope === scope && state.targetUserId === nextUserId) return
    ++pageRequestId
    set({
      scope,
      targetUserId: nextUserId,
      targetGeneration: state.targetGeneration + 1,
      config: emptyDraft(),
      loadedTargetToken: null,
      loading: false,
      loadError: false,
      pageError: false,
      customStringPage: [],
      customStringTotal: 0,
      customStringFirst: 0,
      customStringDirty: false,
      customStringRevealAll: false,
      validationError: false,
      previewResult: null,
    })
  },

  async refresh() {
    const state = get()
    if (state.identityKey === null || state.saving) return
    const token = redactionTargetToken(state)
    const requestId = ++settingsRequestId
    set({ loading: true, loadError: false, pageError: false })
    try {
      const response = expectData(
        await getRedactionSetting<true>(
          withData({
            query: redactionTargetQuery(state.scope, state.targetUserId),
          }),
        ),
      )
      const latest = get()
      if (
        requestId !== settingsRequestId ||
        token !== redactionTargetToken(latest)
      )
        return
      const config = normalizeRedactionConfig(
        createRedactionDefaults(response.config),
      )
      set({
        config,
        scope: response.scope,
        targetUserId:
          response.scope === 'global'
            ? null
            : (response.user_id ?? latest.targetUserId),
        loadedTargetToken: token,
        customStringDirty: false,
        customStringRevealAll: false,
        validationError: false,
        customStringPage: [],
        customStringTotal: 0,
      })
      await fetchCustomStringPage(token)
    } catch (cause) {
      const latest = get()
      if (
        requestId === settingsRequestId &&
        token === redactionTargetToken(latest)
      ) {
        set({ loadError: true })
      }
      throw cause
    } finally {
      const latest = get()
      if (
        requestId === settingsRequestId &&
        token === redactionTargetToken(latest)
      ) {
        set({ loading: false })
      }
    }
  },

  async loadCustomStringPage() {
    const state = get()
    if (
      state.identityKey === null ||
      state.loadedTargetToken !== redactionTargetToken(state) ||
      state.customStringDirty
    ) {
      return
    }
    await fetchCustomStringPage(redactionTargetToken(state))
  },

  async setCustomStringPage(first, rows) {
    if (!STANDARD_PAGE_SIZE_OPTIONS.includes(rows)) return
    const state = get()
    const nextFirst = Math.max(0, Math.floor(first / rows) * rows)
    writeStorage(pageSizeKey, String(rows))
    set({ customStringFirst: nextFirst, customStringRows: rows })
    if (state.customStringDirty) return
    await get().loadCustomStringPage()
  },

  async setCustomStringSearch(search) {
    const state = get()
    set({ customStringSearch: search, customStringFirst: 0 })
    if (state.customStringDirty) return
    await get().loadCustomStringPage()
  },

  addCustomStringRule() {
    const state = get()
    const customStrings: RedactionConfigSchema['custom_strings'] = [
      ...state.config.custom_strings,
      { pattern: '', match_type: 'contains', scope: 'text' },
    ]
    const config = { ...state.config, custom_strings: customStrings }
    const local = localRedactionPage({
      config,
      first: 0,
      rows: state.customStringRows,
      search: state.customStringSearch,
    })
    set({
      config,
      customStringDirty: true,
      customStringFirst: 0,
      customStringPage: local.items,
      customStringTotal: local.total,
      validationError: false,
    })
  },

  updateCustomStringRule(arrayIndex, patch) {
    const state = get()
    if (!state.config.custom_strings[arrayIndex]) return
    const customStrings = state.config.custom_strings.map((rule, index) =>
      index === arrayIndex ? { ...rule, ...patch } : rule,
    )
    const config = { ...state.config, custom_strings: customStrings }
    const local = localRedactionPage({
      config,
      first: state.customStringFirst,
      rows: state.customStringRows,
      search: state.customStringSearch,
    })
    set({
      config,
      customStringDirty: true,
      customStringFirst: local.first,
      customStringPage: local.items,
      customStringTotal: local.total,
      validationError: false,
    })
  },

  removeCustomStringRule(arrayIndex) {
    const state = get()
    if (!state.config.custom_strings[arrayIndex]) return
    const customStrings = state.config.custom_strings.filter(
      (_rule, index) => index !== arrayIndex,
    )
    const config = { ...state.config, custom_strings: customStrings }
    const local = localRedactionPage({
      config,
      first: state.customStringFirst,
      rows: state.customStringRows,
      search: state.customStringSearch,
    })
    set({
      config,
      customStringDirty: true,
      customStringFirst: local.first,
      customStringPage: local.items,
      customStringTotal: local.total,
      validationError: false,
    })
  },

  setCustomStringRevealAll(revealed) {
    set({ customStringRevealAll: revealed })
  },

  setEnabled(enabled) {
    set((state) => ({ config: { ...state.config, enabled } }))
  },

  setBuiltinRule(rule, enabled) {
    set((state) => ({
      config: {
        ...state.config,
        rules: { ...state.config.rules, [rule]: enabled },
      },
    }))
  },

  async save() {
    const state = get()
    if (
      state.identityKey === null ||
      state.loadedTargetToken !== redactionTargetToken(state) ||
      state.saving ||
      Object.hasOwn(getPendingActions(), REDACTION_RULES_KEY)
    ) {
      return { executed: false }
    }
    const warnings = customStringRuleWarnings(state.config)
    if (warnings.some((warning) => warning.invalidRegex)) {
      set({ validationError: true })
      return { executed: false }
    }

    const submitted = normalizeRedactionConfig(state.config)
    const customNormalized =
      JSON.stringify(state.config.custom_strings) !==
      JSON.stringify(submitted.custom_strings)
    const token = redactionTargetToken(state)
    const targetQuery = redactionTargetQuery(state.scope, state.targetUserId)
    const identityKey = state.identityKey
    const identityGeneration = state.identityGeneration
    set({
      config: submitted,
      customStringDirty: state.customStringDirty || customNormalized,
      validationError: false,
    })

    const outcome = await runAsyncAction(
      REDACTION_RULES_KEY,
      async () => {
        set({ saving: true })
        try {
          const response = expectData(
            await setRedactionSetting<true>(
              withData({ query: targetQuery, body: cloneConfig(submitted) }),
            ),
          )
          const latest = get()
          if (
            latest.identityKey === identityKey &&
            latest.identityGeneration === identityGeneration &&
            token === redactionTargetToken(latest)
          ) {
            // Re-mask on save success even when a newer edit kept the response
            // config out, so a revealed row cannot be mistaken for another one.
            set(
              sameConfig(latest.config, submitted)
                ? {
                    config: normalizeRedactionConfig(response.config),
                    customStringDirty: false,
                    customStringRevealAll: false,
                  }
                : { customStringRevealAll: false },
            )
          }
          const afterWrite = get()
          if (
            afterWrite.identityKey === identityKey &&
            afterWrite.identityGeneration === identityGeneration &&
            token === redactionTargetToken(afterWrite)
          ) {
            await fetchCustomStringPage(token)
          }
          return normalizeRedactionConfig(response.config)
        } finally {
          const latest = get()
          if (
            latest.identityKey === identityKey &&
            latest.identityGeneration === identityGeneration
          ) {
            set({ saving: false })
          }
        }
      },
      'save',
    )
    return outcome
  },

  setPreviewText(previewText) {
    set({ previewText })
  },

  setPreviewInputKind(previewInputKind) {
    set({ previewInputKind })
  },

  setPreviewResult(previewResult) {
    set({ previewResult })
  },

  async runPreview() {
    return runAsyncAction(
      REDACTION_PREVIEW_KEY,
      async () => {
        const state = get()
        const targetToken = redactionTargetToken(state)
        const configSnapshot = cloneConfig(state.config)
        const textSnapshot = state.previewText
        const kindSnapshot = state.previewInputKind
        const identityKey = state.identityKey
        const identityGeneration = state.identityGeneration
        const body = {
          enabled: configSnapshot.enabled,
          custom_strings: configSnapshot.custom_strings,
          input_kind: kindSnapshot,
          rules: configSnapshot.rules,
          text: textSnapshot,
        }
        set({ previewResult: null, previewing: true })

        try {
          const response = expectData(
            await previewRedaction<true>(withData({ body })),
          )
          const latest = get()
          if (
            targetToken === redactionTargetToken(latest) &&
            JSON.stringify(latest.config) === JSON.stringify(configSnapshot) &&
            latest.previewText === textSnapshot &&
            latest.previewInputKind === kindSnapshot
          ) {
            set({ previewResult: response.preview })
          }
          return response.preview
        } finally {
          const latest = get()
          if (
            latest.identityKey === identityKey &&
            latest.identityGeneration === identityGeneration
          ) {
            set({ previewing: false })
          }
        }
      },
      'preview',
    )
  },
}))

useSessionStore.subscribe((state, previous) => {
  const currentKey = redactionIdentityKey(state.me)
  const previousKey = redactionIdentityKey(previous.me)
  if (currentKey !== previousKey) {
    useRedactionStore.getState().resetIdentity(state.me)
  }
})

export function visibleRedactionRows(
  state: Pick<
    RedactionState,
    | 'config'
    | 'customStringDirty'
    | 'customStringFirst'
    | 'customStringRows'
    | 'customStringSearch'
    | 'customStringPage'
  >,
): RedactionCustomStringRuleRowSchema[] {
  if (!state.customStringDirty) return state.customStringPage
  return localRedactionPage({
    config: state.config,
    first: state.customStringFirst,
    rows: state.customStringRows,
    search: state.customStringSearch,
  }).items
}

export function visibleRedactionTotal(
  state: Pick<
    RedactionState,
    'config' | 'customStringDirty' | 'customStringSearch' | 'customStringTotal'
  >,
): number {
  if (!state.customStringDirty) return state.customStringTotal
  return localRedactionPage({
    config: state.config,
    first: 0,
    rows: Number.MAX_SAFE_INTEGER,
    search: state.customStringSearch,
  }).total
}

export type { CustomStringRuleWarning }
