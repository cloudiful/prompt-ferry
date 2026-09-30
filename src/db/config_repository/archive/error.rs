//! Stable error surface for the encrypted configuration archive.
//!
//! Every variant maps to a machine-readable code the admin API returns, and no
//! message ever carries the passphrase or any archive byte.

use super::super::snapshot::ConfigBackendKind;

#[derive(Debug)]
pub enum ArchiveError {
    PassphraseTooShort {
        min: usize,
    },
    PassphraseTooLong {
        max: usize,
    },
    InvalidHeader,
    UnsupportedVersion {
        found: u16,
        supported: u16,
    },
    BackendMismatch {
        expected: ConfigBackendKind,
        found: ConfigBackendKind,
    },
    DecryptionFailed,
    TooLarge {
        limit: usize,
    },
    InvalidPayload(&'static str),
    KeyDerivation,
    Compression,
    Serialization,
}

impl ArchiveError {
    /// Stable machine-readable code for the admin API error body. Never
    /// contains archive contents or the passphrase.
    pub fn code(&self) -> &'static str {
        match self {
            Self::PassphraseTooShort { .. } => "archive_passphrase_too_short",
            Self::PassphraseTooLong { .. } => "archive_passphrase_too_long",
            Self::InvalidHeader => "archive_invalid_header",
            Self::UnsupportedVersion { .. } => "archive_unsupported_version",
            Self::BackendMismatch { .. } => "archive_backend_mismatch",
            Self::DecryptionFailed => "archive_decrypt_failed",
            Self::TooLarge { .. } => "archive_too_large",
            Self::InvalidPayload(_) => "archive_invalid_payload",
            Self::KeyDerivation => "archive_key_derivation_failed",
            Self::Compression => "archive_compression_failed",
            Self::Serialization => "archive_serialization_failed",
        }
    }
}

impl std::fmt::Display for ArchiveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PassphraseTooShort { min } => {
                write!(
                    formatter,
                    "archive passphrase must be at least {min} characters"
                )
            }
            Self::PassphraseTooLong { max } => {
                write!(
                    formatter,
                    "archive passphrase must be at most {max} characters"
                )
            }
            Self::InvalidHeader => formatter.write_str("not a prompt-ferry configuration archive"),
            Self::UnsupportedVersion { found, supported } => write!(
                formatter,
                "unsupported configuration archive version {found} (supported {supported})"
            ),
            Self::BackendMismatch { expected, found } => write!(
                formatter,
                "configuration archive was written by the {found} backend but the target is {expected}"
            ),
            Self::DecryptionFailed => {
                formatter.write_str("configuration archive could not be decrypted")
            }
            Self::TooLarge { limit } => {
                write!(
                    formatter,
                    "configuration archive exceeds the {limit} byte limit"
                )
            }
            Self::InvalidPayload(reason) => {
                write!(
                    formatter,
                    "configuration archive payload is invalid: {reason}"
                )
            }
            Self::KeyDerivation => {
                formatter.write_str("failed to derive the configuration archive key")
            }
            Self::Compression => {
                formatter.write_str("failed to compress or decompress the configuration archive")
            }
            Self::Serialization => {
                formatter.write_str("failed to encode or decode the configuration archive payload")
            }
        }
    }
}

impl std::error::Error for ArchiveError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable_and_never_carry_payload_data() {
        assert_eq!(
            ArchiveError::DecryptionFailed.code(),
            "archive_decrypt_failed"
        );
        assert_eq!(
            ArchiveError::UnsupportedVersion {
                found: 9,
                supported: 1,
            }
            .code(),
            "archive_unsupported_version"
        );
        let message =
            ArchiveError::InvalidPayload("backend tag does not match the payload").to_string();
        assert!(message.contains("backend tag"));
    }
}
