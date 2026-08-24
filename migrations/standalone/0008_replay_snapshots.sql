-- Phase 1C-b: durable standalone replay snapshots.
--
-- The standalone SQLite worker persists a single replay checkpoint per
-- conversation so that process restart does not discard the latest
-- prompt-reference snapshot needed to reconstruct a conversation's
-- recent prompt chain. The PostgreSQL replay snapshot history remains
-- untouched; this table is the SQLite-only durable equivalent of
-- `request_record_replay_snapshots` (migration 0009).
--
-- The table is keyed by `conversation_id` (one snapshot per conversation)
-- because the standalone path keeps only the latest checkpoint per
-- conversation rather than the historical append-only stream stored on
-- PostgreSQL. The monotonic upsert in
-- `src/sql/standalone/upsert_replay_snapshot.sql` rejects incoming
-- snapshots that would regress the stored checkpoint by sequence or,
-- for equal sequence numbers, by base event id.
--
-- `prompt_refs_json` carries only role and block-hash references (the
-- same shape as the PostgreSQL snapshot column and as the in-memory
-- `ReplaySnapshotValue::prompt_refs`); raw request or response bodies,
-- encrypted upstream sessions, billing, approval, and quota state
-- remain out of scope for this slice.
--
-- `ref_count` and `byte_size` are integer snapshots of the prompt-ref
-- array length and serialized JSON byte size so that consumers can
-- make retention or affinity decisions without re-decoding the JSON
-- payload. CHECK constraints reject negative values so a corrupt
-- row will fail at insert time rather than silently round-trip.

CREATE TABLE IF NOT EXISTS standalone_replay_snapshots (
    conversation_id TEXT PRIMARY KEY,
    base_event_id INTEGER NOT NULL CHECK (base_event_id >= 0),
    conversation_seq INTEGER NOT NULL CHECK (conversation_seq > 0),
    prompt_refs_json TEXT NOT NULL CHECK (length(prompt_refs_json) > 0),
    ref_count INTEGER NOT NULL CHECK (ref_count >= 0),
    byte_size INTEGER NOT NULL CHECK (byte_size >= 0),
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_standalone_replay_snapshots_seq
    ON standalone_replay_snapshots(conversation_seq);

UPDATE standalone_schema_meta
SET schema_version = 8
WHERE schema_key = 'standalone';
