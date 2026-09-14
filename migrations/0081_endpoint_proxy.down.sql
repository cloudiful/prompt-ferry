ALTER TABLE model_route_targets DROP COLUMN IF EXISTS proxy_url_override;
ALTER TABLE provider_endpoints DROP COLUMN IF EXISTS proxy_url;
