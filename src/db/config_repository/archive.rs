//! Versioned, passphrase-encrypted container for [`ConfigSnapshot`] payloads.
//!
//! The module owns the archive's public surface — seal, open, and the
//! passphrase/size contract — while the binary layout and the error vocabulary
//! live in [`codec`] and [`error`]. Neither the passphrase nor any payload byte
//! is ever logged or echoed.

mod codec;
mod error;

pub use codec::{
    ARCHIVE_FORMAT_VERSION, ARCHIVE_MAGIC, EncodedArchive, MAX_ARCHIVE_BYTES, MAX_PASSPHRASE_LEN,
    MAX_PLAINTEXT_BYTES, MIN_PASSPHRASE_LEN, decode_archive, decode_archive_for_backend,
    encode_archive, validate_passphrase,
};
pub use error::ArchiveError;
