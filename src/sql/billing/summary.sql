SELECT COUNT(*)::BIGINT AS "request_count!",
       COUNT(*) FILTER (WHERE c.usage_status = 'known')::BIGINT AS "known_count!",
       COUNT(*) FILTER (WHERE c.usage_status = 'unknown')::BIGINT AS "unknown_count!",
       COUNT(*) FILTER (WHERE c.pricing_status = 'priced')::BIGINT AS "priced_count!",
       COUNT(*) FILTER (WHERE c.pricing_status = 'unpriced')::BIGINT AS "unpriced_count!",
       COALESCE(SUM(c.customer_amount), 0)::NUMERIC AS "customer_amount!"
FROM usage_charges c
WHERE ($1::BIGINT IS NULL OR c.user_id = $1)
  AND ($2::BIGINT IS NULL OR c.client_key_id = $2)
  AND ($3::TEXT IS NULL OR c.requested_model = $3)
  AND ($4::UUID IS NULL OR c.endpoint_id = $4)
  AND ($5::TEXT IS NULL OR c.usage_status = $5)
  AND ($6::TEXT IS NULL OR c.pricing_status = $6)
  AND ($7::UUID IS NULL OR c.request_id = $7)
  AND ($8::TIMESTAMPTZ IS NULL OR c.created_at >= $8)
  AND ($9::TIMESTAMPTZ IS NULL OR c.created_at < $9)
