import type { ReactNode } from 'react'
import { Info } from 'lucide-react'
import { Button } from '@/components/ui/button'
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'

type SettingsFieldRowProps = {
  children: ReactNode
  hint?: string
  label: string
  htmlFor?: string
}

export function SettingsFieldRow({
  children,
  hint,
  label,
  htmlFor,
}: SettingsFieldRowProps) {
  return (
    <div className="grid min-w-0 gap-1 md:grid-cols-[9rem_minmax(0,1fr)] md:items-center">
      <div className="flex min-w-0 items-center gap-1">
        {htmlFor ? (
          <label className="text-xs font-medium" htmlFor={htmlFor}>
            {label}
          </label>
        ) : (
          <span className="text-xs font-medium">{label}</span>
        )}
        {hint ? (
          <TooltipProvider delayDuration={250}>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  aria-label={hint}
                  className="size-6"
                  size="icon-xs"
                  type="button"
                  variant="ghost"
                >
                  <Info aria-hidden="true" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>{hint}</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        ) : null}
      </div>
      <div className="min-w-0">{children}</div>
    </div>
  )
}
