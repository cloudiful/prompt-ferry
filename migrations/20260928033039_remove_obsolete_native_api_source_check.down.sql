-- Reverse the obsolete-check removal: re-add the narrow 0001-era CHECK
-- only when no row uses a wider value. Rows written under the canonical
-- shape (`auto`) would violate the narrow CHECK, and business data must
-- never be dropped or rewritten by a migration, so abort loudly with the
-- offending values instead of touching rows.
DO $$
DECLARE
    offenders TEXT;
BEGIN
    SELECT string_agg(DISTINCT native_api_source, ', ' ORDER BY native_api_source)
    INTO offenders
    FROM provider_endpoints
    WHERE native_api_source NOT IN ('detected', 'manual');
    IF offenders IS NOT NULL THEN
        RAISE EXCEPTION 'cannot restore legacy native_api_source CHECK: rows use wider values: %', offenders;
    END IF;
END $$;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS provider_endpoints_native_api_source_check;

ALTER TABLE provider_endpoints
ADD CONSTRAINT provider_endpoints_native_api_source_check
CHECK (native_api_source IN ('detected', 'manual'));
