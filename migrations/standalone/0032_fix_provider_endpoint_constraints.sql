-- Issue #584: mirror the PostgreSQL constraint fix for the standalone SQLite
-- store. The standalone table already carries the corrected constraints since
-- 0016: `native_api` lists every kind the admin handler can persist, and the
-- region domain spells the NULL case out explicitly
-- (`provider_region IS NULL OR provider_region IN ('cn', 'global')`). SQLite
-- cannot alter a CHECK in place and a rebuild would drop the FK child rows it
-- pauses, so no DDL is needed here; the version advance keeps both schemas on
-- the same constraint generation.
UPDATE standalone_schema_meta
SET schema_version = 32
WHERE schema_key = 'standalone';
