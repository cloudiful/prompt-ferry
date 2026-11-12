import { type QueryClient, type QueryKey } from '@tanstack/react-query'
import type {
  CustomStringRuleSchema,
  MeResponse,
  RedactionConfigSchema,
  RedactionCustomStringRulePageResponseSchema,
  RedactionCustomStringRuleRowSchema,
  RedactionInputKindSchema,
  RedactionPreviewSchema,
  RedactionScopeSchema,
  GetRedactionSettingResponse,
} from '@/generated/admin-api'
import {
  getRedactionSettingOptions,
  getRedactionSettingQueryKey,
  listRedactionCustomStringsOptions,
  listRedactionCustomStringsQueryKey,
} from '@/generated/admin-api/@tanstack/react-query.gen'
import { createRedactionDefaults } from '@/admin-mappers/redaction'
import { useSessionStore } from '@/lib/session'
import type { CustomStringRuleWarning } from '@/redaction-rules'
import { findRuleWarnings } from '@/redaction-rules'

export const REDACTION_RULES_KEY = 'redaction:rules'
export const REDACTION_PREVIEW_KEY = 'redaction:preview'
export const REDACTION_PAGE_SIZE_KEY = 'redaction-custom-strings'

export type RedactionRuleKey = keyof RedactionConfigSchema['rules']

export type RedactionTargetQuery = {
  scope: RedactionScopeSchema
  user_id?: number
}

export type RedactionPreviewFormValues = {
  input_kind: RedactionInputKindSchema
  text: string
}

type RedactionIdentity = Pick<MeResponse, 'user_id' | 'login_name' | 'is_admin'>

export function redactionIdentityKey(
  identity: RedactionIdentity | null,
): string | null {
  if (!identity) return null
  return `${identity.user_id}:${identity.login_name}:${identity.is_admin ? 'admin' : 'user'}`
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

export type RedactionRetainedDraft = {
  identityKey: string
  epoch: number
  scope: RedactionScopeSchema
  targetUserId: number | null
  first: number
  search: string
  loaded: boolean
  config: RedactionConfigSchema
  customStringDirty: boolean
  customStringRevealAll: boolean
  validationError: boolean
  previewText: string
  previewInputKind: RedactionInputKindSchema
  previewResult: RedactionPreviewSchema | null
}

let retainedRedactionDraft: RedactionRetainedDraft | null = null

/**
 * The epoch of the session that is authenticated right now.
 *
 * A mounted component keeps the epoch it was mounted under as the owner of
 * its own draft, but only this live value decides whether that draft still
 * belongs to the current session: a component that a logout unmounted never
 * re-renders, so the epoch it captured is stale by the time its cleanup runs.
 */
function currentSessionEpoch(): number {
  return useSessionStore.getState().epoch
}

export function readRetainedRedactionDraft(
  identityKey: string | null,
): RedactionRetainedDraft | null {
  if (
    identityKey === null ||
    retainedRedactionDraft === null ||
    retainedRedactionDraft.identityKey !== identityKey ||
    retainedRedactionDraft.epoch !== currentSessionEpoch()
  ) {
    // A retired session's snapshot must never come back: drop it instead of
    // serving it to the next login, same principal or not.
    retainedRedactionDraft = null
    return null
  }
  return retainedRedactionDraft
}

export function retainRedactionDraft(draft: RedactionRetainedDraft): void {
  // Only the live epoch may write; a cleanup closure that runs after its
  // session ended cannot resurrect its draft.
  const liveEpoch = currentSessionEpoch()
  if (draft.epoch !== liveEpoch) {
    if (retainedRedactionDraft?.epoch === liveEpoch) return
    retainedRedactionDraft = null
    return
  }
  retainedRedactionDraft = draft
}

export function clearRetainedRedactionDraft(): void {
  if (retainedRedactionDraft === null) return
  if (retainedRedactionDraft.epoch !== currentSessionEpoch()) {
    retainedRedactionDraft = null
  }
}

export function cloneRedactionConfig(
  config: RedactionConfigSchema,
): RedactionConfigSchema {
  return {
    ...config,
    rules: { ...config.rules },
    custom_strings: config.custom_strings.map((rule) => ({ ...rule })),
  }
}

export function emptyRedactionDraft(): RedactionConfigSchema {
  return createRedactionDefaults()
}

export function defaultRedactionPreviewForm(): RedactionPreviewFormValues {
  return { input_kind: 'text', text: '' }
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

export function sameRedactionConfig(
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

export function redactionSaveBody(
  config: RedactionConfigSchema,
): RedactionConfigSchema {
  return cloneRedactionConfig(normalizeRedactionConfig(config))
}

export function redactionPreviewBody(
  config: RedactionConfigSchema,
  form: RedactionPreviewFormValues,
) {
  const body = cloneRedactionConfig(config)
  return {
    custom_strings: body.custom_strings,
    enabled: body.enabled,
    input_kind: form.input_kind,
    rules: body.rules,
    text: form.text,
  }
}

export function customStringRuleWarnings(
  config: RedactionConfigSchema,
): CustomStringRuleWarning[] {
  return findRuleWarnings(config.custom_strings)
}

export function clampRedactionPageFirst(
  first: number,
  rows: number,
  total: number,
): number {
  if (total === 0) return 0
  if (first >= total) return Math.floor((total - 1) / rows) * rows
  return Math.max(0, Math.floor(first / rows) * rows)
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
  const first = clampRedactionPageFirst(options.first, options.rows, all.length)
  return {
    items: all.slice(first, first + options.rows),
    first,
    total: all.length,
  }
}

export function redactionCustomStringsQuery(
  target: RedactionTargetQuery,
  first: number,
  rows: number,
  search: string,
): RedactionTargetQuery & { first: number; rows: number; search?: string } {
  const trimmed = search.trim()
  return {
    ...target,
    first,
    rows,
    ...(trimmed ? { search: trimmed } : {}),
  }
}

export function redactionCustomStringsOptions(
  target: RedactionTargetQuery,
  first: number,
  rows: number,
  search: string,
) {
  return listRedactionCustomStringsOptions({
    query: redactionCustomStringsQuery(target, first, rows, search),
  })
}

export function redactionSettingOptions(target: RedactionTargetQuery) {
  return getRedactionSettingOptions({ query: target })
}

export function redactionPageRows(
  state: {
    config: RedactionConfigSchema
    dirty: boolean
    first: number
    rows: number
    search: string
  },
  page: RedactionCustomStringRulePageResponseSchema | undefined,
): RedactionCustomStringRuleRowSchema[] {
  if (!state.dirty && page) return page.items
  return localRedactionPage(state).items
}

export function redactionPageTotal(
  state: {
    config: RedactionConfigSchema
    dirty: boolean
    search: string
  },
  page: RedactionCustomStringRulePageResponseSchema | undefined,
): number {
  if (!state.dirty && page) return page.total
  return localRedactionPage({
    config: state.config,
    first: 0,
    rows: Number.MAX_SAFE_INTEGER,
    search: state.search,
  }).total
}

async function fetchFresh<T>(
  queryClient: QueryClient,
  queryKey: QueryKey,
  options: Record<string, unknown>,
): Promise<T> {
  await queryClient.cancelQueries({ queryKey, exact: true })
  return queryClient.fetchQuery(options as never) as Promise<T>
}

export async function refreshRedactionSetting(
  queryClient: QueryClient,
  target: RedactionTargetQuery,
): Promise<GetRedactionSettingResponse> {
  return fetchFresh<GetRedactionSettingResponse>(
    queryClient,
    getRedactionSettingQueryKey({ query: target }),
    {
      ...redactionSettingOptions(target),
      staleTime: 0,
    },
  )
}

export async function refreshRedactionCustomStrings(
  queryClient: QueryClient,
  target: RedactionTargetQuery,
  first: number,
  rows: number,
  search: string,
): Promise<RedactionCustomStringRulePageResponseSchema> {
  let requestedFirst = first
  while (true) {
    const query = redactionCustomStringsQuery(
      target,
      requestedFirst,
      rows,
      search,
    )
    const page = await fetchFresh<RedactionCustomStringRulePageResponseSchema>(
      queryClient,
      listRedactionCustomStringsQueryKey({ query }),
      {
        ...listRedactionCustomStringsOptions({ query }),
        staleTime: 0,
      },
    )
    const pageRows = page.rows > 0 ? page.rows : rows
    if (
      pageRows > 0 &&
      page.items.length === 0 &&
      page.total > 0 &&
      requestedFirst >= page.total
    ) {
      const corrected = clampRedactionPageFirst(
        page.total,
        pageRows,
        page.total,
      )
      if (corrected !== requestedFirst) {
        requestedFirst = corrected
        continue
      }
    }
    return page
  }
}

export type { CustomStringRuleWarning }
