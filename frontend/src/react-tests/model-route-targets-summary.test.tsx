import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
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

function renderSummary(
  targets: ModelRouteTargetItemView[],
  props: {
    onToggleTarget?: (targetId: string, enabled: boolean) => void
    pending?: boolean
    pendingTargetId?: string | null
  } = {},
) {
  return render(
    <LocaleProvider>
      <ModelRouteTargetsSummary targets={targets} {...props} />
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
  it('keeps the empty state and offers the disclosure when every target is off', () => {
    renderSummary([])
    expect(screen.getByText('-')).toBeTruthy()
    cleanup()
    renderSummary([target('a', { enabled: false })])
    const disclosure = screen.getByRole('button', {
      name: 'No enabled targets',
    })
    fireEvent.click(disclosure)
    const list = screen.getByRole('list')
    expect(within(list).getAllByRole('listitem')).toHaveLength(1)
    expect(within(list).getByText('Endpoint a')).toBeTruthy()
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

  it('lists every configured target and names each switch', () => {
    renderSummary([
      target('a'),
      target('b', { enabled: false }),
      target('c', { endpoint_enabled: false }),
    ])
    fireEvent.click(screen.getByRole('button', { name: 'Endpoint a' }))
    const list = screen.getByRole('list')
    expect(within(list).getAllByRole('listitem')).toHaveLength(3)

    const enabled = within(list).getByRole('switch', {
      name: 'Target Endpoint a',
    })
    expect(enabled.getAttribute('aria-checked')).toBe('true')
    const disabled = within(list).getByRole('switch', {
      name: 'Target Endpoint b',
    })
    expect(disabled.getAttribute('aria-checked')).toBe('false')
    const unavailable = within(list).getByRole('switch', {
      name: 'Target Endpoint c',
    })
    expect(unavailable.getAttribute('aria-checked')).toBe('true')
    expect(within(list).getByText('Upstream unavailable')).toBeTruthy()
  })

  it('switches one target and leaves the others alone', () => {
    const onToggleTarget = vi.fn()
    renderSummary([target('a'), target('b', { enabled: false })], {
      onToggleTarget,
    })
    fireEvent.click(screen.getByRole('button', { name: 'Endpoint a' }))
    const list = screen.getByRole('list')
    fireEvent.click(
      within(list).getByRole('switch', { name: 'Target Endpoint b' }),
    )
    expect(onToggleTarget).toHaveBeenCalledTimes(1)
    expect(onToggleTarget).toHaveBeenCalledWith('b', true)
  })

  it('disables every target switch while the route write is pending', () => {
    const onToggleTarget = vi.fn()
    renderSummary([target('a'), target('b', { enabled: false })], {
      onToggleTarget,
      pending: true,
      pendingTargetId: 'a',
    })
    fireEvent.click(screen.getByRole('button', { name: 'Endpoint a' }))
    const list = screen.getByRole('list')
    const first = within(list).getByRole('switch', {
      name: 'Target Endpoint a',
    })
    const second = within(list).getByRole('switch', {
      name: 'Target Endpoint b',
    })
    expect((first as HTMLButtonElement).disabled).toBe(true)
    expect((second as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(first)
    expect(onToggleTarget).not.toHaveBeenCalled()
  })
})
