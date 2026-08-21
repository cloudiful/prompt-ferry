CREATE TABLE IF NOT EXISTS standalone_users (
    user_id INTEGER PRIMARY KEY,
    login_name TEXT NOT NULL COLLATE NOCASE UNIQUE,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    is_admin INTEGER NOT NULL CHECK (is_admin IN (0, 1)),
    enabled INTEGER NOT NULL CHECK (enabled IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_standalone_users_enabled_admin
    ON standalone_users(enabled, is_admin, login_name);

UPDATE standalone_schema_meta
SET schema_version = 2
WHERE schema_key = 'standalone';
