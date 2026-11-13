ALTER TABLE model_route_targets
    ADD COLUMN prefer_new_sessions BOOLEAN NOT NULL DEFAULT FALSE;

CREATE UNIQUE INDEX ux_model_route_targets_preferred_new_session
    ON model_route_targets(rule_id)
    WHERE prefer_new_sessions;

ALTER TABLE request_records
    DROP CONSTRAINT IF EXISTS ck_request_records_route_selection_reason;

ALTER TABLE request_records
    ADD CONSTRAINT ck_request_records_route_selection_reason CHECK (
        route_selection_reason IN (
            'default',
            'session_affinity',
            'session_load_balance',
            'conversation_override',
            'quota_failover',
            'new_session_preference'
        )
    );
