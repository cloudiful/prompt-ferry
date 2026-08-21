DELETE FROM standalone_provider_endpoints
WHERE endpoint_id NOT IN (
    SELECT endpoint_id FROM standalone_snapshot_endpoint_ids
);
