SELECT audit.audit_id,
       audit.actor_user_id,
       actor.login_name AS actor_login_name,
       audit.action,
       audit.backend_kind,
       audit.format_version,
       audit.archive_bytes,
       audit.payload_fingerprint,
       audit.success,
       audit.error_code,
       audit.error_message,
       audit.domain_summary,
       audit.created_at
FROM config_archive_audit audit
LEFT JOIN users actor ON actor.user_id = audit.actor_user_id
ORDER BY audit.created_at DESC, audit.audit_id DESC
LIMIT $2 OFFSET $1
