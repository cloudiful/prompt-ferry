use super::*;

fn binding(endpoint_id: Uuid) -> ResponseAffinityBinding {
    ResponseAffinityBinding {
        endpoint_id,
        endpoint_key_id: None,
        endpoint_key_fingerprint: "fingerprint".to_string(),
        target: None,
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

#[tokio::test]
async fn local_store_delete_reports_whether_key_existed_and_removes_it() {
    let store = ResponseAffinityStore::for_tests();
    let key = "affinity-delete-key";
    assert!(!store.delete(key).await.unwrap());
    assert_eq!(store.get(key).await.unwrap(), None);

    store
        .get_or_create(key, &binding(Uuid::new_v4()))
        .await
        .unwrap();
    assert!(store.delete(key).await.unwrap());
    assert!(!store.delete(key).await.unwrap());
    assert_eq!(store.get(key).await.unwrap(), None);
    assert_eq!(store.peek(key).await.unwrap(), None);
}

#[tokio::test]
async fn local_store_supports_refreshing_get_and_compare_and_replace() {
    let store = ResponseAffinityStore::local_with_ttl_and_capacity(Duration::from_millis(250), 4);
    let key = "affinity-crud-key";
    let first = binding(Uuid::new_v4());
    let replacement = binding(Uuid::new_v4());

    store.get_or_create(key, &first).await.unwrap();
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(store.get(key).await.unwrap(), Some(first.clone()));
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert!(
        !store
            .replace_if_current(key, &binding(Uuid::new_v4()), &replacement)
            .await
            .unwrap()
    );
    assert!(
        store
            .replace_if_current(key, &first, &replacement)
            .await
            .unwrap()
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(store.peek(key).await.unwrap(), Some(replacement.clone()));
    tokio::time::sleep(Duration::from_millis(180)).await;
    assert_eq!(store.get(key).await.unwrap(), None);
}

#[tokio::test]
async fn local_store_evicts_least_recently_used_binding_at_capacity() {
    let store = ResponseAffinityStore::local_with_ttl_and_capacity(Duration::from_secs(30), 2);
    let first_key = "affinity-capacity-first";
    let second_key = "affinity-capacity-second";
    let third_key = "affinity-capacity-third";

    store
        .get_or_create(first_key, &binding(Uuid::new_v4()))
        .await
        .unwrap();
    store
        .get_or_create(second_key, &binding(Uuid::new_v4()))
        .await
        .unwrap();
    store.get(first_key).await.unwrap();
    store
        .get_or_create(third_key, &binding(Uuid::new_v4()))
        .await
        .unwrap();

    assert!(store.peek(first_key).await.unwrap().is_some());
    assert_eq!(store.peek(second_key).await.unwrap(), None);
    assert!(store.peek(third_key).await.unwrap().is_some());
}

#[tokio::test]
async fn local_store_peek_does_not_refresh_ttl() {
    let store = ResponseAffinityStore::for_tests_with_ttl(Duration::from_secs(2));
    let key = "affinity-peek-key";
    store
        .get_or_create(key, &binding(Uuid::new_v4()))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(
        store.peek(key).await.unwrap().is_some(),
        "binding should still be alive shortly after creation"
    );
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(2300) {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        store.peek(key).await.unwrap(),
        None,
        "peek must not extend the binding TTL"
    );
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

/// A legacy payload carries no target identity and must round-trip
/// byte-identically, so an affinity entry written before the identity field
/// exists keeps its exact bytes and its endpoint-only meaning.
#[test]
fn a_legacy_payload_round_trips_byte_identically() {
    let endpoint_id = Uuid::new_v4();
    let payload = format!(
        r#"{{"endpoint_id":"{endpoint_id}","endpoint_key_id":null,"endpoint_key_fingerprint":"fingerprint"}}"#
    );
    let binding: ResponseAffinityBinding = serde_json::from_str(&payload).unwrap();
    assert_eq!(binding.target, None);
    assert_eq!(serde_json::to_string(&binding).unwrap(), payload);
}

/// A new payload records the selected target: an inherited model serializes
/// an empty identity, an explicit model serializes the normalized model.
#[test]
fn a_target_identity_is_serialized_with_the_binding() {
    let endpoint_id = Uuid::new_v4();
    let inherited = ResponseAffinityBinding {
        endpoint_id,
        endpoint_key_id: None,
        endpoint_key_fingerprint: "fingerprint".to_string(),
        target: Some(ResponseAffinityTargetIdentity {
            upstream_model: None,
        }),
    };
    assert_eq!(
        serde_json::to_string(&inherited).unwrap(),
        format!(
            r#"{{"endpoint_id":"{endpoint_id}","endpoint_key_id":null,"endpoint_key_fingerprint":"fingerprint","target":{{}}}}"#
        )
    );

    let explicit = ResponseAffinityBinding {
        target: Some(ResponseAffinityTargetIdentity {
            upstream_model: Some("upstream-model".to_string()),
        }),
        ..inherited.clone()
    };
    assert_eq!(
        serde_json::to_string(&explicit).unwrap(),
        format!(
            r#"{{"endpoint_id":"{endpoint_id}","endpoint_key_id":null,"endpoint_key_fingerprint":"fingerprint","target":{{"upstream_model":"upstream-model"}}}}"#
        )
    );

    // An inherited model is a different binding than a legacy one.
    assert_ne!(inherited, binding(endpoint_id));
    assert_ne!(inherited.target, explicit.target);
}
