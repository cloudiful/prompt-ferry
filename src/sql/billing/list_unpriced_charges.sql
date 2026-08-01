SELECT c.charge_id AS "charge_id!", c.requested_model,
       c.created_at AS "created_at!",
       c.input_tokens AS "input_tokens!", c.cache_read_tokens AS "cache_read_tokens!",
       c.cache_write_tokens AS "cache_write_tokens!", c.output_tokens AS "output_tokens!"
FROM usage_charges c
WHERE c.usage_status = 'known' AND c.pricing_status = 'unpriced'
ORDER BY c.created_at ASC
LIMIT $1
