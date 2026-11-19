-- Export readonly verification: every maintenance-export session must report
-- 'on' before it is handed out. Statement fixed at build time.
SHOW default_transaction_read_only;
