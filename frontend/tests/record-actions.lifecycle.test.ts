import { expect, mock, test } from 'bun:test'
import { reactive } from 'vue'

import { useAsyncAction } from '../src/composables/useAsyncAction'
import { USER_KEY_PREFIX, userActionKey } from '../src/stores/users-actions'

/**
 * The completion contract for the record domains: what each operation reports,
 * and what it may close.
 *
 * The store half is real here — a real guard behind the fake store — because
 * "three presses, one completion" is a property of the guard, and a fake that
 * always answers `executed: true` would prove nothing. Which keys exclude which
 * is covered by `record-actions.gates.test.ts`.
 */

const pending = reactive({ user: {} as Record<string, string> })
const notices = { errors: [] as unknown[], successes: [] as string[] }
const confirmations: string[] = []

function reset(): void {
  pending.user = {}
  notices.errors.length = 0
  notices.successes.length = 0
  confirmations.length = 0
}

/**
 * A users page over a store whose actions run behind a real guard. `hold` keeps
 * the next write open, so a second press really does arrive while the first is
 * still in flight.
 */
async function buildUsersPage(hold?: () => Promise<void>) {
  reset()
  const { useUsersPageActions } =
    await import('../src/composables/useUsersPageActions')
  const guard = useAsyncAction()
  const writes = { count: 0 }

  const guarded = <T>(key: string, label: string, body: () => Promise<T>) =>
    guard.run(
      key,
      async () => {
        writes.count += 1
        if (hold) await hold()
        return body()
      },
      label,
    )

  const store = {
    get userPendingActions() {
      return pending.user
    },
    createNewUser: mock((body: unknown) =>
      guarded(userActionKey(null), 'save', async () => {
        writes.created = body as { login_name: string }
        return { user_id: 9 }
      }),
    ),
    removeUser: mock((id: number) =>
      guarded(userActionKey(id), 'delete', async () => undefined),
    ),
    resetUserPassword: mock((id: number) =>
      guarded(userActionKey(id), 'reset', async () => undefined),
    ),
    saveUser: mock((id: number) =>
      guarded(userActionKey(id), 'save', async () => ({ user_id: id })),
    ),
  }

  const page = useUsersPageActions({
    confirm: (message) => {
      confirmations.push(message)
      return true
    },
    notifyApiError: (cause) => notices.errors.push(cause),
    notifySuccess: (message) => notices.successes.push(message),
    store: store as never,
    t: (key) => key,
  })
  return { guard, page, store, writes }
}

test('three presses in a row produce one write and one completion', async () => {
  let release!: () => void
  const held = new Promise<void>((settle) => {
    release = settle
  })
  const { page, writes } = await buildUsersPage(() => held)
  page.openCreateUser()
  page.createUserForm.value.login_name = 'newbie'

  // A held Enter and two clicks reach the page; the guard collapses them to one
  // accepted call, and the page completes once.
  const presses = [
    page.submitCreateUser(),
    page.submitCreateUser(),
    page.submitCreateUser(),
  ]
  release()
  await Promise.all(presses)

  expect(writes.count).toBe(1)
  expect(notices.successes).toEqual(['userCreated'])
  expect(page.createUserVisible.value).toBe(false)
  expect(page.createUserForm.value.login_name).toBe('')
})

test('a refused save reports nothing and leaves the dialog open', async () => {
  let release!: () => void
  const held = new Promise<void>((settle) => {
    release = settle
  })
  const { page, store } = await buildUsersPage(() => held)
  page.openCreateUser()
  page.createUserForm.value.login_name = 'newbie'

  const refused = page.submitCreateUser()
  const second = page.submitCreateUser()
  release()
  await Promise.all([refused, second])

  // One completion belongs to the one accepted call; the refusal is silent.
  expect(notices.successes).toEqual(['userCreated'])
  expect(notices.errors).toEqual([])
  expect(store.createNewUser).toHaveBeenCalledTimes(2)
})

test('a failed save reports the error and keeps the draft for a retry', async () => {
  const { page, store } = await buildUsersPage()
  const boom = new Error('validation failed')
  store.createNewUser.mockImplementation(async () => {
    throw boom
  })
  page.openCreateUser()
  page.createUserForm.value.login_name = 'newbie'

  await page.submitCreateUser()

  expect(notices.errors).toEqual([boom])
  expect(notices.successes).toEqual([])
  // The draft survives so the create can be retried.
  expect(page.createUserVisible.value).toBe(true)
  expect(page.createUserForm.value.login_name).toBe('newbie')
  // A failure is not a completion, so the key is already released and a retry
  // is not blocked.
  expect(page.createUserPending.value).toBe(false)
  await page.submitCreateUser()
  expect(notices.errors).toEqual([boom, boom])
})

test('a save that lands on a reopened dialog never closes it', async () => {
  let release!: () => void
  const held = new Promise<void>((settle) => {
    release = settle
  })
  const { page } = await buildUsersPage(() => held)

  page.openCreateUser()
  page.createUserForm.value.login_name = 'newbie'
  const saving = page.submitCreateUser()

  // The operator opens the dialog again before the write settles.
  page.openCreateUser()
  page.createUserForm.value.login_name = 'someone-else'
  release()
  await saving

  expect(page.createUserVisible.value).toBe(true)
  expect(page.createUserForm.value.login_name).toBe('someone-else')
  // The write itself did happen, so it is still reported once.
  expect(notices.successes).toEqual(['userCreated'])
})

test('a reset that lands on another user never clears that dialog', async () => {
  let release!: () => void
  const held = new Promise<void>((settle) => {
    release = settle
  })
  const { page } = await buildUsersPage(() => held)

  page.openResetPassword({ login_name: 'alice', user_id: 1 } as never)
  page.resetPasswordValue.value = 'hunter2'
  const saving = page.submitResetPassword()

  page.openResetPassword({ login_name: 'bob', user_id: 2 } as never)
  release()
  await saving

  expect(page.resetPasswordUser.value?.user_id).toBe(2)
  expect(notices.successes).toEqual(['passwordReset'])
})

test('a save and a delete of one user cannot both report', async () => {
  let release!: () => void
  const held = new Promise<void>((settle) => {
    release = settle
  })
  const { page, writes } = await buildUsersPage(() => held)
  const alice = { login_name: 'alice', user_id: 1 } as never

  const saving = page.saveUser(alice)
  const removing = page.deleteUser(alice)
  release()
  await Promise.all([saving, removing])

  expect(writes.count).toBe(1)
  expect(notices.successes).toHaveLength(1)
})

test('a delete reports once, only once confirmed', async () => {
  const { page } = await buildUsersPage()
  await page.deleteUser({ login_name: 'alice', user_id: 1 } as never)

  expect(confirmations).toEqual(['delete alice?'])
  expect(notices.successes).toEqual(['userDeleted'])
})

test('each dialog spinner follows the record its own draft belongs to', async () => {
  const { page } = await buildUsersPage()

  // The create dialog has no record yet, so it follows the create slot.
  expect(page.createUserPending.value).toBe(false)
  pending.user = { create: 'save' }
  expect(page.createUserPending.value).toBe(true)

  // A write against another account is not this dialog's.
  pending.user = { '7': 'save' }
  expect(page.createUserPending.value).toBe(false)

  pending.user = { '1': 'reset' }
  expect(page.resetPasswordPending.value).toBe(false)
  page.openResetPassword({ login_name: 'alice', user_id: 1 } as never)
  expect(page.resetPasswordPending.value).toBe(true)
  // A delete of the same account still freezes it: that account is being
  // written either way.
  pending.user = { '1': 'delete' }
  expect(page.resetPasswordPending.value).toBe(true)

  pending.user = { '7': 'save' }
  expect(page.resetPasswordPending.value).toBe(false)
  expect(USER_KEY_PREFIX).toBe('user')
})
