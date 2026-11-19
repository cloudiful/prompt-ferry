SELECT
    (SELECT extversion FROM pg_extension WHERE extname = 'timescaledb') AS installed_version,
    current_setting('timescaledb.license', true) AS license
