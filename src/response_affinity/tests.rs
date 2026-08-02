use super::*;

fn binding(endpoint_id: Uuid) -> ResponseAffinityBinding {
    ResponseAffinityBinding {
        endpoint_id,
        endpoint_key_id: None,
        endpoint_key_fingerprint: "fingerprint".to_string(),
    }
}

#[tokio::test]
async fn local_store_keeps_first_binding_for_concurrent_creators() {
    let store = ResponseAffinityStore::for_tests();
    let key = "affinity-key";
    let first = binding(Uuid::new_v4());
    let second = binding(Uuid::new_v4());
    let (left, right) = tokio::join!(
        store.get_or_create(key, &first),
        store.get_or_create(key, &second)
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_eq!(left, right);
    assert!(left == first || left == second);
}

#[test]
fn cache_key_is_hashed_and_scope_sensitive() {
    let rule_id = Uuid::new_v4();
    let first = ResponseAffinityStore::cache_key(1, rule_id, "session-a");
    let second = ResponseAffinityStore::cache_key(1, rule_id, "session-b");
    assert!(first.starts_with(RESPONSE_AFFINITY_VALKEY_KEY_PREFIX));
    assert_ne!(first, second);
    assert!(!first.contains("session-a"));
}
