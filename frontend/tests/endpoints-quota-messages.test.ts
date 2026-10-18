import { expect, test } from 'bun:test'
import { createI18n } from 'vue-i18n'
import { endpointMessages } from '../src/i18n/modules/endpoints'

// The real app creates its i18n instance from these modules, so the plural
// check has to read the same messages through a real translator instead of
// asserting the pipe source text.
const i18n = createI18n({
  legacy: false,
  locale: 'en-US',
  fallbackLocale: 'zh-CN',
  messages: {
    'en-US': endpointMessages['en-US'],
    'zh-CN': endpointMessages['zh-CN'],
  },
})

const t = i18n.global.t

function translate(key: string, count: number): string {
  return t(key, { count }) as string
}

test('English collection failures are singular for one and plural above one', () => {
  expect(translate('quotaTrendFailed', 1)).toBe('1 quota collection failure')
  expect(translate('quotaTrendFailed', 2)).toBe('2 quota collection failures')
  expect(translate('quotaTrendFailed', 5)).toBe('5 quota collection failures')
})

test('English window counts are singular for one and plural above one', () => {
  expect(translate('tokenPlanWindowCount', 1)).toBe('1 window')
  expect(translate('tokenPlanWindowCount', 2)).toBe('2 windows')
  expect(translate('tokenPlanWindowCount', 3)).toBe('3 windows')
})

test('Chinese keeps its single form and meaning for both counts', () => {
  i18n.global.locale.value = 'zh-CN'
  expect(translate('quotaTrendFailed', 1)).toBe('额度采集失败 1 次')
  expect(translate('quotaTrendFailed', 2)).toBe('额度采集失败 2 次')
  expect(translate('tokenPlanWindowCount', 1)).toBe('1 个窗口')
  expect(translate('tokenPlanWindowCount', 2)).toBe('2 个窗口')
  i18n.global.locale.value = 'en-US'
})

test('the remaining label stays explicit in both locales', () => {
  i18n.global.locale.value = 'en-US'
  expect(t('quotaRemainingPercent')).toBe('Remaining')
  i18n.global.locale.value = 'zh-CN'
  expect(t('quotaRemainingPercent')).toBe('剩余')
  i18n.global.locale.value = 'en-US'
})
