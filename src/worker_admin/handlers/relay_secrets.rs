use super::*;

pub(super) fn encrypt_create_secret(
    manager: &crate::relay_secrets::RelaySecretManager,
    patch: Option<ManagedRelaySecretPatch>,
    field_name: &str,
) -> Result<Option<crate::relay_secrets::EncryptedSecretEnvelope>, Box<Response>> {
    match patch {
        None => Ok(None),
        Some(ManagedRelaySecretPatch::Replace { value }) => {
            encrypt_secret_value(manager, value, field_name)
        }
        Some(ManagedRelaySecretPatch::Keep) | Some(ManagedRelaySecretPatch::Clear) => {
            Err(Box::new(bad_request(&format!(
                "{field_name} cannot use keep/clear on create"
            ))))
        }
    }
}

pub(super) fn resolve_secret_patch(
    manager: &crate::relay_secrets::RelaySecretManager,
    patch: Option<ManagedRelaySecretPatch>,
    existing: Option<crate::relay_secrets::EncryptedSecretEnvelope>,
    field_name: &str,
) -> Result<Option<crate::relay_secrets::EncryptedSecretEnvelope>, Box<Response>> {
    match patch {
        None | Some(ManagedRelaySecretPatch::Keep) => Ok(existing),
        Some(ManagedRelaySecretPatch::Clear) => Ok(None),
        Some(ManagedRelaySecretPatch::Replace { value }) => {
            encrypt_secret_value(manager, value, field_name)
        }
    }
}

fn encrypt_secret_value(
    manager: &crate::relay_secrets::RelaySecretManager,
    value: String,
    field_name: &str,
) -> Result<Option<crate::relay_secrets::EncryptedSecretEnvelope>, Box<Response>> {
    let value = value.trim().to_string();
    if value.is_empty() {
        return Err(Box::new(bad_request(&format!(
            "{field_name} cannot be empty"
        ))));
    }
    manager
        .encrypt(&value)
        .map(Some)
        .map_err(|err| Box::new(bad_request(&err.to_string())))
}
