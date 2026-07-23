import { createI18n } from 'vue-i18n'
import { messages, type Locale } from './index'

export const i18n = createI18n({
  legacy: false,
  locale: 'zh-CN' satisfies Locale,
  fallbackLocale: 'zh-CN' satisfies Locale,
  messages,
})
