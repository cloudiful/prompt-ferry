-- Issue #589: drop the optional OpenAI Admin API key column.
ALTER TABLE provider_endpoints
    DROP COLUMN admin_api_key;
