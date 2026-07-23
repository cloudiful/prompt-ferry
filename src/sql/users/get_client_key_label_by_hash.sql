SELECT label
FROM client_keys
WHERE key_hash = $1
LIMIT 1
