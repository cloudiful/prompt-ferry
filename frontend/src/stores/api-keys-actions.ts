import type { Ref } from 'vue'

import type {
  AsyncActionGuard,
  AsyncActionOutcome,
} from '@/composables/useAsyncAction'
import type {
  ClientKey,
  CreateClientKeyResponse,
  UpdateClientKeyRequest,
} from '@/generated/admin-api'
import {
  createClientKey,
  deleteClientKey,
  meCreateClientKey,
  meDeleteClientKey,
  meUpdateClientKey,
  updateClientKey,
} from '@/generated/admin-api'
import { expectData, withData } from '@/api'
import { capturePayload, recordActionKey } from '@/stores/record-action-types'

/**
 * The key that addresses the authenticated user's own keys. It lives here
 * rather than in the store so the actions do not have to import the store they
 * are wired into; the store re-exports it, which is where callers import it
 * from.
 */
export const SELF_USER_ID = 0

/**
 * Key namespace for the guarded client-key writes.
 *
 * Every operation that writes one key shares its gate, so a label committed with
 * Enter and a delete of the same key cannot interleave. The create key carries
 * the owning user id: a create is not yet a key the list can act on, and two
 * users' create dialogs are genuinely independent actions that must not block
 * one another.
 */
export const CLIENT_KEY_PREFIX = 'clientkey'

export function clientKeyActionKey(keyId: string): string {
  return recordActionKey(CLIENT_KEY_PREFIX, keyId)
}

export function clientKeyCreateKey(userId: number): string {
  return `${CLIENT_KEY_PREFIX}:create:${userId}`
}

export type ApiKeyActionDeps = {
  guard: AsyncActionGuard
  loadKeys: (targetUserId: number) => Promise<unknown>
  selectedUserId: Ref<number>
}

/**
 * The client-key writes.
 *
 * The owning user is resolved once, before the request is claimed, and that
 * same id drives the follow-up reload. A user switch while a write is in flight
 * therefore cannot redirect the reload onto the newly selected user, and cannot
 * let the key land on whoever happens to be selected when it settles.
 */
export function createApiKeyActions(deps: ApiKeyActionDeps) {
  const { guard, loadKeys, selectedUserId } = deps

  function createKey(
    label?: string | null,
    forceUserId?: number,
  ): Promise<AsyncActionOutcome<CreateClientKeyResponse>> {
    const targetUserId = forceUserId ?? selectedUserId.value
    const capturedLabel = label ?? null
    return guard.run(
      clientKeyCreateKey(targetUserId),
      async () => {
        const created =
          targetUserId === SELF_USER_ID
            ? expectData(
                await meCreateClientKey<true>(
                  withData({ body: { label: capturedLabel } }),
                ),
              )
            : expectData(
                await createClientKey<true>(
                  withData({
                    path: { user_id: targetUserId },
                    body: { label: capturedLabel },
                  }),
                ),
              )
        await loadKeys(targetUserId)
        return created
      },
      'save',
    )
  }

  function saveKey(
    keyId: string,
    input: UpdateClientKeyRequest,
    forceUserId?: number,
  ): Promise<AsyncActionOutcome<ClientKey>> {
    const targetUserId = forceUserId ?? selectedUserId.value
    const captured = capturePayload(input)
    return guard.run(
      clientKeyActionKey(keyId),
      async () => {
        const saved =
          targetUserId === SELF_USER_ID
            ? expectData(
                await meUpdateClientKey<true>(
                  withData({ path: { key_id: keyId }, body: captured }),
                ),
              )
            : expectData(
                await updateClientKey<true>(
                  withData({
                    path: { user_id: targetUserId, key_id: keyId },
                    body: captured,
                  }),
                ),
              )
        await loadKeys(targetUserId)
        return saved
      },
      'save',
    )
  }

  function removeKey(
    keyId: string,
    forceUserId?: number,
  ): Promise<AsyncActionOutcome<void>> {
    const targetUserId = forceUserId ?? selectedUserId.value
    return guard.run(
      clientKeyActionKey(keyId),
      async () => {
        if (targetUserId === SELF_USER_ID) {
          await meDeleteClientKey<true>(withData({ path: { key_id: keyId } }))
        } else {
          await deleteClientKey<true>(
            withData({ path: { user_id: targetUserId, key_id: keyId } }),
          )
        }
        await loadKeys(targetUserId)
      },
      'delete',
    )
  }

  return { createKey, removeKey, saveKey }
}

export type ApiKeyActions = ReturnType<typeof createApiKeyActions>
