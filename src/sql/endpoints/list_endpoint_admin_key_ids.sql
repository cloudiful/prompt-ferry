SELECT endpoint_id
FROM provider_endpoints
WHERE admin_api_key IS NOT NULL
  AND btrim(admin_api_key) <> ''
