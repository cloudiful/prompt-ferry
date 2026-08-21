INSERT INTO standalone_provider_endpoints (
    endpoint_id, name, provider, provider_region, base_url, native_api,
    native_api_source, key_lb_enabled, enabled, mcp_enabled,
    api_key_ciphertext, api_key_nonce, api_key_key_version, updated_at
) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
ON CONFLICT(endpoint_id) DO UPDATE SET
    name = excluded.name,
    provider = excluded.provider,
    provider_region = excluded.provider_region,
    base_url = excluded.base_url,
    native_api = excluded.native_api,
    native_api_source = excluded.native_api_source,
    key_lb_enabled = excluded.key_lb_enabled,
    enabled = excluded.enabled,
    mcp_enabled = excluded.mcp_enabled,
    api_key_ciphertext = excluded.api_key_ciphertext,
    api_key_nonce = excluded.api_key_nonce,
    api_key_key_version = excluded.api_key_key_version,
    updated_at = CURRENT_TIMESTAMP;
