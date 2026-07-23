import type { TranslateFn as AppTranslateFn } from '@/i18n'

declare global {
  type TranslateFn = AppTranslateFn
}

export {}
