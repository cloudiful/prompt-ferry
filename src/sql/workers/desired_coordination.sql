-- Read the desired coordination state of the deployment.
--
-- The revision and whether a URL is stored are plain fields; admission never
-- needs the encrypted URL. A missing row is the bootstrap mode.
select
    coalesce((setting_value ->> 'revision')::bigint, 0) as revision,
    (setting_value ? 'url' and setting_value -> 'url' is not null) as configured
from worker_settings
where setting_key = $1
