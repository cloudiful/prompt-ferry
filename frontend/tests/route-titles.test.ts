import { expect, test } from 'bun:test'

import { formatPageTitle, routeTitleKey, router } from '../src/router'

function titleFor(path: string, locale: 'zh-CN' | 'en-US'): string {
  return formatPageTitle(locale, routeTitleKey(router.resolve(path)))
}

test('each route contributes its localized navigation label', () => {
  expect(titleFor('/api-keys', 'zh-CN')).toBe('API 密钥 · Prompt Ferry')
  expect(titleFor('/api-keys', 'en-US')).toBe('API Keys · Prompt Ferry')
  expect(titleFor('/approvals', 'zh-CN')).toBe('审批队列 · Prompt Ferry')
  expect(titleFor('/endpoints/routes', 'zh-CN')).toBe('模型路由 · Prompt Ferry')
  expect(titleFor('/settings/general', 'en-US')).toBe('General · Prompt Ferry')
})

test('the title changes with the route and with the app language', () => {
  const zhKeys = titleFor('/api-keys', 'zh-CN')
  const enKeys = titleFor('/api-keys', 'en-US')
  const zhRelays = titleFor('/relays', 'zh-CN')

  expect(zhKeys).not.toBe(enKeys)
  expect(zhKeys).not.toBe(zhRelays)
  expect(enKeys).toContain('Prompt Ferry')
})

test('login and unknown paths still resolve a localized title', () => {
  expect(titleFor('/login', 'zh-CN')).toBe('登录 · Prompt Ferry')
  expect(titleFor('/login', 'en-US')).toBe('Login · Prompt Ferry')

  const unknown = titleFor('/not-a-page', 'zh-CN')
  expect(unknown).toContain('Prompt Ferry')
  expect(unknown.trim()).not.toBe('Prompt Ferry')
})
