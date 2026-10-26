import type { ClientKey, User } from '../generated/admin-api'

export const SELF_USER_ID = 0
export const CLIENT_KEY_PREFIX = 'clientkey'

export function clientKeyActionKey(keyId: string): string {
  return `${CLIENT_KEY_PREFIX}:${keyId}`
}

export function clientKeyCreateKey(userId: number): string {
  return `${CLIENT_KEY_PREFIX}:create:${userId}`
}

export type ApiKeyItemView = {
  key: ClientKey
  key_id: string
  key_prefix: string
  label: string
  enabled: boolean
  secret: string
  visible_secret: boolean
}

export type ApiKeysWorkspaceView = {
  has_keys: boolean
  key_items: ApiKeyItemView[]
  selected_user: User | null
}

export function createApiKeysWorkspaceView(options: {
  keys: ClientKey[]
  selectedUser: User | null
  visibleKeySecrets: Record<string, boolean>
}): ApiKeysWorkspaceView {
  return {
    has_keys: options.keys.length > 0,
    key_items: options.keys.map((key) => ({
      key,
      key_id: key.key_id,
      key_prefix: key.key_prefix,
      label: key.label,
      enabled: key.enabled,
      secret: key.secret ?? '',
      visible_secret: Boolean(options.visibleKeySecrets[key.key_id]),
    })),
    selected_user: options.selectedUser,
  }
}
