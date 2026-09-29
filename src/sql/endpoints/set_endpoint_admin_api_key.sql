UPDATE provider_endpoints
SET admin_api_key = $2,
    updated_at = NOW()
WHERE endpoint_id = $1
