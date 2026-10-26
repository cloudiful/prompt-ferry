import { fireEvent, render, screen } from '@testing-library/react'
import { useState } from 'react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import type { User } from '@/generated/admin-api'
import { messages } from '@/i18n'
import { LocaleProvider } from '@/lib/i18n'
import { UsageClearDialog } from '@/components/usage/UsageClearDialog'
import {
  createDefaultUsageClearForm,
  isSameUsageClearForm,
  usageClearPayload,
  usageClearToastMessage,
  usageClearScopeOptions,
  usageClearUserOptions,
  usageLocalDateTimeToIso,
  type UsageClearForm,
} from '@/lib/usage-clear'

const copy = messages['en-US']

class TestResizeObserver implements ResizeObserver {
  readonly disconnect = vi.fn()
  readonly observe = vi.fn()
  readonly unobserve = vi.fn()

  constructor(_callback: ResizeObserverCallback) {}
}

const users: User[] = [
  {
    created_at: '2026-01-01T00:00:00.000Z',
    display_name: 'Fixture Admin',
    is_active: true,
    is_admin: true,
    login_name: 'fixture-admin',
    updated_at: '2026-01-01T00:00:00.000Z',
    user_id: 1,
  },
  {
    created_at: '2026-01-01T00:00:00.000Z',
    display_name: 'Fixture Operator',
    is_active: true,
    is_admin: false,
    login_name: 'fixture-operator',
    updated_at: '2026-01-01T00:00:00.000Z',
    user_id: 2,
  },
]

beforeEach(() => {
  localStorage.setItem('prompt-ferry:locale', 'en-US')
  Element.prototype.scrollIntoView = vi.fn()
  if (!('ResizeObserver' in globalThis)) {
    Object.defineProperty(globalThis, 'ResizeObserver', {
      configurable: true,
      value: TestResizeObserver,
    })
  }
})

describe('usage clear model', () => {
  it('starts on the current user with no bounds and no delete-all', () => {
    expect(createDefaultUsageClearForm()).toEqual({
      delete_all: false,
      end_at: '',
      scope: 'current_user',
      start_at: '',
      user_id: null,
    })
  })

  it('offers the target and all-user scopes to an admin only', () => {
    expect(
      usageClearScopeOptions(false, {
        allUsers: 'All',
        currentUser: 'Me',
        targetUser: 'Target',
      }),
    ).toEqual([{ label: 'Me', value: 'current_user' }])
    expect(
      usageClearScopeOptions(true, {
        allUsers: 'All',
        currentUser: 'Me',
        targetUser: 'Target',
      }).map((option) => option.value),
    ).toEqual(['current_user', 'target_user', 'all_users'])
  })

  it('labels the target user options with the display name and login', () => {
    expect(usageClearUserOptions(users)).toEqual([
      { label: 'Fixture Admin / fixture-admin', value: 1 },
      { label: 'Fixture Operator / fixture-operator', value: 2 },
    ])
  })

  it('converts a local date-time to a UTC instant', () => {
    const iso = usageLocalDateTimeToIso('2026-10-01T12:30')
    expect(iso).toBe(new Date(2026, 9, 1, 12, 30, 0, 0).toISOString())
    expect(iso).not.toBe('2026-10-01T12:30')
    expect(iso).toMatch(/Z$/)
    expect(usageLocalDateTimeToIso('2026-10-01T12:30:15')).toBe(
      new Date(2026, 9, 1, 12, 30, 15, 0).toISOString(),
    )
    expect(usageLocalDateTimeToIso('')).toBeNull()
  })

  it('rejects an impossible or malformed local date before any request', () => {
    expect(() => usageLocalDateTimeToIso('2026-02-30T10:00')).toThrow()
    expect(() => usageLocalDateTimeToIso('2026-13-01T10:00')).toThrow()
    expect(() => usageLocalDateTimeToIso('2026-10-01 10:00')).toThrow()
    expect(() => usageLocalDateTimeToIso('2026-10-01T24:00')).toThrow()
  })

  it('builds the payload with instants, null bounds and no category', () => {
    const payload = usageClearPayload({
      ...createDefaultUsageClearForm(),
      end_at: '2026-10-02T10:00',
      scope: 'target_user',
      start_at: '2026-10-01T10:00',
      user_id: 2,
    })
    expect(payload).toEqual({
      delete_all: false,
      end_at: new Date(2026, 9, 2, 10, 0, 0, 0).toISOString(),
      scope: 'target_user',
      start_at: new Date(2026, 9, 1, 10, 0, 0, 0).toISOString(),
      user_id: 2,
    })
    expect(Object.hasOwn(payload, 'category')).toBe(false)
  })

  it('clears both bounds for delete-all and keeps empty ranges null', () => {
    expect(
      usageClearPayload({
        ...createDefaultUsageClearForm(),
        delete_all: true,
        end_at: '2026-10-02T10:00',
        start_at: '2026-10-01T10:00',
      }),
    ).toMatchObject({ delete_all: true, end_at: null, start_at: null })
    expect(usageClearPayload(createDefaultUsageClearForm())).toMatchObject({
      delete_all: false,
      end_at: null,
      start_at: null,
    })
  })

  it('composes the clear success notice from the localized label and count', () => {
    expect(usageClearToastMessage((key) => key, 3)).toBe(
      'clearHistorySubmit: 3',
    )
  })

  it('compares a completion by the submitted form identity', () => {
    const form = createDefaultUsageClearForm()
    expect(isSameUsageClearForm(form, form)).toBe(true)
    expect(isSameUsageClearForm(form, { ...form })).toBe(false)
  })
})

type HarnessProps = {
  isAdmin?: boolean
  onOpenChange?: (open: boolean) => void
  onSubmit?: (form: UsageClearForm) => void
  pending?: boolean
  pendingOrigin?: string | null
  startForm?: UsageClearForm
}

function Harness(props: HarnessProps) {
  const [form, setForm] = useState(
    props.startForm ?? createDefaultUsageClearForm(),
  )
  return (
    <UsageClearDialog
      currentUserLabel="Fixture Operator / fixture-operator"
      form={form}
      isAdmin={props.isAdmin ?? true}
      onFormChange={setForm}
      onOpenChange={props.onOpenChange ?? vi.fn()}
      onSubmit={(captured) => {
        props.onSubmit?.(captured)
      }}
      open
      pending={props.pending ?? false}
      pendingOrigin={props.pendingOrigin ?? null}
      users={users}
    />
  )
}

function renderDialog(props: HarnessProps = {}) {
  return render(
    <LocaleProvider>
      <Harness {...props} />
    </LocaleProvider>,
  )
}

describe('usage clear dialog', () => {
  it('offers all scopes to an admin and shows the target user select', () => {
    renderDialog()
    const scope = screen.getByRole('combobox', { name: copy.clearScope })
    expect(scope.textContent).toContain(copy.clearCurrentUserHistory)
    fireEvent.click(scope)
    expect(
      screen.getByRole('option', { name: copy.clearSelectedUserHistory }),
    ).toBeTruthy()
    expect(
      screen.getByRole('option', { name: copy.clearAllRecords }),
    ).toBeTruthy()
  })

  it('offers only the current user to an ordinary user', () => {
    renderDialog({ isAdmin: false })
    fireEvent.click(screen.getByRole('combobox', { name: copy.clearScope }))
    expect(
      screen.queryByRole('option', { name: copy.clearSelectedUserHistory }),
    ).toBeNull()
    expect(
      screen.queryByRole('option', { name: copy.clearAllRecords }),
    ).toBeNull()
    expect(screen.queryByRole('combobox', { name: copy.selectUser })).toBeNull()
  })

  it('disables both date inputs when delete-all is checked', () => {
    renderDialog()
    const start = screen.getByLabelText(copy.startTime) as HTMLInputElement
    const end = screen.getByLabelText(copy.endTime) as HTMLInputElement
    expect(start.disabled).toBe(false)
    fireEvent.click(screen.getByRole('checkbox'))
    expect(start.disabled).toBe(true)
    expect(end.disabled).toBe(true)
  })

  it('submits the exact current form object and no selected category', () => {
    const onSubmit = vi.fn()
    renderDialog({ onSubmit })
    fireEvent.change(screen.getByLabelText(copy.startTime), {
      target: { value: '2026-10-01T10:00' },
    })
    fireEvent.click(
      screen.getByRole('button', { name: copy.clearHistorySubmit }),
    )

    expect(onSubmit).toHaveBeenCalledTimes(1)
    const captured = onSubmit.mock.calls[0][0] as UsageClearForm
    expect(captured.start_at).toBe('2026-10-01T10:00')
    expect(Object.hasOwn(captured, 'category')).toBe(false)
  })

  it('freezes the dialog and only the clear spinner while pending', () => {
    const onOpenChange = vi.fn()
    renderDialog({ onOpenChange, pending: true, pendingOrigin: 'clear' })
    expect(document.querySelector('[data-slot="dialog-close"]')).toBeNull()
    expect(
      (screen.getByRole('button', { name: copy.cancel }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
    expect(
      (
        screen.getByRole('button', {
          name: copy.clearHistorySubmit,
        }) as HTMLButtonElement
      ).disabled,
    ).toBe(true)
    expect(
      (screen.getByLabelText(copy.startTime) as HTMLInputElement).disabled,
    ).toBe(true)

    fireEvent.keyDown(document.activeElement ?? document.body, {
      key: 'Escape',
    })
    fireEvent.pointerDown(document.body)
    expect(onOpenChange).not.toHaveBeenCalledWith(false)
  })

  it('keeps the date inputs usable for a different pending action', () => {
    renderDialog({ pending: true, pendingOrigin: 'save' })
    expect(
      (screen.getByLabelText(copy.startTime) as HTMLInputElement).disabled,
    ).toBe(true)
    // The dialog is still gated by any pending write.
    expect(
      (screen.getByRole('button', { name: copy.cancel }) as HTMLButtonElement)
        .disabled,
    ).toBe(true)
  })
})
