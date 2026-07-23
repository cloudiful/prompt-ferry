ALTER TABLE request_records
DROP COLUMN http_request_compression_ratio;

ALTER TABLE request_records
DROP COLUMN http_request_decompressed_bytes;

ALTER TABLE request_records
DROP COLUMN http_request_compressed_bytes;

ALTER TABLE request_records
DROP COLUMN http_request_compressed;

ALTER TABLE request_records
DROP COLUMN http_request_content_encoding;
