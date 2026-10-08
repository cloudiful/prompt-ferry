import type {
  HostRole,
  RelayHostResponse,
  RelayHostRoleRequest,
  RelaySettingsResponse,
  RelaySettingsUpdate,
} from '@/generated/admin-api'

export const HOST_ROLES: readonly HostRole[] = ['integrated', 'worker', 'relay']

export type RelayHostForm = {
  role: HostRole
  admin_bind: string
}

export function createEmptyRelayHostForm(): RelayHostForm {
  return { role: 'integrated', admin_bind: '' }
}

export function isValidHostRole(value: unknown): value is HostRole {
  return value === 'integrated' || value === 'worker' || value === 'relay'
}

export function relayHostToForm(
  host: RelayHostResponse | null | undefined,
  settings: RelaySettingsResponse | null | undefined,
): RelayHostForm {
  const role = host?.role
  return {
    role: isValidHostRole(role) ? role : 'integrated',
    admin_bind: settings?.relay.admin_bind ?? '',
  }
}

export function createRelayHostRoleRequest(
  form: RelayHostForm,
): RelayHostRoleRequest {
  return { role: form.role }
}

export function createRelaySettingsUpdate(
  form: RelayHostForm,
): RelaySettingsUpdate {
  const trimmed = form.admin_bind.trim()
  if (!trimmed) return {}
  return { admin_bind: trimmed }
}

export function hasHostRoleChange(
  form: RelayHostForm,
  host: RelayHostResponse | null | undefined,
): boolean {
  if (!host) return false
  if (form.role === host.role) return false
  return true
}

export function hasRelaySettingsChange(
  form: RelayHostForm,
  settings: RelaySettingsResponse | null | undefined,
): boolean {
  const current = (settings?.relay.admin_bind ?? '').trim()
  return form.admin_bind.trim() !== current
}
