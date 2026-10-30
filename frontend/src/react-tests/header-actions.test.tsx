import { fireEvent, render, screen, within } from '@testing-library/react'
import { MemoryRouter, Route, Routes, useNavigate } from 'react-router'
import { describe, expect, it, vi } from 'vitest'
import { HeaderActions } from '@/components/layout/HeaderActions'
import { AppShell } from '@/layouts/AppShell'
import { LocaleProvider } from '@/lib/i18n'
import { NotifierProvider } from '@/lib/notifier'

function ActionsPage({ onAction }: { onAction: () => void }) {
  const navigate = useNavigate()
  return (
    <>
      <HeaderActions>
        <button onClick={onAction} type="button">
          Action
        </button>
      </HeaderActions>
      <button onClick={() => navigate('/settings/general')} type="button">
        Next
      </button>
    </>
  )
}

describe('shell header actions', () => {
  it('renders route actions in the header and removes them on navigation', () => {
    localStorage.setItem('prompt-ferry:locale', 'en-US')
    const action = vi.fn()
    render(
      <LocaleProvider>
        <NotifierProvider>
          <MemoryRouter initialEntries={['/api-keys']}>
            <Routes>
              <Route element={<AppShell />}>
                <Route
                  path="/api-keys"
                  element={<ActionsPage onAction={action} />}
                />
                <Route
                  path="/settings/general"
                  element={<div>Settings content</div>}
                />
              </Route>
            </Routes>
          </MemoryRouter>
        </NotifierProvider>
      </LocaleProvider>,
    )
    const header = document.querySelector('header')!
    expect(screen.getAllByRole('heading', { level: 1 })).toHaveLength(1)
    expect(
      within(header).getByRole('heading', { name: 'API Keys' }),
    ).toBeTruthy()
    fireEvent.click(within(header).getByRole('button', { name: 'Action' }))
    expect(action).toHaveBeenCalledOnce()
    fireEvent.click(screen.getByRole('button', { name: 'Next' }))
    expect(within(header).queryByRole('button', { name: 'Action' })).toBeNull()
    expect(
      within(header).getByRole('heading', { name: 'General' }),
    ).toBeTruthy()
    expect(screen.getAllByRole('heading', { level: 1 })).toHaveLength(1)
  })
})
