INSERT INTO standalone_config_archive_audit (
    actor_user_id,
    action,
    backend_kind,
    format_version,
    archive_bytes,
    payload_fingerprint,
    success,
    error_code,
    error_message,
    domain_summary,
    created_at
)
VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
