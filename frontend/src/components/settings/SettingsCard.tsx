import type { ReactNode } from 'react'
import type { LucideIcon } from 'lucide-react'

type SettingsCardProps = {
  children: ReactNode
  icon: LucideIcon
  title: string
}

/**
 * One dense preference card: an icon-labelled header above its control.
 *
 * The card is presentation only; the control inside owns the value and the
 * write, so a card can never hold a hidden second copy of a preference.
 */
export function SettingsCard({
  children,
  icon: Icon,
  title,
}: SettingsCardProps) {
  return (
    <section
      className="grid min-w-0 gap-2 rounded-lg border bg-card px-3 py-2.5"
      data-settings-card={title}
    >
      <h2 className="m-0 inline-flex items-center gap-1.5 text-[0.82rem] leading-[1.3] font-semibold">
        <Icon aria-hidden="true" className="size-3.5 text-muted-foreground" />
        <span className="min-w-0 break-words">{title}</span>
      </h2>
      {children}
    </section>
  )
}
