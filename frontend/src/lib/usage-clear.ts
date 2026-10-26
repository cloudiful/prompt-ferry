import type {
  RequestRecordsClearRequest,
  UsageClearScope,
  User,
} from '@/generated/admin-api'

export type UsageClearForm = {
  scope: UsageClearScope
  user_id: number | null
  start_at: string
  end_at: string
  delete_all: boolean
}

export type UsageClearOption<T> = { label: string; value: T }

export function createDefaultUsageClearForm(): UsageClearForm {
  return {
    scope: 'current_user',
    user_id: null,
    start_at: '',
    end_at: '',
    delete_all: false,
  }
}

export function usageClearScopeOptions(
  isAdmin: boolean,
  labels: {
    currentUser: string
    targetUser: string
    allUsers: string
  },
): UsageClearOption<UsageClearScope>[] {
  if (!isAdmin) {
    return [{ label: labels.currentUser, value: 'current_user' }]
  }
  return [
    { label: labels.currentUser, value: 'current_user' },
    { label: labels.targetUser, value: 'target_user' },
    { label: labels.allUsers, value: 'all_users' },
  ]
}

export function usageClearUserOptions(
  users: readonly User[],
): UsageClearOption<number>[] {
  return users.map((user) => ({
    label: `${user.display_name} / ${user.login_name}`,
    value: user.user_id,
  }))
}

const LOCAL_DATE_TIME = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}(?::\d{2})?$/

/**
 * A local date-time input value as a UTC instant.
 *
 * The contract stores the bounds as date-times and the backend parses a UTC
 * instant, so the form's local wall-clock text must be converted before the
 * request. An impossible date is rejected here instead of being silently
 * normalized, and the field is left empty when the operator typed nothing.
 */
export function usageLocalDateTimeToIso(value: string): string | null {
  if (!value) return null
  if (!LOCAL_DATE_TIME.test(value)) {
    throw new Error('Invalid local date-time')
  }
  const [datePart, timePart] = value.split('T')
  const [year, month, day] = datePart.split('-').map(Number)
  const [hour, minute, second = 0] = timePart.split(':').map(Number)
  const utcCalendar = new Date(Date.UTC(year, month - 1, day))
  if (
    utcCalendar.getUTCFullYear() !== year ||
    utcCalendar.getUTCMonth() !== month - 1 ||
    utcCalendar.getUTCDate() !== day ||
    hour > 23 ||
    minute > 59 ||
    second > 59
  ) {
    throw new Error('Invalid local date-time')
  }
  const date = new Date(year, month - 1, day, hour, minute, second, 0)
  if (Number.isNaN(date.getTime())) {
    throw new Error('Invalid local date-time')
  }
  return date.toISOString()
}

/**
 * The clear payload.
 *
 * Delete-all clears both bounds, and an empty bound stays `null` so the server
 * owns the `range_required` decision. A category is never sent: the action
 * clears across categories.
 */
export function usageClearPayload(
  form: UsageClearForm,
): RequestRecordsClearRequest {
  return {
    delete_all: form.delete_all,
    end_at: form.delete_all ? null : usageLocalDateTimeToIso(form.end_at),
    scope: form.scope,
    start_at: form.delete_all ? null : usageLocalDateTimeToIso(form.start_at),
    user_id: form.user_id,
  }
}

/** The success notice text for a completed clear. */
export function usageClearToastMessage(
  t: (key: 'clearHistorySubmit') => string,
  deleted: number,
): string {
  return `${t('clearHistorySubmit')}: ${deleted}`
}

/** Whether a completion still belongs to the form that started it. */
export function isSameUsageClearForm(
  submitted: UsageClearForm,
  current: UsageClearForm,
): boolean {
  return submitted === current
}
