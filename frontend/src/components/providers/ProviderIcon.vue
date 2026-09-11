<script setup lang="ts">
import { computed } from 'vue'
import deepseekRaw from '@/assets/providers/deepseek.svg?raw'
import minimaxRaw from '@/assets/providers/minimax.svg?raw'
import opencodeRaw from '@/assets/providers/opencode.svg?raw'
import openrouterRaw from '@/assets/providers/openrouter.svg?raw'
import { useLocale } from '@/composables/useLocale'
import {
  providerLabelKey,
  providerVisual,
  type ProviderBrand,
} from './provider-visuals'

const props = withDefaults(
  defineProps<{
    provider: string
    size?: 'xs' | 'sm' | 'md'
  }>(),
  { size: 'sm' },
)

const { t } = useLocale()

const BRAND_RAW: Record<ProviderBrand, string> = {
  deepseek: deepseekRaw,
  minimax: minimaxRaw,
  opencode: opencodeRaw,
  openrouter: openrouterRaw,
}

const SIZE_CLASS: Record<'xs' | 'sm' | 'md', string> = {
  xs: 'h-3.5 w-3.5',
  sm: 'h-4 w-4',
  md: 'h-5 w-5',
}

const LETTER_TEXT_CLASS: Record<'xs' | 'sm' | 'md', string> = {
  xs: 'text-[0.44rem]',
  sm: 'text-[0.5rem]',
  md: 'text-[0.6rem]',
}

const visual = computed(() => providerVisual(props.provider))
const brandRaw = computed(() =>
  visual.value.kind === 'brand' ? BRAND_RAW[visual.value.brand] : null,
)
const letter = computed(() =>
  visual.value.kind === 'letter' ? visual.value.letter : null,
)
const label = computed(() =>
  t(providerLabelKey(props.provider) ?? 'providerGeneric'),
)
</script>

<template>
  <span
    role="img"
    class="inline-flex shrink-0 items-center justify-center"
    :class="SIZE_CLASS[size]"
    :aria-label="label"
    :title="label"
  >
    <span
      v-if="brandRaw"
      class="block h-full w-full [&_svg]:h-full [&_svg]:w-full [&_svg]:fill-current"
      v-html="brandRaw"
    />
    <span
      v-else-if="letter"
      class="flex h-full w-full items-center justify-center overflow-hidden rounded bg-elevated leading-none font-bold text-muted"
      :class="LETTER_TEXT_CLASS[size]"
      >{{ letter }}</span
    >
    <UIcon v-else name="i-lucide-server" class="h-full w-full text-muted" />
  </span>
</template>
