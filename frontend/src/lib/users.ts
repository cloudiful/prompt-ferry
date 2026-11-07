import type { QueryClient } from '@tanstack/react-query'
import {
  createUser as createUserRequest,
  deleteUser as deleteUserRequest,
  listUsers,
  resetPassword as resetPasswordRequest,
  updateUser as updateUserRequest,
} from '@/generated/admin-api'
import type {
  CreateUserRequest,
  User,
  UserPageResponse,
  UserUpdate,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { refreshPagedQuery } from './paged-query'

const usersPageRoot = ['users', 'page'] as const

export function usersPagesQueryKey() {
  return usersPageRoot
}

export function usersPageQueryKey(first: number, rows: number) {
  return [...usersPageRoot, first, rows] as const
}

export async function listUsersPage(
  first: number,
  rows: number,
): Promise<UserPageResponse> {
  return expectData(await listUsers<true>(withData({ query: { first, rows } })))
}

export async function createUserRecord(body: CreateUserRequest): Promise<User> {
  return expectData(await createUserRequest<true>(withData({ body })))
}

export async function updateUserRecord(
  userId: number,
  body: UserUpdate,
): Promise<User> {
  return expectData(
    await updateUserRequest<true>(
      withData({ path: { user_id: userId }, body }),
    ),
  )
}

export async function deleteUserRecord(userId: number): Promise<void> {
  await deleteUserRequest<true>(withData({ path: { user_id: userId } }))
}

export async function resetUserPasswordRecord(
  userId: number,
  password: string,
): Promise<void> {
  await resetPasswordRequest<true>(
    withData({ path: { user_id: userId }, body: { password } }),
  )
}

export async function refreshUsersPage(
  queryClient: QueryClient,
  first: number,
  rows: number,
): Promise<UserPageResponse> {
  return refreshPagedQuery<UserPageResponse, User>(
    queryClient,
    {
      queryKey: usersPageQueryKey,
      namespaceKey: usersPagesQueryKey(),
      fetchPage: listUsersPage,
      items: (page) => page.users,
    },
    first,
    rows,
  )
}
