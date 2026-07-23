DELETE FROM request_records
WHERE user_id = $1
  AND ($2::TIMESTAMPTZ IS NULL OR created_at >= $2)
  AND ($3::TIMESTAMPTZ IS NULL OR created_at <= $3)
