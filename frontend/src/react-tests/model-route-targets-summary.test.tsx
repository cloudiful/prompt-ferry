import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { ModelRouteTargetsSummary } from '@/components/endpoints/ModelRouteTargetsSummary'
import { LocaleProvider } from '@/lib/i18n'
import type { ModelRouteTargetItemView } from '@/models/endpoints'

function target(
  id: string,
  overrides: Partial<ModelRouteTargetItemView> = {},
): ModelRouteTargetItemView {
  return {
    target_id: id,
    endpoint_label: `Endpoint ${id}`,
    enabled: true,
    endpoint_enabled: true,
    upstream_model: null,
    ...overrides,
  }
}

function renderSummary(targets: ModelRouteTargetItemView[]) {
  return render(
    <LocaleProvider>
      <ModelRouteTargetsSummary targets={targets} />
    </LocaleProvider>,
  )
}

beforeEach(() => {
  localStorage.setItem('prompt-ferry:locale', 'en-US')
})

afterEach(() => {
  cleanup()
})

describe('ModelRouteTargetsSummary', () => {
  it('keeps the empty and disabled-only states', () => {
    renderSummary([])
    expect(screen.getByText('-')).toBeTruthy()
    cleanup()
    renderSummary([target('a', { enabled: false })])
    expect(screen.getByText('No enabled targets')).toBeTruthy()
  })

  it('shows one chip for two enabled targets and discloses the rest', () => {
    renderSummary([
      target('a', { upstream_model: 'model-a' }),
      target('b'),
      target('c'),
    ])
    expect(screen.getAllByTitle('Endpoint a · model-a')).toHaveLength(1)
    expect(screen.queryByTitle('Endpoint b')).toBeNull()
    const more = screen.getByRole('button', { name: '+2 enabled targets' })
    expect(more.textContent).toBe('+2')
    fireEvent.click(more)
    const list = screen.getByRole('list')
    const items = within(list).getAllByRole('listitem')
    expect(items).toHaveLength(3)
    expect(items[0]?.textContent).toContain('Endpoint a')
    expect(items[0]?.textContent).toContain('model-a')
    expect(items[1]?.textContent).toBe('Endpoint b')
    expect(items[2]?.textContent).toBe('Endpoint c')
  })

  it('makes a single long enabled target readable through its own disclosure', () => {
    const longLabel =
      'OpenAI ChatGPT Subscription — Integration Workspace With A Long Name'
    renderSummary([target('a', { endpoint_label: longLabel })])
    const chip = screen.getByRole('button', { name: longLabel })
    expect(chip.textContent).toBe(longLabel)
    expect(chip.getAttribute('title')).toBe(longLabel)
    expect(chip.className).toContain('justify-self-start')
    expect(chip.className).toContain('max-w-full')
    fireEvent.click(chip)
    const list = screen.getByRole('list')
    expect(within(list).getByText(longLabel)).toBeTruthy()
  })
})
