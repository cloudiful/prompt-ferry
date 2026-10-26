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
  const currentPageKey = usersPageQueryKey(first, rows)
  await queryClient.cancelQueries({ queryKey: currentPageKey, exact: true })
  await queryClient.invalidateQueries({
    queryKey: usersPagesQueryKey(),
    refetchType: 'none',
  })

  let requestedFirst = first
  while (true) {
    const page = await queryClient.fetchQuery({
      queryKey: usersPageQueryKey(requestedFirst, rows),
      queryFn: () => listUsersPage(requestedFirst, rows),
      staleTime: 0,
    })
    const pageRows = page.rows > 0 ? page.rows : rows
    if (
      pageRows > 0 &&
      page.users.length === 0 &&
      requestedFirst >= page.total
    ) {
      const correctedFirst =
        page.total > 0 ? Math.floor((page.total - 1) / pageRows) * pageRows : 0
      if (correctedFirst !== requestedFirst) {
        requestedFirst = correctedFirst
        continue
      }
    }
    return page
  }
}
