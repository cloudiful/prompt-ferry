import { expect, test } from 'bun:test'

import { messages } from '../src/i18n'

const source = (relative: string) =>
  Bun.file(new URL(relative, import.meta.url)).text()

const locales = ['zh-CN', 'en-US'] as const

test('workflow intro copy is gone while risk notices survive', async () => {
  const relayHost = await source('../src/pages/RelayHostPage.vue')
  expect(relayHost).not.toContain("t('relayHostLoginDesc')")

  const archive = await source(
    '../src/components/settings/SettingsConfigArchiveTab.vue',
  )
  expect(archive).not.toContain("t('configArchiveExportHelp')")
  expect(archive).toContain("t('configArchiveExportFootnote')")

  const importCard = await source(
    '../src/components/settings/ConfigArchiveImportCard.vue',
  )
  expect(importCard).not.toContain("t('configArchiveImportHelp')")
  // The flow stays discoverable through its own actions, and the irreversible
  // replace warning stays in the confirm dialog.
  expect(importCard).toContain("t('configArchivePreviewAction')")
  expect(importCard).toContain("t('configArchiveApplyAction')")
  expect(importCard).toContain('<ConfigArchiveImportConfirmDialog')

  for (const locale of locales) {
    for (const key of [
      'configArchiveExportHelp',
      'configArchiveImportHelp',
      'relayHostLoginDesc',
    ]) {
      expect(Object.hasOwn(messages[locale], key)).toBe(false)
    }
  }
  expect(messages['zh-CN'].configArchiveImportConfirmBody).toContain('无法撤销')
  expect(messages['en-US'].configArchiveImportConfirmBody).toContain(
    'cannot be undone',
  )
})

test('the user create form labels every field', async () => {
  const dialog = await source('../src/components/users/CreateUserDialog.vue')

  expect(dialog).not.toContain('placeholder="login_name"')
  for (const label of [
    "t('loginName')",
    "t('displayName')",
    "t('userInitialPassword')",
  ]) {
    expect(dialog).toContain(`:label="${label}"`)
  }
  expect(dialog).toContain("t('userLoginPlaceholder')")
})

test('user-facing copy no longer leans on backend field names', () => {
  for (const locale of locales) {
    expect(messages[locale].ownerUser).not.toContain('owner_user_id')
    expect(messages[locale].userLoginPlaceholder).not.toContain('login_name')
  }
  expect(messages['zh-CN'].ownerUser).toBe('选择用户')
  expect(messages['en-US'].ownerUser).toBe('Select user')
})

test('the MCP generic preset keeps its localized name', async () => {
  const dialog = await source('../src/components/mcp/McpDialog.vue')

  expect(dialog).toContain("provider.id === 'generic'")
  expect(dialog).toContain("props.t('providerGeneric')")
  expect(messages['zh-CN'].providerGeneric).toBe('通用')
  expect(messages['en-US'].providerGeneric).toBe('Generic')
})

test('MCP connection copy states the user consequence, not the mechanism', () => {
  expect(messages['zh-CN'].bearerTokenHint).not.toContain('最少使用')
  expect(messages['en-US'].bearerTokenHint).not.toContain('least-used')
  expect(messages['zh-CN'].lifecyclePolicyHint).not.toContain('server/discover')
  expect(messages['en-US'].lifecyclePolicyHint).not.toContain('server/discover')
})

test('organization usage explains scope and time basis without routing internals', () => {
  const zh = messages['zh-CN'].endpointOrganizationUsageHint
  const en = messages['en-US'].endpointOrganizationUsageHint

  expect(zh).toContain('UTC 月初至今')
  expect(zh).toContain('不是 ChatGPT 订阅额度')
  expect(zh).not.toContain('路由权重')
  expect(en).toContain('UTC month to date')
  expect(en).toContain('ChatGPT subscription quota')
  expect(en).not.toContain('routing weight')
})
