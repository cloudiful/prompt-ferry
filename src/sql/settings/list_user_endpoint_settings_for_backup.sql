-- Explicit per-user endpoint selections for the configuration archive.
--
-- A NULL endpoint_id is a real state ("use the routing default") that must
-- round-trip distinctly from an absent row, so the reader selects the nullable
-- column instead of the per-user getter, which conflates both states.
SELECT user_id, endpoint_id
FROM public.user_endpoint_settings
ORDER BY user_id;
