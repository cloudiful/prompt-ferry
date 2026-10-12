SELECT endpoint_id
FROM provider_endpoints
WHERE endpoint_id = $1
FOR UPDATE
