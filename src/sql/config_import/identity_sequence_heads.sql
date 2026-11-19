-- Actual identity heads after the archived rows were inserted.
--
-- `pg_sequences.last_value` is NULL for a never-called sequence, which would
-- read as "no head" and could restart an advanced uncalled sequence backwards.
-- Reading the sequence relations directly returns the real last_value and
-- is_called pair.
SELECT
    (SELECT MAX(user_id) FROM public.users) AS max_user_id,
    (SELECT MAX(key_id) FROM public.client_keys) AS max_client_key_id,
    users_seq.last_value AS user_last_value,
    users_seq.is_called AS user_is_called,
    keys_seq.last_value AS client_key_last_value,
    keys_seq.is_called AS client_key_is_called
FROM public.users_user_id_seq AS users_seq
CROSS JOIN public.client_keys_key_id_seq AS keys_seq;
