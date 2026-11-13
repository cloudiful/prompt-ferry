ALTER TABLE standalone_model_route_targets
    ADD COLUMN prefer_new_sessions INTEGER NOT NULL DEFAULT 0;

CREATE UNIQUE INDEX IF NOT EXISTS ux_standalone_model_route_targets_preferred
    ON standalone_model_route_targets(rule_id)
    WHERE prefer_new_sessions = 1;

UPDATE standalone_schema_meta
SET schema_version = 39
WHERE schema_key = 'standalone';
