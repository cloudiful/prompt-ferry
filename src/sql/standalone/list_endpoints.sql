SELECT endpoint_id, name, provider, provider_region, base_url, native_api,
       native_api_source, key_lb_enabled, enabled, mcp_enabled,
       api_key_ciphertext, api_key_nonce, api_key_key_version
FROM standalone_provider_endpoints
ORDER BY name;
