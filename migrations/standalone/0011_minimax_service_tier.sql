ALTER TABLE standalone_provider_endpoints ADD COLUMN service_tier TEXT NOT NULL DEFAULT 'standard';

UPDATE standalone_provider_endpoints
SET service_tier = 'standard'
WHERE service_tier IS NULL OR service_tier NOT IN ('standard', 'priority');

UPDATE standalone_schema_meta
SET schema_version = 11
WHERE schema_key = 'standalone';
