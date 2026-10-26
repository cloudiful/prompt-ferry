import type { User } from '../generated/admin-api'

export const USER_KEY_PREFIX = 'user'

export type NewUserForm = {
  login_name: string
  password: string
  display_name: string
  is_admin: boolean
}

export function userActionKey(userId: number | null): string {
  return `${USER_KEY_PREFIX}:${userId == null ? 'create' : String(userId)}`
}

export function emptyUserForm(): NewUserForm {
  return {
    login_name: '',
    password: '',
    display_name: '',
    is_admin: false,
  }
}

export type UserListItemView = {
  user: User
  user_id: number
  login_name: string
}

export type UsersWorkspaceView = {
  busy: boolean
  has_users: boolean
  user_items: UserListItemView[]
  first: number
  rows: number
  total: number
}

export function createUsersWorkspaceView(options: {
  busy: boolean
  users: User[]
  first: number
  rows: number
  total: number
}): UsersWorkspaceView {
  return {
    busy: options.busy,
    has_users: options.users.length > 0,
    first: options.first,
    rows: options.rows,
    total: options.total,
    user_items: options.users.map((user) => ({
      user,
      user_id: user.user_id,
      login_name: user.login_name,
    })),
  }
}
