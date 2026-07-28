CREATE INDEX IF NOT EXISTS idx_approval_requests_resolved_created_at
ON approval_requests(created_at ASC, approval_id ASC)
WHERE approval_status <> 'pending';
