import type { ClientKey, User } from '../generated/admin-api'

export type ApiKeyItemView = {
  key: ClientKey
  key_id: number
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
  visibleKeySecrets: Record<number, boolean>
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
