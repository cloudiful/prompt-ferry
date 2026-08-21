INSERT OR IGNORE INTO standalone_users(
    user_id,
    login_name,
    display_name,
    password_hash,
    is_admin,
    enabled
)
SELECT orphan.user_id,
       'legacy_user_' || CAST(orphan.user_id AS TEXT),
       'Legacy user ' || CAST(orphan.user_id AS TEXT),
       '!',
       0,
       0
FROM (
    SELECT DISTINCT user_id
    FROM standalone_client_keys
) AS orphan
WHERE NOT EXISTS (
    SELECT 1
    FROM standalone_users
    WHERE standalone_users.user_id = orphan.user_id
);

CREATE TABLE standalone_client_keys_v3 (
    key_id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES standalone_users(user_id) ON DELETE CASCADE,
    key_prefix TEXT NOT NULL,
    label TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    secret_ciphertext BLOB NOT NULL,
    secret_nonce BLOB NOT NULL,
    secret_key_version INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO standalone_client_keys_v3(
    key_id,
    user_id,
    key_prefix,
    label,
    enabled,
    secret_ciphertext,
    secret_nonce,
    secret_key_version,
    created_at,
    updated_at
)
SELECT key_id,
       user_id,
       key_prefix,
       label,
       enabled,
       secret_ciphertext,
       secret_nonce,
       secret_key_version,
       created_at,
       updated_at
FROM standalone_client_keys;

DROP TABLE standalone_client_keys;
ALTER TABLE standalone_client_keys_v3 RENAME TO standalone_client_keys;

UPDATE standalone_schema_meta
SET schema_version = 3
WHERE schema_key = 'standalone';
