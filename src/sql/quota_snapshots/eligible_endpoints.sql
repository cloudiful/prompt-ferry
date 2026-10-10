SELECT endpoint.endpoint_id
FROM provider_endpoints AS endpoint
WHERE endpoint.enabled = TRUE
  AND endpoint.provider = 'openai'
  AND EXISTS (
      SELECT 1
      FROM endpoint_oauth_tokens AS oauth
      WHERE oauth.endpoint_id = endpoint.endpoint_id
        AND oauth.refresh_token IS NOT NULL
  )
ORDER BY endpoint.endpoint_id;
