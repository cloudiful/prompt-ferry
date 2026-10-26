import { CircleAlert, CircleX } from 'lucide-react'
import type { CustomStringRuleWarning } from '@/redaction-rules'
import { useLocale } from '@/lib/i18n'

export function RuleWarningBadge({
  warning,
}: {
  warning: CustomStringRuleWarning
}) {
  const { t } = useLocale()
  if (!warning.invalidRegex && !warning.duplicate && !warning.conflict) {
    return null
  }

  const invalid = warning.invalidRegex
  const message = invalid
    ? t('ruleInvalidRegex')
    : warning.duplicate
      ? t('ruleDuplicateHint')
      : t('ruleConflictHint')

  return (
    <span
      aria-label={message}
      className={`inline-flex w-fit items-center gap-1 rounded-md border px-1.5 py-0.5 text-xs ${
        invalid
          ? 'border-destructive bg-destructive/10 text-destructive'
          : 'border-amber-600/40 bg-amber-500/10 text-amber-700 dark:text-amber-300'
      }`}
      title={message}
    >
      {invalid ? (
        <CircleX aria-hidden="true" className="size-3.5" />
      ) : (
        <CircleAlert aria-hidden="true" className="size-3.5" />
      )}
      <span>{message}</span>
    </span>
  )
}
