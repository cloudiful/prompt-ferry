use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::db::types::{ConversationRedactionSessionCreate, ConversationRedactionSessionRow};

/// Issue #524 Task 6: enough of the stored session to decide whether an upsert
/// would rewrite an identical payload. `session_ciphertext_sha256` is computed
/// by PostgreSQL (`sha256(bytea)`) so the fingerprint check never transfers the
/// multi-megabyte ciphertext itself.
#[derive(Debug, sqlx::FromRow)]
struct ConversationRedactionSessionFingerprint {
    policy_version: i64,
    session_key_version: i16,
    last_event_id: Option<i64>,
    session_ciphertext_sha256: Vec<u8>,
}

fn session_ciphertext_fingerprint(ciphertext: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(ciphertext);
    let mut fingerprint = [0_u8; 32];
    fingerprint.copy_from_slice(&digest);
    fingerprint
}

/// The conditional upsert only writes when `policy_version` changes or when
/// `last_event_id` moves forward. A missing counter on either side still forces
/// the write, so only a strictly newer incoming id counts as an advance.
fn last_event_id_advances(stored: Option<i64>, incoming: Option<i64>) -> bool {
    match (stored, incoming) {
        (Some(stored), Some(incoming)) => incoming > stored,
        _ => true,
    }
}

pub async fn get_conversation_redaction_session(
    pool: &PgPool,
    conversation_id: Uuid,
    policy_version: i64,
) -> Result<Option<ConversationRedactionSessionRow>> {
    sqlx::query_file_as!(
        ConversationRedactionSessionRow,
        "src/sql/usage/get_conversation_redaction_session.sql",
        conversation_id,
        policy_version
    )
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

/// Returns the affected row count. `0` means the conditional upsert kept a row
/// that was already newer (higher `last_event_id` for the same policy version),
/// so the caller can warn about a dropped mapping.
///
/// Issue #524 Task 6: when the stored row already carries the same encrypted
/// session (identical ciphertext fingerprint, key version, and policy version)
/// and the incoming event id does not advance the counter, the write would only
/// touch `updated_at`. Read the fingerprint first and skip it, without touching
/// the multi-megabyte `session_ciphertext`.
pub async fn upsert_conversation_redaction_session(
    pool: &PgPool,
    input: ConversationRedactionSessionCreate,
) -> Result<u64> {
    if let Some(existing) = sqlx::query_file_as!(
        ConversationRedactionSessionFingerprint,
        "src/sql/usage/get_conversation_redaction_session_fingerprint.sql",
        input.conversation_id,
    )
    .fetch_optional(pool)
    .await?
        && existing.policy_version == input.policy_version
        && existing.session_key_version == input.session_key_version
        && !last_event_id_advances(existing.last_event_id, input.last_event_id)
        && existing.session_ciphertext_sha256
            == session_ciphertext_fingerprint(&input.session_ciphertext)
    {
        return Ok(0);
    }

    let result = sqlx::query_file!(
        "src/sql/usage/upsert_conversation_redaction_session.sql",
        input.conversation_id,
        input.session_ciphertext,
        input.session_nonce,
        input.session_key_version,
        input.last_event_id,
        input.policy_version,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

pub async fn delete_conversation_redaction_session(
    pool: &PgPool,
    conversation_id: Uuid,
) -> Result<u64> {
    let result = sqlx::query_file!(
        "src/sql/usage/delete_conversation_redaction_session.sql",
        conversation_id
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_sha256_of_ciphertext() {
        assert_eq!(
            session_ciphertext_fingerprint(b"abc"),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
        assert_ne!(
            session_ciphertext_fingerprint(b"abc"),
            session_ciphertext_fingerprint(b"abd")
        );
    }

    #[test]
    fn only_a_newer_incoming_event_id_advances() {
        assert!(last_event_id_advances(Some(10), Some(11)));
        assert!(last_event_id_advances(None, Some(1)));
        assert!(last_event_id_advances(Some(10), None));
        assert!(last_event_id_advances(None, None));
        assert!(!last_event_id_advances(Some(10), Some(10)));
        assert!(!last_event_id_advances(Some(10), Some(3)));
    }
}
