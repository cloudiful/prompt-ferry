use super::*;
use redis::AsyncCommands;

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

#[tokio::test]
async fn redis_store_get_or_create_refreshes_ttl_and_preserves_cas() {
    let Ok(url) = std::env::var("PROMPT_FERRY_TEST_VALKEY_URL") else {
        eprintln!(
            "skipping Valkey response affinity test: PROMPT_FERRY_TEST_VALKEY_URL is not set"
        );
        return;
    };

    let client = redis::Client::open(url).unwrap();
    let manager = client.get_connection_manager().await.unwrap();
    let store = ResponseAffinityStore::from_connection_manager(manager.clone(), 30);
    let key = format!(
        "{RESPONSE_AFFINITY_VALKEY_KEY_PREFIX}test:{}",
        Uuid::new_v4()
    );
    let first = binding(Uuid::new_v4());
    let second = binding(Uuid::new_v4());
    let mut connection = manager.clone();

    let _: usize = connection.del(&key).await.unwrap();
    let (left, right) = tokio::join!(
        store.get_or_create(&key, &first),
        store.get_or_create(&key, &second)
    );
    let bound = left.unwrap();
    assert_eq!(right.unwrap(), bound.clone());
    assert_eq!(store.get_or_create(&key, &first).await.unwrap(), bound);

    let _: bool = connection.expire(&key, 1).await.unwrap();
    assert_eq!(store.get_or_create(&key, &second).await.unwrap(), bound);
    let ttl: i64 = connection.ttl(&key).await.unwrap();
    assert!(ttl > 1, "get_or_create should refresh the binding TTL");

    assert!(
        store
            .replace_if_current(&key, &bound, &second)
            .await
            .unwrap()
    );
    assert_eq!(store.get(&key).await.unwrap(), Some(second.clone()));
    assert!(
        !store
            .replace_if_current(&key, &bound, &binding(Uuid::new_v4()))
            .await
            .unwrap()
    );

    let _: usize = connection.del(&key).await.unwrap();
}
