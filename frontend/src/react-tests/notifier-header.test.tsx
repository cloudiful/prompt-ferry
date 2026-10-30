import { act, fireEvent, render, screen, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { NotifierProvider, useNotifier } from '@/lib/notifier'

function Trigger() {
  const { notifyError } = useNotifier()
  return (
    <button onClick={() => notifyError('Long error '.repeat(40))}>
      Notify
    </button>
  )
}

describe('notifications', () => {
  afterEach(() => vi.useRealTimers())

  it('dismisses only the selected notification without extending another timer', () => {
    vi.useFakeTimers()
    render(
      <NotifierProvider>
        <Trigger />
      </NotifierProvider>,
    )
    fireEvent.click(screen.getByRole('button', { name: 'Notify' }))
    act(() => vi.advanceTimersByTime(2000))
    fireEvent.click(screen.getByRole('button', { name: 'Notify' }))
    const alerts = screen.getAllByRole('alert')
    expect(alerts).toHaveLength(2)
    expect(alerts[0].className).toContain('pointer-events-auto')
    expect(alerts[0].parentElement?.className).toContain('pointer-events-none')
    const close = within(alerts[1]).getByRole('button', { name: 'Close' })
    close.focus()
    expect(document.activeElement).toBe(close)
    fireEvent.click(close)
    expect(screen.getAllByRole('alert')).toHaveLength(1)
    act(() => vi.advanceTimersByTime(2999))
    expect(screen.getAllByRole('alert')).toHaveLength(1)
    act(() => vi.advanceTimersByTime(1))
    expect(screen.queryByRole('alert')).toBeNull()
  })
})
