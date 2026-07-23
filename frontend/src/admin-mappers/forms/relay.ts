import type {
  BridgeEncryptionMode,
  ManagedRelay,
  ManagedRelayPatchRequest,
  ManagedRelayRequest,
  ManagedRelaySecretPatch,
  TlsMode,
} from '@/generated/admin-api'

export type RelaySecretAction = 'keep' | 'replace' | 'clear'

export type RelayForm = {
  relay_id: string
  name: string
  relay_url: string
  enabled: boolean
  tls_mode: TlsMode
  bridge_encryption_mode: BridgeEncryptionMode
  has_relay_ca: boolean
  has_client_cert: boolean
  has_client_key: boolean
  has_bridge_key: boolean
  relay_ca_action: RelaySecretAction
  client_cert_action: RelaySecretAction
  client_key_action: RelaySecretAction
  bridge_key_action: RelaySecretAction
  relay_ca_pem: string
  client_cert_pem: string
  client_key_pem: string
  bridge_encryption_key: string
}

export function createEmptyRelayForm(): RelayForm {
  return {
    relay_id: '',
    name: '',
    relay_url: '',
    enabled: true,
    tls_mode: 'off',
    bridge_encryption_mode: 'off',
    has_relay_ca: false,
    has_client_cert: false,
    has_client_key: false,
    has_bridge_key: false,
    relay_ca_action: 'replace',
    client_cert_action: 'replace',
    client_key_action: 'replace',
    bridge_key_action: 'replace',
    relay_ca_pem: '',
    client_cert_pem: '',
    client_key_pem: '',
    bridge_encryption_key: '',
  }
}

export function relayToForm(relay: ManagedRelay): RelayForm {
  return {
    ...createEmptyRelayForm(),
    relay_id: relay.relay_id,
    name: relay.name,
    relay_url: relay.relay_url,
    enabled: relay.enabled,
    tls_mode: relay.tls_mode,
    bridge_encryption_mode: relay.bridge_encryption_mode,
    has_relay_ca: relay.has_relay_ca,
    has_client_cert: relay.has_client_cert,
    has_client_key: relay.has_client_key,
    has_bridge_key: relay.has_bridge_key,
    relay_ca_action: relay.has_relay_ca ? 'keep' : 'replace',
    client_cert_action: relay.has_client_cert ? 'keep' : 'replace',
    client_key_action: relay.has_client_key ? 'keep' : 'replace',
    bridge_key_action: relay.has_bridge_key ? 'keep' : 'replace',
  }
}

function secretPatch(
  action: RelaySecretAction,
  value: string,
  existing: boolean,
  creating: boolean,
): ManagedRelaySecretPatch | undefined {
  const trimmed = value.trim()
  if (creating) return trimmed ? { mode: 'replace', value: trimmed } : undefined
  if (action === 'clear') return { mode: 'clear' }
  if (trimmed) return { mode: 'replace', value: trimmed }
  return existing ? { mode: 'keep' } : undefined
}

function relayFields(form: RelayForm) {
  return {
    name: form.name.trim(),
    relay_url: form.relay_url.trim(),
    enabled: form.enabled,
    tls_mode: form.tls_mode,
    bridge_encryption_mode: form.bridge_encryption_mode,
  }
}

export function createRelayRequest(form: RelayForm): ManagedRelayRequest {
  return {
    ...relayFields(form),
    relay_ca_pem: secretPatch(
      form.relay_ca_action,
      form.relay_ca_pem,
      form.has_relay_ca,
      true,
    ),
    client_cert_pem: secretPatch(
      form.client_cert_action,
      form.client_cert_pem,
      form.has_client_cert,
      true,
    ),
    client_key_pem: secretPatch(
      form.client_key_action,
      form.client_key_pem,
      form.has_client_key,
      true,
    ),
    bridge_encryption_key: secretPatch(
      form.bridge_key_action,
      form.bridge_encryption_key,
      form.has_bridge_key,
      true,
    ),
  }
}

export function createRelayPatchRequest(
  form: RelayForm,
): ManagedRelayPatchRequest {
  return {
    ...relayFields(form),
    relay_ca_pem: secretPatch(
      form.relay_ca_action,
      form.relay_ca_pem,
      form.has_relay_ca,
      false,
    ),
    client_cert_pem: secretPatch(
      form.client_cert_action,
      form.client_cert_pem,
      form.has_client_cert,
      false,
    ),
    client_key_pem: secretPatch(
      form.client_key_action,
      form.client_key_pem,
      form.has_client_key,
      false,
    ),
    bridge_encryption_key: secretPatch(
      form.bridge_key_action,
      form.bridge_encryption_key,
      form.has_bridge_key,
      false,
    ),
  }
}
