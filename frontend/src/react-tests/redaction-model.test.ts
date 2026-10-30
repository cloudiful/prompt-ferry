import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type {
  RedactionConfigSchema,
  RedactionCustomStringRulePageResponseSchema,
  RedactionPreviewResponseSchema,
  RedactionPreviewSchema,
  RedactionSettingResponseSchema,
} from '@/generated/admin-api'

const api = vi.hoisted(() => ({
  authLogin: vi.fn(),
  authLogout: vi.fn(),
  authMe: vi.fn(),
  bridgeStatus: vi.fn(),
  relayAuthLogin: vi.fn(),
  relayAuthLogout: vi.fn(),
  relayAuthMe: vi.fn(),
  getRedactionSetting: vi.fn(),
  listRedactionCustomStrings: vi.fn(),
  listUserOptions: vi.fn(),
  previewRedaction: vi.fn(),
  setRedactionSetting: vi.fn(),
}))

vi.mock('@/generated/admin-api', () => api)

import { useRedactionStore, redactionTargetToken } from '@/lib/redaction'
import {
  redactionInputSegments,
  redactionOutputSegments,
} from '@/lib/redaction-preview'
import { useSessionStore } from '@/lib/session'

const admin = { user_id: 1, login_name: 'admin', is_admin: true }
const operator = { user_id: 2, login_name: 'operator', is_admin: false }

function config(
  customStrings: RedactionConfigSchema['custom_strings'] = [],
): RedactionConfigSchema {
  return {
    custom_strings: customStrings,
    enabled: true,
    rules: {
      cidr: true,
      domain: true,
      email: true,
      ip: true,
      organization: false,
      person: false,
      phone: true,
      secret: true,
      url: true,
    },
  }
}

function setting(
  overrides: Partial<RedactionSettingResponseSchema> = {},
): RedactionSettingResponseSchema {
  return {
    config: config(),
    scope: 'global',
    user_id: null,
    ...overrides,
  }
}

function page(
  overrides: Partial<RedactionCustomStringRulePageResponseSchema> = {},
): RedactionCustomStringRulePageResponseSchema {
  return { first: 0, items: [], rows: 10, total: 0, ...overrides }
}

function preview(): RedactionPreviewSchema {
  return {
    applied_replacements: [],
    findings: [],
    redacted_text: 'fixture output',
    stats: {
      applied_replacements: 0,
      dropped_findings: 0,
      llm_candidates_accepted: 0,
      llm_candidates_rejected: 0,
      llm_configured: false,
      llm_request_failed: false,
      total_findings: 0,
    },
  }
}

function readyAdmin(configValue = config()): void {
  const store = useRedactionStore.getState()
  store.resetIdentity(admin)
  const state = useRedactionStore.getState()
  useRedactionStore.setState({
    config: configValue,
    loadedTargetToken: redactionTargetToken(state),
  })
}

beforeEach(() => {
  localStorage.clear()
  vi.clearAllMocks()
  useSessionStore.setState({
    me: null,
    bootstrapped: true,
    bootstrapping: false,
    error: '',
  })
  useRedactionStore.getState().resetIdentity(null)
  api.listRedactionCustomStrings.mockResolvedValue(page())
  api.setRedactionSetting.mockImplementation(async (options) =>
    setting({
      config: options.body,
      scope: options.query.scope,
      user_id: options.query.user_id ?? null,
    }),
  )
  api.previewRedaction.mockImplementation(async () => ({ preview: preview() }))
})

afterEach(() => {
  useSessionStore.setState({ me: null })
  useRedactionStore.getState().resetIdentity(null)
  localStorage.clear()
})

describe('redaction rules model', () => {
  it('normalizes empty and identical signatures but keeps conflicting signatures', async () => {
    const { normalizeRedactionConfig, customStringRuleWarnings } =
      await import('@/lib/redaction')
    const source = config([
      { pattern: '  token  ', match_type: 'contains', scope: 'text' },
      { pattern: 'token', match_type: 'contains', scope: 'text' },
      { pattern: 'token', match_type: 'regex', scope: 'line' },
      { pattern: '  ', match_type: 'regex', scope: 'text' },
    ])
    const normalized = normalizeRedactionConfig(source)
    const warnings = customStringRuleWarnings(source)

    expect(normalized.custom_strings).toEqual([
      { pattern: 'token', match_type: 'contains', scope: 'text' },
      { pattern: 'token', match_type: 'regex', scope: 'line' },
    ])
    expect(warnings[0]).toMatchObject({ duplicate: true, conflict: false })
    expect(warnings[2]).toMatchObject({ duplicate: false, conflict: true })
    expect(warnings[3]?.invalidRegex).toBe(false)
  })

  it('pages dirty drafts by descending array index and clamps the final page', async () => {
    const { localRedactionPage } = await import('@/lib/redaction')
    const source = config([
      { pattern: 'first fixture', match_type: 'contains', scope: 'text' },
      { pattern: 'second fixture', match_type: 'contains', scope: 'text' },
      { pattern: 'third fixture', match_type: 'contains', scope: 'text' },
    ])

    expect(
      localRedactionPage({ config: source, first: 0, rows: 2, search: '' }),
    ).toMatchObject({
      first: 0,
      total: 3,
      items: [{ array_index: 2 }, { array_index: 1 }],
    })
    expect(
      localRedactionPage({
        config: source,
        first: 20,
        rows: 2,
        search: '  THIRD ',
      }),
    ).toMatchObject({
      first: 0,
      total: 1,
      items: [{ array_index: 2, pattern: 'third fixture' }],
    })
  })

  it('keeps preview draft fields across target changes and clears them on identity changes', () => {
    useRedactionStore.getState().resetIdentity(admin)
    useRedactionStore.getState().setPreviewText('fixture text')
    useRedactionStore.getState().setPreviewInputKind('git_diff')
    useRedactionStore.getState().setPreviewResult({
      applied_replacements: [],
      findings: [],
      redacted_text: '',
      stats: {
        applied_replacements: 0,
        dropped_findings: 0,
        llm_candidates_accepted: 0,
        llm_candidates_rejected: 0,
        llm_configured: false,
        llm_request_failed: false,
        total_findings: 0,
      },
    })
    useRedactionStore.getState().setTarget('user', 2)

    expect(useRedactionStore.getState()).toMatchObject({
      previewText: 'fixture text',
      previewInputKind: 'git_diff',
      previewResult: null,
    })

    useRedactionStore.getState().resetIdentity(operator)
    expect(useRedactionStore.getState()).toMatchObject({
      previewText: '',
      previewInputKind: 'text',
      previewResult: null,
      scope: 'user',
      targetUserId: 2,
    })
  })

  it('forces a non-admin target to their own user scope', () => {
    useRedactionStore.getState().resetIdentity(operator)
    useRedactionStore.getState().setTarget('global', null)
    useRedactionStore.getState().setTarget('user', 1)

    expect(useRedactionStore.getState()).toMatchObject({
      isAdmin: false,
      scope: 'user',
      targetUserId: 2,
    })
  })

  it('blocks invalid regex locally without acquiring the save guard or patching', async () => {
    const { REDACTION_RULES_KEY } = await import('@/lib/redaction')
    readyAdmin(config([{ pattern: '[', match_type: 'regex', scope: 'text' }]))

    const outcome = await useRedactionStore.getState().save()

    expect(outcome).toEqual({ executed: false })
    expect(useRedactionStore.getState().validationError).toBe(true)
    expect(api.setRedactionSetting).not.toHaveBeenCalled()
    expect(useRedactionStore.getState().saving).toBe(false)
    expect(REDACTION_RULES_KEY).toBe('redaction:rules')
  })

  it('captures the submitted snapshot and keeps newer edits after save response', async () => {
    const pending = deferred<RedactionSettingResponseSchema>()
    const submittedConfig = { ...config(), enabled: false }
    readyAdmin(submittedConfig)
    api.setRedactionSetting.mockReturnValueOnce(pending.promise)

    const save = useRedactionStore.getState().save()
    useRedactionStore.getState().setEnabled(true)
    pending.resolve(setting({ config: submittedConfig, scope: 'global' }))
    const outcome = await save

    expect(outcome.executed).toBe(true)
    expect(api.setRedactionSetting).toHaveBeenCalledTimes(1)
    expect(api.setRedactionSetting.mock.calls[0]?.[0].body.enabled).toBe(false)
    expect(useRedactionStore.getState().config.enabled).toBe(true)
    expect(api.listRedactionCustomStrings).toHaveBeenCalledTimes(1)
  })

  it('rejects a duplicate save trigger while the shared rules guard is held', async () => {
    const pending = deferred<RedactionSettingResponseSchema>()
    readyAdmin(config())
    api.setRedactionSetting.mockReturnValueOnce(pending.promise)

    const first = useRedactionStore.getState().save()
    const second = await useRedactionStore.getState().save()
    pending.resolve(setting({ config: config(), scope: 'global' }))
    const firstOutcome = await first

    expect(second).toEqual({ executed: false })
    expect(firstOutcome.executed).toBe(true)
    expect(api.setRedactionSetting).toHaveBeenCalledTimes(1)
  })
})

describe('redaction rule warnings', () => {
  const rule = (
    pattern: string,
    match_type: 'exact' | 'contains' | 'regex' = 'contains',
    scope: 'text' | 'line' = 'text',
  ) => ({ pattern, match_type, scope })

  it('flags an invalid regex and accepts the compileable ones', async () => {
    const { validateCustomStringRule } = await import('@/redaction-rules')

    expect(validateCustomStringRule('hello', 'regex')).toEqual({
      invalidRegex: false,
    })
    expect(validateCustomStringRule('\\b(foo', 'regex')).toEqual({
      invalidRegex: true,
    })
    expect(validateCustomStringRule('plain', 'contains')).toEqual({
      invalidRegex: false,
    })
    expect(validateCustomStringRule('', 'regex')).toEqual({
      invalidRegex: false,
    })
    expect(validateCustomStringRule('   ', 'regex')).toEqual({
      invalidRegex: false,
    })
  })

  it('flags an invalid regex even when the rule is alone', async () => {
    const { findRuleWarnings } = await import('@/redaction-rules')

    const warnings = findRuleWarnings([rule('oops(', 'regex')])

    expect(warnings[0]?.invalidRegex).toBe(true)
    expect(warnings[0]?.duplicate).toBe(false)
    expect(warnings[0]?.conflict).toBe(false)
  })

  it('leaves empty-pattern rules clean', async () => {
    const { findRuleWarnings } = await import('@/redaction-rules')

    expect(findRuleWarnings([rule('', 'regex')])[0]).toEqual({
      duplicate: false,
      conflict: false,
      invalidRegex: false,
    })
  })

  it('marks identical rules as duplicates', async () => {
    const { findRuleWarnings } = await import('@/redaction-rules')

    const warnings = findRuleWarnings([
      rule('acme', 'exact'),
      rule('acme', 'exact'),
      rule('plain'),
    ])

    expect(warnings[0]?.duplicate).toBe(true)
    expect(warnings[1]?.duplicate).toBe(true)
    expect(warnings[2]?.duplicate).toBe(false)
  })

  it('marks the same pattern with a different match type as a conflict', async () => {
    const { findRuleWarnings } = await import('@/redaction-rules')

    const warnings = findRuleWarnings([
      rule('beta', 'contains'),
      rule('beta', 'regex'),
    ])

    expect(warnings[0]?.conflict).toBe(true)
    expect(warnings[1]?.conflict).toBe(true)
    expect(warnings[0]?.duplicate).toBe(false)
  })

  it('marks the same pattern with a different scope as a conflict', async () => {
    const { findRuleWarnings } = await import('@/redaction-rules')

    const warnings = findRuleWarnings([
      rule('gamma', 'contains', 'text'),
      rule('gamma', 'contains', 'line'),
    ])

    expect(warnings[0]?.conflict).toBe(true)
    expect(warnings[1]?.conflict).toBe(true)
  })

  it('prioritizes the duplicate flag over the conflict for an exact duplicate', async () => {
    const { findRuleWarnings } = await import('@/redaction-rules')

    const warnings = findRuleWarnings([
      rule('delta', 'regex'),
      rule('delta', 'regex'),
      rule('delta', 'contains'),
    ])

    expect(warnings[0]?.duplicate).toBe(true)
    expect(warnings[1]?.duplicate).toBe(true)
    expect(warnings[0]?.conflict).toBe(false)
    expect(warnings[2]?.conflict).toBe(true)
  })
})

describe('redaction global reveal state', () => {
  const fiveRules = config([
    { pattern: 'one fixture', match_type: 'contains', scope: 'text' },
    { pattern: 'two fixture', match_type: 'contains', scope: 'text' },
    { pattern: 'three fixture', match_type: 'contains', scope: 'text' },
    { pattern: 'four fixture', match_type: 'contains', scope: 'text' },
    { pattern: 'five fixture', match_type: 'contains', scope: 'text' },
  ])

  it('masks every rule by default', () => {
    readyAdmin(fiveRules)

    expect(useRedactionStore.getState().customStringRevealAll).toBe(false)
  })

  it('toggles every rule through one global state', () => {
    readyAdmin(fiveRules)

    useRedactionStore.getState().setCustomStringRevealAll(true)
    expect(useRedactionStore.getState().customStringRevealAll).toBe(true)

    useRedactionStore.getState().setCustomStringRevealAll(false)
    expect(useRedactionStore.getState().customStringRevealAll).toBe(false)
  })

  it('keeps the global state for added and removed rules', () => {
    readyAdmin(fiveRules)

    useRedactionStore.getState().setCustomStringRevealAll(true)
    useRedactionStore.getState().addCustomStringRule()
    expect(useRedactionStore.getState().config.custom_strings).toHaveLength(6)
    expect(useRedactionStore.getState().customStringRevealAll).toBe(true)

    useRedactionStore.getState().removeCustomStringRule(0)
    expect(useRedactionStore.getState().config.custom_strings).toHaveLength(5)
    expect(useRedactionStore.getState().customStringRevealAll).toBe(true)
  })

  it('resets masking on identity and scope changes', () => {
    readyAdmin(fiveRules)

    useRedactionStore.getState().setCustomStringRevealAll(true)
    useRedactionStore.getState().resetIdentity(operator)
    expect(useRedactionStore.getState().customStringRevealAll).toBe(false)

    readyAdmin(fiveRules)
    useRedactionStore.getState().setCustomStringRevealAll(true)
    useRedactionStore.getState().setTarget('user', 2)
    expect(useRedactionStore.getState().customStringRevealAll).toBe(false)
  })

  it('resets masking when the configuration is refreshed', async () => {
    readyAdmin(fiveRules)
    api.getRedactionSetting.mockResolvedValueOnce(
      setting({ config: fiveRules, scope: 'global' }),
    )
    useRedactionStore.getState().setCustomStringRevealAll(true)

    await useRedactionStore.getState().refresh()

    expect(useRedactionStore.getState().customStringRevealAll).toBe(false)
    expect(api.listRedactionCustomStrings).toHaveBeenCalledTimes(1)
  })

  it('resets masking after a save even when a newer edit kept the response config out', async () => {
    const pending = deferred<RedactionSettingResponseSchema>()
    const submittedConfig = config([
      { pattern: 'submitted fixture', match_type: 'contains', scope: 'text' },
    ])
    readyAdmin(submittedConfig)
    api.setRedactionSetting.mockReturnValueOnce(pending.promise)
    useRedactionStore.getState().setCustomStringRevealAll(true)

    const save = useRedactionStore.getState().save()
    useRedactionStore.getState().setEnabled(false)
    pending.resolve(setting({ config: submittedConfig, scope: 'global' }))
    const outcome = await save

    expect(outcome.executed).toBe(true)
    expect(useRedactionStore.getState().config.enabled).toBe(false)
    expect(useRedactionStore.getState().customStringRevealAll).toBe(false)
  })
})

describe('redaction preview model', () => {
  it('posts the current config, text, and input kind and stores the unwrapped preview', async () => {
    readyAdmin(
      config([
        { pattern: 'fixture-rule', match_type: 'contains', scope: 'text' },
      ]),
    )
    useRedactionStore.getState().setPreviewText('fixture body')
    useRedactionStore.getState().setPreviewInputKind('git_diff')

    const outcome = await useRedactionStore.getState().runPreview()

    expect(outcome.executed).toBe(true)
    expect(api.previewRedaction).toHaveBeenCalledTimes(1)
    expect(api.previewRedaction.mock.calls[0]?.[0].body).toMatchObject({
      custom_strings: [expect.objectContaining({ pattern: 'fixture-rule' })],
      enabled: true,
      input_kind: 'git_diff',
      rules: expect.objectContaining({ email: true, secret: true }),
      text: 'fixture body',
    })
    expect(useRedactionStore.getState().previewResult).toMatchObject({
      redacted_text: 'fixture output',
    })
    expect(useRedactionStore.getState().previewing).toBe(false)
  })

  it('refuses a duplicate preview while the shared preview guard is held', async () => {
    const pending = deferred<RedactionPreviewResponseSchema>()
    readyAdmin(config())
    api.previewRedaction.mockReturnValueOnce(pending.promise)

    const first = useRedactionStore.getState().runPreview()
    const second = await useRedactionStore.getState().runPreview()

    expect(second).toEqual({ executed: false })
    expect(api.previewRedaction).toHaveBeenCalledTimes(1)
    expect(useRedactionStore.getState().previewing).toBe(true)

    pending.resolve({ preview: preview() })
    const firstOutcome = await first
    expect(firstOutcome.executed).toBe(true)
    expect(useRedactionStore.getState().previewing).toBe(false)
  })

  it('discards a late preview response when the draft changed during the request', async () => {
    const pending = deferred<RedactionPreviewResponseSchema>()
    readyAdmin(config())
    useRedactionStore.getState().setPreviewText('first body')
    api.previewRedaction.mockReturnValueOnce(pending.promise)

    const first = useRedactionStore.getState().runPreview()
    useRedactionStore.getState().setPreviewText('second body')
    pending.resolve({ preview: preview() })
    await first

    expect(useRedactionStore.getState().previewResult).toBeNull()
    expect(useRedactionStore.getState().previewing).toBe(false)
  })

  it('keeps the preview result when only the rules configuration changed', async () => {
    const pending = deferred<RedactionPreviewResponseSchema>()
    readyAdmin(config())
    useRedactionStore.getState().setPreviewText('body')
    api.previewRedaction.mockReturnValueOnce(pending.promise)

    const first = useRedactionStore.getState().runPreview()
    useRedactionStore.getState().setEnabled(false)
    pending.resolve({ preview: preview() })
    await first

    expect(useRedactionStore.getState().previewResult).toBeNull()
  })

  it('clears the preview result and releases the guard after a failed request', async () => {
    readyAdmin(config())
    api.previewRedaction.mockRejectedValueOnce(new Error('preview failed'))

    await expect(useRedactionStore.getState().runPreview()).rejects.toThrow(
      'preview failed',
    )

    expect(useRedactionStore.getState().previewResult).toBeNull()
    expect(useRedactionStore.getState().previewing).toBe(false)

    const retry = await useRedactionStore.getState().runPreview()
    expect(retry.executed).toBe(true)
  })

  it('runs preview and save independently and keeps both guards separate', async () => {
    const savePending = deferred<RedactionSettingResponseSchema>()
    const previewPending = deferred<RedactionPreviewResponseSchema>()
    readyAdmin(config())
    api.setRedactionSetting.mockReturnValueOnce(savePending.promise)
    api.previewRedaction.mockReturnValueOnce(previewPending.promise)

    const save = useRedactionStore.getState().save()
    const previewRun = useRedactionStore.getState().runPreview()

    expect(useRedactionStore.getState().saving).toBe(true)
    expect(useRedactionStore.getState().previewing).toBe(true)

    previewPending.resolve({ preview: preview() })
    await previewRun
    expect(useRedactionStore.getState().previewResult).not.toBeNull()
    expect(useRedactionStore.getState().saving).toBe(true)

    savePending.resolve(setting({ config: config(), scope: 'global' }))
    await save
    expect(useRedactionStore.getState().saving).toBe(false)
  })

  it('clears preview state on identity reset and keeps a stale finally from clearing a newer preview', async () => {
    const pending = deferred<RedactionPreviewResponseSchema>()
    readyAdmin(config())
    api.previewRedaction.mockReturnValueOnce(pending.promise)

    const first = useRedactionStore.getState().runPreview()
    useRedactionStore.getState().resetIdentity(operator)
    expect(useRedactionStore.getState()).toMatchObject({
      previewText: '',
      previewResult: null,
      previewing: false,
    })

    pending.resolve({ preview: preview() })
    await first
    expect(useRedactionStore.getState().previewing).toBe(false)
  })
})

describe('redaction preview segments', () => {
  const finding = {
    confidence: 0.98,
    end: 5,
    kind: 'email',
    match_text: 'mail@',
    normalized_key: 'email',
    source: 'rule',
    start: 0,
  }

  it('returns one plain segment without a selection', () => {
    expect(redactionOutputSegments('redacted text', null)).toEqual([
      { text: 'redacted text', highlighted: false },
    ])
    expect(redactionInputSegments('plain text', null)).toEqual([
      { text: 'plain text', highlighted: false },
    ])
  })

  it('clamps the output hit and splits before, hit, and after', () => {
    expect(
      redactionOutputSegments('abcdefgh', { ...finding, start: 2, end: 99 }),
    ).toEqual([
      { text: 'ab', highlighted: false },
      { text: 'cdefgh', highlighted: true },
    ])
  })

  it('keeps the output plain for invalid bounds', () => {
    expect(redactionOutputSegments('abcdef', { ...finding, start: 6 })).toEqual(
      [{ text: 'abcdef', highlighted: false }],
    )
    expect(
      redactionOutputSegments('abcdef', { ...finding, start: 2, end: 2 }),
    ).toEqual([{ text: 'abcdef', highlighted: false }])
  })

  it('locates the input hit at or after the reported start', () => {
    expect(
      redactionInputSegments('xx mail@ yy', { ...finding, start: 2 }),
    ).toEqual([
      { text: 'xx ', highlighted: false },
      { text: 'mail@', highlighted: true },
      { text: ' yy', highlighted: false },
    ])
  })

  it('keeps the input plain when the match text is absent', () => {
    expect(
      redactionInputSegments('nothing here', {
        ...finding,
        match_text: 'mail@',
      }),
    ).toEqual([{ text: 'nothing here', highlighted: false }])
  })
})

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((resolvePromise) => {
    resolve = resolvePromise
  })
  return { promise, resolve }
}
