DELETE FROM request_record_raw_payloads_default
WHERE created_at >= $1
  AND created_at < $2;
