SELECT COUNT(*)::BIGINT AS "count!"
FROM request_records rr
WHERE rr.created_at < $1::TIMESTAMPTZ
  AND EXISTS (
      SELECT 1
      FROM usage_charges charge
      WHERE charge.event_id = rr.event_id
  );
