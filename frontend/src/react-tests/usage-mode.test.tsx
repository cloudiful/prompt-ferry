import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { messages } from '@/i18n'
import { LocaleProvider } from '@/lib/i18n'
import {
  UsageModeSwitch,
  usageModeShortLabel,
} from '@/components/usage/UsageModeSwitch'
import type { UsageMode } from '@/lib/usage-model'

const copy = messages['en-US']

beforeEach(() => {
  localStorage.setItem('prompt-ferry:locale', 'en-US')
})

function renderSwitch(value: UsageMode, onChange = vi.fn()) {
  render(
    <LocaleProvider>
      <UsageModeSwitch onChange={onChange} value={value} />
    </LocaleProvider>,
  )
  return onChange
}

describe('usage mode switch', () => {
  it('labels the group and both buttons with the localized names', () => {
    renderSwitch('overview')
    expect(
      screen.getByRole('group', {
        name: `${copy.currentSlice}: ${copy.overviewMode} / ${copy.recordsMode}`,
      }),
    ).toBeTruthy()
    const overview = screen.getByRole('button', { name: copy.overviewMode })
    const records = screen.getByRole('button', { name: copy.recordsMode })
    expect(overview.getAttribute('aria-pressed')).toBe('true')
    expect(records.getAttribute('aria-pressed')).toBe('false')
  })

  it('reports the chosen mode and never issues a read itself', () => {
    const onChange = renderSwitch('overview')
    fireEvent.click(screen.getByRole('button', { name: copy.recordsMode }))
    expect(onChange).toHaveBeenCalledTimes(1)
    expect(onChange).toHaveBeenCalledWith('records')
  })

  it('shows a two-character short label while keeping the full accessible name', () => {
    renderSwitch('records')
    // The short label is the mobile-only visible text; the accessible name is
    // always the full localized label.
    expect(
      screen.getByText(usageModeShortLabel(copy.overviewMode)),
    ).toBeTruthy()
    expect(screen.getByText(usageModeShortLabel(copy.recordsMode))).toBeTruthy()
    expect(screen.getByRole('button', { name: copy.recordsMode })).toBeTruthy()
    expect(usageModeShortLabel('Overview')).toBe('Ov')
    expect(usageModeShortLabel('View')).toBe('View')
  })
})
