import { mock } from 'bun:test'
import type {
  RedactionConfigSchema,
  RedactionPreviewSchema,
} from '../src/generated/admin-api'

const actualApi = await import('../src/generated/admin-api')

export const getRedactionSetting = mock(async (options: unknown) => {
  const query = (options as { query?: { scope?: string; user_id?: number } })
    .query
  return {
    config: redactionConfig(),
    scope: query?.scope ?? 'user',
    user_id: query?.user_id,
  }
})
export const listRedactionCustomStrings = mock(async (options: unknown) => {
  const query = (options as { query?: { first?: number; rows?: number } }).query
  return {
    first: query?.first ?? 0,
    items: [],
    rows: query?.rows ?? 10,
    total: 0,
  }
})
export const setRedactionSetting = mock(async (options: unknown) => {
  const request = options as {
    body: RedactionConfigSchema
    query: { scope: string; user_id?: number }
  }
  return {
    config: request.body,
    scope: request.query.scope,
    user_id: request.query.user_id,
  }
})
export const previewRedaction = mock(async () => ({
  preview: previewFixture(),
}))

mock.module('../src/generated/admin-api', () => ({
  ...actualApi,
  getRedactionSetting,
  listRedactionCustomStrings,
  previewRedaction,
  setRedactionSetting,
}))

const { createPinia, setActivePinia } = await import('pinia')
const { useRedactionStore } = await import('../src/stores/redaction')

export function redactionConfig(): RedactionConfigSchema {
  return {
    custom_strings: [],
    enabled: false,
    rules: {
      cidr: true,
      domain: true,
      email: true,
      ip: true,
      organization: true,
      person: true,
      phone: true,
      secret: true,
      url: true,
    },
  }
}

function previewFixture(): RedactionPreviewSchema {
  return {
    applied_replacements: [],
    findings: [],
    redacted_text: 'redacted',
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

export function deferred<T = void>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((settle) => {
    resolve = settle
  })
  return { promise, resolve }
}

export function freshRedactionStore() {
  setActivePinia(createPinia())
  getRedactionSetting.mockClear()
  listRedactionCustomStrings.mockClear()
  setRedactionSetting.mockClear()
  previewRedaction.mockClear()
  getRedactionSetting.mockImplementation(async (options: unknown) => {
    const query = (options as { query?: { scope?: string; user_id?: number } })
      .query
    return {
      config: redactionConfig(),
      scope: query?.scope ?? 'user',
      user_id: query?.user_id,
    }
  })
  listRedactionCustomStrings.mockImplementation(async (options: unknown) => {
    const query = (options as { query?: { first?: number; rows?: number } })
      .query
    return {
      first: query?.first ?? 0,
      items: [],
      rows: query?.rows ?? 10,
      total: 0,
    }
  })
  setRedactionSetting.mockImplementation(async (options: unknown) => {
    const request = options as {
      body: RedactionConfigSchema
      query: { scope: string; user_id?: number }
    }
    return {
      config: request.body,
      scope: request.query.scope,
      user_id: request.query.user_id,
    }
  })
  previewRedaction.mockImplementation(async () => ({
    preview: previewFixture(),
  }))
  return useRedactionStore()
}
