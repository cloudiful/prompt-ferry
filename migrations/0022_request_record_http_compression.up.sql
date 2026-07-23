ALTER TABLE request_records
ADD COLUMN http_request_content_encoding TEXT;

ALTER TABLE request_records
ADD COLUMN http_request_compressed BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE request_records
ADD COLUMN http_request_compressed_bytes BIGINT;

ALTER TABLE request_records
ADD COLUMN http_request_decompressed_bytes BIGINT;

ALTER TABLE request_records
ADD COLUMN http_request_compression_ratio DOUBLE PRECISION;
