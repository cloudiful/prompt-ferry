import type { User } from '../generated/admin-api'

export type UserListItemView = {
  user: User
  user_id: number
  login_name: string
}

export type UsersWorkspaceView = {
  busy: boolean
  has_users: boolean
  user_items: UserListItemView[]
}

export function createUsersWorkspaceView(options: {
  busy: boolean
  users: User[]
}): UsersWorkspaceView {
  return {
    busy: options.busy,
    has_users: options.users.length > 0,
    user_items: options.users.map((user) => ({
      user,
      user_id: user.user_id,
      login_name: user.login_name,
    })),
  }
}
