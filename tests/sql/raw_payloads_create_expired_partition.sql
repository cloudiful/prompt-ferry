CREATE TABLE request_record_raw_payloads_20000101
PARTITION OF request_record_raw_payloads
FOR VALUES FROM ('2000-01-01 00:00:00+00') TO ('2000-01-02 00:00:00+00');
