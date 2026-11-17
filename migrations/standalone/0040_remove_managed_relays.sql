-- Relay connection targets are local worker configuration, so the SQLite
-- configuration store no longer persists managed relay rows.
DROP TABLE IF EXISTS standalone_relays;

UPDATE standalone_schema_meta
SET schema_version = 40
WHERE schema_key = 'standalone';
