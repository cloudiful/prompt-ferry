-- Serialize identity writers with the configuration replace.
--
-- SHARE ROW EXCLUSIVE conflicts with the INSERT/UPDATE/DELETE of ordinary
-- user/client-key writers but not with SELECT, so the importer can read and
-- restart both sequences without a concurrent default nextval() racing it.
-- Every import takes the same fixed table order.
LOCK TABLE public.users, public.client_keys IN SHARE ROW EXCLUSIVE MODE;
