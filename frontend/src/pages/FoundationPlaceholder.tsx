import { useLocale } from '@/lib/i18n'
import type { AppRoute } from '@/lib/nav'

export function FoundationPlaceholder({ route }: { route: AppRoute }) {
  const { t } = useLocale()

  return (
    <section
      aria-label={t(route.titleKey)}
      className="min-h-[calc(100vh-4rem)]"
      data-foundation-placeholder={route.id}
      data-testid="foundation-placeholder"
    />
  )
}
