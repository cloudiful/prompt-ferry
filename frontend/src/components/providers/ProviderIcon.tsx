import deepseekRaw from '@/assets/providers/deepseek.svg?raw'
import minimaxRaw from '@/assets/providers/minimax.svg?raw'
import openaiRaw from '@/assets/providers/openai.svg?raw'
import opencodeRaw from '@/assets/providers/opencode.svg?raw'
import openrouterRaw from '@/assets/providers/openrouter.svg?raw'
import { Server } from 'lucide-react'
import { useLocale } from '@/lib/i18n'
import {
  providerLabelKey,
  providerVisual,
  type ProviderBrand,
} from './provider-visuals'

type ProviderIconProps = {
  provider: string
  size?: 'xs' | 'sm' | 'md'
}

const BRAND_RAW: Record<ProviderBrand, string> = {
  deepseek: deepseekRaw,
  minimax: minimaxRaw,
  openai: openaiRaw,
  opencode: opencodeRaw,
  openrouter: openrouterRaw,
}

const SIZE_CLASS: Record<NonNullable<ProviderIconProps['size']>, string> = {
  xs: 'size-3.5',
  sm: 'size-4',
  md: 'size-5',
}

const LETTER_BY_PROVIDER: Record<string, string> = {
  command_code: 'CC',
  generic: 'G',
  glm: 'GLM',
}

export function ProviderIcon({ provider, size = 'sm' }: ProviderIconProps) {
  const { t } = useLocale()
  const visual = providerVisual(provider)
  const labelKey = providerLabelKey(provider)
  const label = t(labelKey ?? 'providerGeneric')
  const sizeClass = SIZE_CLASS[size]

  if (visual.kind === 'brand') {
    return (
      <span
        aria-label={label}
        className={`inline-flex shrink-0 items-center justify-center ${sizeClass}`}
        role="img"
        title={label}
      >
        <span
          className="block size-full [&_svg]:size-full [&_svg]:fill-current"
          dangerouslySetInnerHTML={{ __html: BRAND_RAW[visual.brand] }}
        />
      </span>
    )
  }

  if (visual.kind === 'letter') {
    return (
      <span
        aria-label={label}
        className={`inline-flex shrink-0 items-center justify-center overflow-hidden rounded bg-muted px-0.5 text-[0.55rem] leading-none font-bold text-muted-foreground ${sizeClass}`}
        role="img"
        title={label}
      >
        {visual.letter}
      </span>
    )
  }

  return (
    <span
      aria-label={label}
      className={`inline-flex shrink-0 items-center justify-center ${sizeClass}`}
      role="img"
      title={label}
    >
      <Server aria-hidden="true" className="size-full text-muted-foreground" />
    </span>
  )
}
