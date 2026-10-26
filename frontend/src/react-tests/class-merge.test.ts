import { describe, expect, it } from 'vitest'
import { cn } from '@/lib/utils'

describe('generated shadcn class merging', () => {
  it('joins conditional classes and resolves Tailwind conflicts', () => {
    expect(cn('px-3 text-sm bg-card', 'px-5 bg-primary')).toBe(
      'text-sm px-5 bg-primary',
    )
    expect(
      cn('bg-primary text-primary-foreground', 'bg-card text-card-foreground'),
    ).toBe('bg-card text-card-foreground')
    expect(cn('gap-2 sm:gap-4', 'gap-1 sm:gap-6')).toBe('gap-1 sm:gap-6')
  })
})
