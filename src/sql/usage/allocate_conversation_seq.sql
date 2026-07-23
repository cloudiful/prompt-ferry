INSERT INTO conversation_counters (
    conversation_id,
    next_seq,
    updated_at
)
VALUES ($1, GREATEST(COALESCE($2, 1), 1) + 1, NOW())
ON CONFLICT (conversation_id)
DO UPDATE SET
    next_seq = GREATEST(conversation_counters.next_seq, GREATEST(COALESCE($2, 1), 1)) + 1,
    updated_at = NOW()
RETURNING next_seq - 1 AS "conversation_seq!"
