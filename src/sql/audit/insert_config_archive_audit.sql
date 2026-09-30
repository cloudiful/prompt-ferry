INSERT INTO config_archive_audit (
    actor_user_id,
    action,
    backend_kind,
    format_version,
    archive_bytes,
    payload_fingerprint,
    success,
    error_code,
    error_message,
    domain_summary
)
VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
RETURNING audit_id
