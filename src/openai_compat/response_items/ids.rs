pub(crate) fn generate_call_id() -> String {
    format!("call_{}", uuid::Uuid::new_v4().simple())
}

pub(crate) fn generate_message_id() -> String {
    format!("msg_{}", uuid::Uuid::new_v4().simple())
}

pub(crate) fn generate_reasoning_id() -> String {
    format!("rs_{}", uuid::Uuid::new_v4().simple())
}

pub(crate) fn generate_response_id() -> String {
    format!("resp_{}", uuid::Uuid::new_v4().simple())
}

pub(super) fn current_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}
