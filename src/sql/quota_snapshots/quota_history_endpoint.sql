SELECT
    endpoint_id,
    provider
FROM provider_endpoints
WHERE endpoint_id = $1;
