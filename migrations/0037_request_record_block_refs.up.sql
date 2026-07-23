CREATE TABLE IF NOT EXISTS request_record_block_refs (
    event_id  BIGINT NOT NULL REFERENCES request_records(event_id) ON DELETE CASCADE,
    block_hash TEXT NOT NULL REFERENCES usage_prompt_blocks(block_hash),
    PRIMARY KEY (event_id, block_hash)
);
