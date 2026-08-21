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

#[tokio::test]
async fn local_store_delete_reports_whether_key_existed_and_removes_it() {
    let store = ResponseAffinityStore::for_tests();
    let key = "affinity-delete-key";
    assert_eq!(store.delete(key).await.unwrap(), false);
    assert_eq!(store.get(key).await.unwrap(), None);

    store
        .get_or_create(key, &binding(Uuid::new_v4()))
        .await
        .unwrap();
    assert_eq!(store.delete(key).await.unwrap(), true);
    assert_eq!(store.delete(key).await.unwrap(), false);
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

#[tokio::test]
async fn redis_store_delete_reports_whether_key_existed_and_removes_it() {
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
    let mut connection = manager.clone();
    let _: usize = connection.del(&key).await.unwrap();

    assert_eq!(store.delete(&key).await.unwrap(), false);
    assert_eq!(store.get(&key).await.unwrap(), None);

    let first = binding(Uuid::new_v4());
    store.get_or_create(&key, &first).await.unwrap();
    assert_eq!(store.delete(&key).await.unwrap(), true);
    assert_eq!(store.delete(&key).await.unwrap(), false);
    assert_eq!(store.get(&key).await.unwrap(), None);
    assert_eq!(store.peek(&key).await.unwrap(), None);

    let _: usize = connection.del(&key).await.unwrap();
}

#[tokio::test]
async fn redis_store_peek_does_not_refresh_ttl() {
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
    let mut connection = manager.clone();
    let _: usize = connection.del(&key).await.unwrap();

    store
        .get_or_create(&key, &binding(Uuid::new_v4()))
        .await
        .unwrap();
    let _: bool = connection.expire(&key, 30).await.unwrap();
    assert!(store.peek(&key).await.unwrap().is_some());
    let ttl: i64 = connection.ttl(&key).await.unwrap();
    assert!(
        (25..=30).contains(&ttl),
        "peek must not refresh the binding TTL (got {ttl})"
    );

    let _: usize = connection.del(&key).await.unwrap();
}
