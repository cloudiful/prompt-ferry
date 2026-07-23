DELETE FROM request_records
WHERE ($1::TIMESTAMPTZ IS NULL OR created_at >= $1)
  AND ($2::TIMESTAMPTZ IS NULL OR created_at <= $2)
