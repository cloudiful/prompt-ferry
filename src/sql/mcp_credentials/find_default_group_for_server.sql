SELECT group_id
FROM mcp_quota_groups
WHERE group_id = md5('group:' || $1::uuid::text)::uuid
