import { describe, expect, it } from 'vitest'
import {
  createUsersWorkspaceView,
  emptyUserForm,
  userActionKey,
} from '@/models/users'
import type { User } from '@/generated/admin-api'

const firstUser: User = {
  created_at: '2026-10-16T12:00:00.000Z',
  display_name: 'Fixture Admin',
  is_active: true,
  is_admin: true,
  login_name: 'admin',
  updated_at: '2026-10-23T12:00:00.000Z',
  user_id: 1,
}

const secondUser: User = {
  ...firstUser,
  display_name: 'Fixture User',
  is_admin: false,
  login_name: 'operator',
  user_id: 2,
}

describe('users model', () => {
  it('uses one create key and one shared write key per account', () => {
    expect(userActionKey(null)).toBe('user:create')
    expect(userActionKey(1)).toBe('user:1')
    expect(userActionKey(2)).toBe('user:2')
  })

  it('returns independent blank create forms', () => {
    const first = emptyUserForm()
    const second = emptyUserForm()
    first.login_name = 'draft'

    expect(first).toEqual({
      login_name: 'draft',
      password: '',
      display_name: '',
      is_admin: false,
    })
    expect(second.login_name).toBe('')
  })

  it('projects the current page without losing busy or paging state', () => {
    const view = createUsersWorkspaceView({
      busy: true,
      users: [firstUser, secondUser],
      first: 10,
      rows: 10,
      total: 12,
    })

    expect(view).toEqual({
      busy: true,
      has_users: true,
      user_items: [
        { user: firstUser, user_id: 1, login_name: 'admin' },
        { user: secondUser, user_id: 2, login_name: 'operator' },
      ],
      first: 10,
      rows: 10,
      total: 12,
    })
    expect(
      createUsersWorkspaceView({
        busy: false,
        users: [],
        first: 0,
        rows: 10,
        total: 0,
      }).has_users,
    ).toBe(false)
  })
})
