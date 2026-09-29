-- Reverse the obsolete-check removal: re-add the narrow 0001-era CHECK
-- only when no row uses a wider value. Rows written under the canonical
-- shape (`anthropic_messages`, `auto`) would violate the narrow CHECK, and
-- business data must never be dropped or rewritten by a migration, so abort
-- loudly with the offending values instead of touching rows.
DO $$
DECLARE
    offenders TEXT;
BEGIN
    SELECT string_agg(DISTINCT native_api, ', ' ORDER BY native_api)
    INTO offenders
    FROM provider_endpoints
    WHERE native_api NOT IN ('responses', 'chat');
    IF offenders IS NOT NULL THEN
        RAISE EXCEPTION 'cannot restore legacy native_api CHECK: rows use wider values: %', offenders;
    END IF;
END $$;

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS provider_endpoints_native_api_check;

ALTER TABLE provider_endpoints
ADD CONSTRAINT provider_endpoints_native_api_check
CHECK (native_api IN ('responses', 'chat'));
