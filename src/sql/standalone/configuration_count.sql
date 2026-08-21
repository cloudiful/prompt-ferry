SELECT (
    (SELECT COUNT(*) FROM standalone_relays)
    + (SELECT COUNT(*) FROM standalone_provider_endpoints)
    + (SELECT COUNT(*) FROM standalone_model_routes)
    + (SELECT COUNT(*) FROM standalone_client_keys)
    + (SELECT COUNT(*) FROM standalone_mcp_servers)
    + (SELECT COUNT(*) FROM standalone_settings)
) AS record_count;
