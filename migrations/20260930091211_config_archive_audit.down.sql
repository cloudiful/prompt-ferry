-- Issue #661 Phase P3: revert of the administrator configuration archive audit
-- trail. The table owns no other object, so a plain drop is the exact inverse.
DROP TABLE IF EXISTS config_archive_audit;
