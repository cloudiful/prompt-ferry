import { useState } from 'react'
import {
  act,
  cleanup,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { EndpointModelInput } from '@/components/endpoints/EndpointModelInput'
import { clearEndpointModelCatalogCache } from '@/lib/endpoint-models'
import { LocaleProvider } from '@/lib/i18n'

const api = vi.hoisted(() => ({
  fetchEndpointModels: vi.fn(),
}))

vi.mock('@/stores/endpoints-api', () => ({
  fetchEndpointModels: api.fetchEndpointModels,
}))

function Harness({
  endpointId = 'ep-input',
  initialValue = '',
}: {
  endpointId?: string
  initialValue?: string
}) {
  const [value, setValue] = useState(initialValue)
  return (
    <LocaleProvider>
      <EndpointModelInput
        endpointId={endpointId}
        onChange={setValue}
        placeholder="Upstream model"
        value={value}
      />
      <output data-testid="committed-value">{value}</output>
    </LocaleProvider>
  )
}

function input(): HTMLInputElement {
  return document.querySelector(
    '[data-model-candidate-input]',
  ) as HTMLInputElement
}

function suggestions(): string[] {
  return Array.from(
    document.querySelectorAll('[data-model-candidate-option]'),
  ).map((option) => (option.textContent ?? '').trim())
}

function committedValue(): string {
  return screen.getByTestId('committed-value').textContent ?? ''
}

async function renderHarness(initialValue = ''): Promise<void> {
  render(<Harness initialValue={initialValue} />)
  await waitFor(() =>
    expect(api.fetchEndpointModels).toHaveBeenCalledWith('ep-input', undefined),
  )
}

beforeEach(() => {
  vi.resetAllMocks()
  localStorage.clear()
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  clearEndpointModelCatalogCache()
  api.fetchEndpointModels.mockResolvedValue({
    models: ['model-b', 'alpha-model', 'model-a', 'other-entry'],
  })
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('EndpointModelInput suggestions', () => {
  it('opens matches while typing and filters by case-insensitive substring in catalog order', async () => {
    await renderHarness()
    const field = input()
    expect(screen.queryByRole('listbox')).toBeNull()

    fireEvent.change(field, { target: { value: 'MODEL' } })
    const list = await screen.findByRole('listbox')
    expect(suggestions()).toEqual(['alpha-model', 'model-a', 'model-b'])
    expect(committedValue()).toBe('MODEL')
    // Typing never preselects or rewrites the draft.
    expect(list.querySelector('[aria-selected="true"]')).toBeNull()
    expect(field.getAttribute('aria-activedescendant')).toBeNull()
    expect(field.getAttribute('aria-expanded')).toBe('true')

    fireEvent.change(field, { target: { value: 'del-a' } })
    await waitFor(() => expect(suggestions()).toEqual(['model-a']))
    expect(committedValue()).toBe('del-a')
  })

  it('shows the full catalog for an empty query and an accessible empty state without matches', async () => {
    await renderHarness()
    const field = input()

    fireEvent.click(screen.getByRole('button', { name: 'Model candidates' }))
    await screen.findByRole('listbox')
    await waitFor(() =>
      expect(suggestions()).toEqual([
        'alpha-model',
        'model-a',
        'model-b',
        'other-entry',
      ]),
    )
    expect(document.querySelector('[data-model-candidate-empty]')).toBeNull()
    expect(document.activeElement).toBe(field)

    fireEvent.change(field, { target: { value: 'no-such-model' } })
    await waitFor(() => expect(suggestions()).toEqual([]))
    const emptyState = document.querySelector('[data-model-candidate-empty]')
    expect(emptyState?.textContent).toBe('No data')
    expect(emptyState?.getAttribute('role')).toBe('status')
    // An unmatched query stays a free-text draft and never commits on Enter.
    expect(field.value).toBe('no-such-model')
    expect(committedValue()).toBe('no-such-model')
    fireEvent.keyDown(field, { key: 'Enter' })
    expect(field.value).toBe('no-such-model')
    expect(committedValue()).toBe('no-such-model')
    expect(screen.getByRole('listbox')).toBeTruthy()
  })

  it('navigates and commits only the current matches, then closes back into the input', async () => {
    await renderHarness()
    const field = input()

    fireEvent.change(field, { target: { value: 'model' } })
    const list = await screen.findByRole('listbox')
    fireEvent.keyDown(field, { key: 'ArrowDown' })
    expect(field.getAttribute('aria-activedescendant')).toBe(`${list.id}-0`)
    fireEvent.keyDown(field, { key: 'ArrowDown' })
    fireEvent.keyDown(field, { key: 'ArrowDown' })
    // Clamped inside the filtered result set, never past its end.
    expect(field.getAttribute('aria-activedescendant')).toBe(`${list.id}-2`)
    const active = list.querySelector('[aria-selected="true"]')
    expect(active?.textContent?.trim()).toBe('model-b')

    fireEvent.keyDown(field, { key: 'Enter' })
    expect(committedValue()).toBe('model-b')
    expect(field.value).toBe('model-b')
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(document.activeElement).toBe(field)
  })

  it('clears the stale active match when the filter changes and keeps focus on Escape', async () => {
    await renderHarness()
    const field = input()

    fireEvent.change(field, { target: { value: 'model' } })
    const list = await screen.findByRole('listbox')
    fireEvent.keyDown(field, { key: 'ArrowDown' })
    expect(field.getAttribute('aria-activedescendant')).toBe(`${list.id}-0`)

    fireEvent.change(field, { target: { value: 'model-b' } })
    await waitFor(() => expect(suggestions()).toEqual(['model-b']))
    expect(field.getAttribute('aria-activedescendant')).toBeNull()
    expect(list.querySelector('[aria-selected="true"]')).toBeNull()

    fireEvent.keyDown(field, { key: 'Escape' })
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(field.value).toBe('model-b')
    expect(committedValue()).toBe('model-b')
    expect(document.activeElement).toBe(field)
  })

  it('commits a clicked match and keeps Tab free', async () => {
    await renderHarness()
    const field = input()

    fireEvent.change(field, { target: { value: 'alpha' } })
    const list = await screen.findByRole('listbox')
    fireEvent.click(within(list).getByRole('option', { name: 'alpha-model' }))
    expect(committedValue()).toBe('alpha-model')
    expect(screen.queryByRole('listbox')).toBeNull()

    fireEvent.change(field, { target: { value: 'model' } })
    await screen.findByRole('listbox')
    const tab = createEvent.keyDown(field, { key: 'Tab' })
    fireEvent(field, tab)
    expect(tab.defaultPrevented).toBe(false)
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(field.value).toBe('model')
  })

  it('ignores selection keys while an IME composition is active', async () => {
    await renderHarness()
    const field = input()

    fireEvent.change(field, { target: { value: 'model' } })
    const list = await screen.findByRole('listbox')
    fireEvent.keyDown(field, { key: 'ArrowDown' })
    expect(field.getAttribute('aria-activedescendant')).toBe(`${list.id}-0`)

    const composingArrow = createEvent.keyDown(field, {
      key: 'ArrowDown',
    })
    Object.defineProperty(composingArrow, 'isComposing', { value: true })
    fireEvent(field, composingArrow)
    expect(composingArrow.defaultPrevented).toBe(false)
    expect(field.getAttribute('aria-activedescendant')).toBe(`${list.id}-0`)

    const composingEnter = createEvent.keyDown(field, { key: 'Enter' })
    Object.defineProperty(composingEnter, 'isComposing', { value: true })
    fireEvent(field, composingEnter)
    expect(committedValue()).toBe('model')
    expect(field.value).toBe('model')
    expect(screen.getByRole('listbox')).toBeTruthy()

    const composingEscape = createEvent.keyDown(field, { key: 'Escape' })
    Object.defineProperty(composingEscape, 'isComposing', { value: true })
    fireEvent(field, composingEscape)
    // The IME still owns Escape: it never commits or rewrites the draft.
    expect(committedValue()).toBe('model')
    expect(field.value).toBe('model')
  })

  it('closes and drops the active match when the endpoint changes', async () => {
    function Routed() {
      const [endpointId, setEndpointId] = useState('ep-input')
      const [value, setValue] = useState('mod')
      return (
        <LocaleProvider>
          <EndpointModelInput
            endpointId={endpointId}
            onChange={setValue}
            placeholder="Upstream model"
            value={value}
          />
          <button onClick={() => setEndpointId('ep-other')} type="button">
            switch endpoint
          </button>
        </LocaleProvider>
      )
    }
    api.fetchEndpointModels.mockImplementation(async (endpointId: string) => ({
      models: endpointId === 'ep-input' ? ['model-a', 'model-b'] : ['other'],
    }))
    render(<Routed />)
    await waitFor(() =>
      expect(api.fetchEndpointModels).toHaveBeenCalledWith(
        'ep-input',
        undefined,
      ),
    )
    const field = input()
    fireEvent.change(field, { target: { value: 'model' } })
    await screen.findByRole('listbox')
    fireEvent.keyDown(field, { key: 'ArrowDown' })

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'switch endpoint' }))
    })
    await waitFor(() =>
      expect(api.fetchEndpointModels).toHaveBeenCalledWith(
        'ep-other',
        undefined,
      ),
    )
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(field.getAttribute('aria-activedescendant')).toBeNull()
    expect(field.value).toBe('model')
  })
})
