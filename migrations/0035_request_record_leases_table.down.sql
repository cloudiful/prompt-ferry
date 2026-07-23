DROP INDEX IF EXISTS idx_request_record_leases_expires_at;
DROP TABLE IF EXISTS request_record_leases;

CREATE INDEX IF NOT EXISTS idx_request_records_active_lease
ON request_records(lease_expires_at ASC, updated_at ASC)
WHERE event_kind = 'request'
  AND request_state IN ('received', 'awaiting_approval', 'upstream_processing');
