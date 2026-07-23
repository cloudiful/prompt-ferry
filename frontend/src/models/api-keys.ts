import type { ClientKey, User } from '../generated/admin-api'

export type ApiKeyItemView = {
  key: ClientKey
  key_id: number
  label: string
  enabled: boolean
  enabled_label: string
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
  labels: {
    active: string
    disabled: string
  }
}): ApiKeysWorkspaceView {
  return {
    has_keys: options.keys.length > 0,
    key_items: options.keys.map((key) => ({
      key,
      key_id: key.key_id,
      label: `${key.key_prefix} / ${key.label}`,
      enabled: key.enabled,
      enabled_label: key.enabled
        ? options.labels.active
        : options.labels.disabled,
      secret: key.secret ?? '',
      visible_secret: Boolean(options.visibleKeySecrets[key.key_id]),
    })),
    selected_user: options.selectedUser,
  }
}
