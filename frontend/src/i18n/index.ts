import { approvalMessages } from './modules/approvals'
import { authMessages } from './modules/auth'
import { coreMessages } from './modules/core'
import { endpointMessages } from './modules/endpoints'
import { mcpMessages } from './modules/mcp'
import { relayMessages } from './modules/relays'
import { redactionMessages } from './modules/redaction'
import { settingsMessages } from './modules/settings'
import { usageMessages } from './modules/usage'
import { userMessages } from './modules/users'

export type Locale = 'zh-CN' | 'en-US'

export const localeOptions: Array<{ label: string; value: Locale }> = [
  { label: '中文', value: 'zh-CN' },
  { label: 'English', value: 'en-US' },
]

const modules = [
  coreMessages,
  authMessages,
  userMessages,
  endpointMessages,
  mcpMessages,
  relayMessages,
  redactionMessages,
  approvalMessages,
  settingsMessages,
  usageMessages,
] as const

function mergeLocale(locale: Locale) {
  return Object.assign({}, ...modules.map((module) => module[locale]))
}

export const messages = {
  'zh-CN': mergeLocale('zh-CN'),
  'en-US': mergeLocale('en-US'),
} as const

export type MessageKey = keyof (typeof messages)['zh-CN']
export type TranslateFn = (key: MessageKey, ...args: unknown[]) => string

export function normalizeLocale(value: string | null): Locale {
  return value === 'en-US' ? 'en-US' : 'zh-CN'
}
