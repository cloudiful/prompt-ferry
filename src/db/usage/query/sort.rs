pub(super) fn request_records_order_by_clause(sort_field: &str, sort_order: i64) -> &'static str {
    match (sort_field, sort_order == 1) {
        ("created_at", true) => "rr.created_at ASC, rr.event_id ASC",
        ("usage_date", true) => {
            "date_trunc('day', rr.created_at) ASC, rr.created_at ASC, rr.event_id ASC"
        }
        ("user_key", true) => {
            "COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') ASC, rr.created_at DESC, rr.event_id DESC"
        }
        ("client_key_label", true) => {
            "COALESCE(rr.client_key_label, '-') ASC, rr.created_at DESC, rr.event_id DESC"
        }
        ("model_key", true) => "COALESCE(rr.model, '-') ASC, rr.created_at DESC, rr.event_id DESC",
        ("mcp_protocol_method", true) => {
            "COALESCE(rr.mcp_protocol_method, '-') ASC, rr.created_at DESC, rr.event_id DESC"
        }
        ("mcp_operation_name", true) => {
            "COALESCE(rr.mcp_operation_name, '-') ASC, rr.created_at DESC, rr.event_id DESC"
        }
        ("target", true) => {
            "COALESCE(rr.mcp_server_name, pe.name, rr.model, rr.path) ASC, rr.created_at DESC, rr.event_id DESC"
        }
        ("request_state", true) => "rr.request_state ASC, rr.created_at DESC, rr.event_id DESC",
        ("status", true) => "rr.status ASC NULLS LAST, rr.created_at DESC, rr.event_id DESC",
        ("duration_ms", true) => {
            "rr.duration_ms ASC NULLS LAST, rr.created_at DESC, rr.event_id DESC"
        }
        ("first_chunk_ms", true) => {
            "rr.first_chunk_ms ASC NULLS LAST, rr.created_at DESC, rr.event_id DESC"
        }
        ("total_tokens", true) => {
            "rr.total_tokens ASC NULLS LAST, rr.created_at DESC, rr.event_id DESC"
        }
        ("created_at", false) => "rr.created_at DESC, rr.event_id DESC",
        ("usage_date", false) => {
            "date_trunc('day', rr.created_at) DESC, rr.created_at DESC, rr.event_id DESC"
        }
        ("user_key", false) => {
            "COALESCE(u.login_name, '#' || rr.user_id::TEXT, '-') DESC, rr.created_at DESC, rr.event_id DESC"
        }
        ("client_key_label", false) => {
            "COALESCE(rr.client_key_label, '-') DESC, rr.created_at DESC, rr.event_id DESC"
        }
        ("model_key", false) => {
            "COALESCE(rr.model, '-') DESC, rr.created_at DESC, rr.event_id DESC"
        }
        ("mcp_protocol_method", false) => {
            "COALESCE(rr.mcp_protocol_method, '-') DESC, rr.created_at DESC, rr.event_id DESC"
        }
        ("mcp_operation_name", false) => {
            "COALESCE(rr.mcp_operation_name, '-') DESC, rr.created_at DESC, rr.event_id DESC"
        }
        ("target", false) => {
            "COALESCE(rr.mcp_server_name, pe.name, rr.model, rr.path) DESC, rr.created_at DESC, rr.event_id DESC"
        }
        ("request_state", false) => "rr.request_state DESC, rr.created_at DESC, rr.event_id DESC",
        ("status", false) => "rr.status DESC NULLS LAST, rr.created_at DESC, rr.event_id DESC",
        ("duration_ms", false) => {
            "rr.duration_ms DESC NULLS LAST, rr.created_at DESC, rr.event_id DESC"
        }
        ("first_chunk_ms", false) => {
            "rr.first_chunk_ms DESC NULLS LAST, rr.created_at DESC, rr.event_id DESC"
        }
        ("total_tokens", false) => {
            "rr.total_tokens DESC NULLS LAST, rr.created_at DESC, rr.event_id DESC"
        }
        _ => "rr.created_at DESC, rr.event_id DESC",
    }
}

#[cfg(test)]
mod tests {
    use super::request_records_order_by_clause;

    #[test]
    fn defaults_to_recent_first_sort() {
        assert_eq!(
            request_records_order_by_clause("unknown", -1),
            "rr.created_at DESC, rr.event_id DESC"
        );
    }

    #[test]
    fn sorts_usage_date_ascending_with_stable_tie_breakers() {
        assert_eq!(
            request_records_order_by_clause("usage_date", 1),
            "date_trunc('day', rr.created_at) ASC, rr.created_at ASC, rr.event_id ASC"
        );
    }
}
