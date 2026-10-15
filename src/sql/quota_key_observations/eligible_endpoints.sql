SELECT
    endpoint.endpoint_id,
    api_key.key_id
FROM provider_endpoints AS endpoint
JOIN endpoint_api_keys AS api_key ON api_key.endpoint_id = endpoint.endpoint_id
WHERE endpoint.enabled = TRUE
  AND api_key.enabled = TRUE
  AND endpoint.provider IN ('minimax', 'command_code', 'opencode_go', 'openrouter', 'glm', 'deepseek')
  AND NULLIF(BTRIM(api_key.api_key), '') IS NOT NULL
ORDER BY endpoint.endpoint_id, api_key.key_id;
