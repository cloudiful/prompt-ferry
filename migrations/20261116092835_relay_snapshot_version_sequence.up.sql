-- Allocate relay snapshot publication fences from a non-transactional
-- sequence, so a publisher whose advisory-lock connection was lost can never
-- mint a newer fence for contents it read before the loss.
create sequence relay_snapshot_version_seq as bigint;

-- Continue after the settings-based internal counter when this deployment
-- already published snapshots, so no fence is reused across the upgrade.
select setval(
    'relay_snapshot_version_seq',
    coalesce(
        (
            select (setting_value ->> 'version')::bigint
            from worker_settings
            where setting_key = 'relay_snapshot_version'
        ),
        0
    ) + 1,
    false
);

-- The sequence now owns the coordination state; the settings row is legacy.
delete from worker_settings where setting_key = 'relay_snapshot_version';
