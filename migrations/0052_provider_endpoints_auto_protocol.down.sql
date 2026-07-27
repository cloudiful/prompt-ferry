UPDATE provider_endpoints
SET native_api = 'responses', native_api_source = 'manual'
WHERE native_api = 'auto' OR native_api_source = 'auto';

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_native_api;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_native_api
CHECK (native_api IN ('responses', 'chat', 'anthropic_messages'));

ALTER TABLE provider_endpoints
DROP CONSTRAINT IF EXISTS ck_provider_endpoints_native_api_source;

ALTER TABLE provider_endpoints
ADD CONSTRAINT ck_provider_endpoints_native_api_source
CHECK (native_api_source IN ('detected', 'manual'));
