SELECT
    endpoint.endpoint_id,
    api_key.key_id,
    endpoint.provider
FROM provider_endpoints AS endpoint
JOIN endpoint_api_keys AS api_key ON api_key.endpoint_id = endpoint.endpoint_id
WHERE endpoint.endpoint_id = $1
  AND api_key.key_id = $2;
