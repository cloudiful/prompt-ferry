DELETE FROM usage_prompt_blocks upb
WHERE upb.created_at < NOW() - make_interval(mins => $1::INT)
  AND NOT EXISTS (
      SELECT 1
      FROM request_record_block_refs ref
      WHERE ref.block_hash = upb.block_hash
  );
