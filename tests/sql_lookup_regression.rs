use chrono::{Duration, Utc};
use prompt_ferry::db::{BillingChargeRow, ConversationEndpointOverride};
use uuid::Uuid;

#[path = "support/sql_lookup_regression.rs"]
mod fixtures;
use fixtures::{insert_charge, insert_endpoint, insert_request, isolated_pool};

#[tokio::test]
async fn scalar_lookups_preserve_optional_metadata_and_override_upserts() {
    let pool = isolated_pool().await;
    let mut transaction = pool
        .begin()
        .await
        .expect("begin isolated fixture transaction");
    let login_name = format!("sql-lookup-{}", Uuid::new_v4());
    let user_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO users (login_name, password_hash, display_name) \
         VALUES ($1, 'fixture-hash', 'SQL lookup fixture') RETURNING user_id",
    )
    .bind(&login_name)
    .fetch_one(&mut *transaction)
    .await
    .expect("insert user fixture");
    let endpoint_id = Uuid::new_v4();
    let endpoint_name = format!("endpoint-{}", Uuid::new_v4());
    insert_endpoint(&mut transaction, endpoint_id, &endpoint_name).await;
    let endpoint_key_id = Uuid::new_v4();
    let endpoint_key_label = "fixture key";
    sqlx::query(
        "INSERT INTO endpoint_api_keys \
         (key_id, endpoint_id, key_label, api_key, position, enabled) \
         VALUES ($1, $2, $3, 'sql-lookup-fixture-key', 0, TRUE)",
    )
    .bind(endpoint_key_id)
    .bind(endpoint_id)
    .bind(endpoint_key_label)
    .execute(&mut *transaction)
    .await
    .expect("insert endpoint key fixture");

    let present_request_id = Uuid::new_v4();
    let present_event_id = insert_request(
        &mut transaction,
        present_request_id,
        Some(user_id),
        Some(endpoint_id),
    )
    .await;
    let present_charge_id = insert_charge(
        &mut transaction,
        present_event_id,
        present_request_id,
        Some(user_id),
        Some(endpoint_id),
    )
    .await;
    let present_rows = sqlx::query_file_as!(
        BillingChargeRow,
        "src/sql/billing/get_charge.sql",
        present_charge_id,
    )
    .fetch_all(&mut *transaction)
    .await
    .expect("query present charge");
    assert_eq!(present_rows.len(), 1);
    let present = present_rows.into_iter().next().expect("present charge row");
    assert_eq!(
        present.user_login_name.as_deref(),
        Some(login_name.as_str())
    );
    assert_eq!(
        present.endpoint_name.as_deref(),
        Some(endpoint_name.as_str())
    );

    let missing_request_id = Uuid::new_v4();
    let missing_event_id = insert_request(&mut transaction, missing_request_id, None, None).await;
    let missing_charge_id = insert_charge(
        &mut transaction,
        missing_event_id,
        missing_request_id,
        None,
        None,
    )
    .await;
    let missing_rows = sqlx::query_file_as!(
        BillingChargeRow,
        "src/sql/billing/get_charge.sql",
        missing_charge_id,
    )
    .fetch_all(&mut *transaction)
    .await
    .expect("query charge without optional associations");
    assert_eq!(missing_rows.len(), 1);
    let missing = missing_rows
        .into_iter()
        .next()
        .expect("base charge row remains present");
    assert!(missing.user_login_name.is_none());
    assert!(missing.endpoint_name.is_none());
    assert!(
        sqlx::query_file_as!(BillingChargeRow, "src/sql/billing/get_charge.sql", i64::MAX,)
            .fetch_all(&mut *transaction)
            .await
            .expect("query absent charge")
            .is_empty()
    );

    let inserted_conversation_id = Uuid::new_v4();
    let inserted_rows = sqlx::query_file_as!(
        ConversationEndpointOverride,
        "src/sql/routes/upsert_conversation_endpoint_override.sql",
        inserted_conversation_id,
        endpoint_id,
        Some(endpoint_key_id),
        user_id,
    )
    .fetch_all(&mut *transaction)
    .await
    .expect("insert endpoint override");
    assert_eq!(inserted_rows.len(), 1);
    let inserted = inserted_rows
        .into_iter()
        .next()
        .expect("inserted override row");
    assert_eq!(inserted.conversation_id, inserted_conversation_id);
    assert_eq!(inserted.endpoint_id, endpoint_id);
    assert_eq!(inserted.endpoint_key_id, Some(endpoint_key_id));
    assert_eq!(
        inserted.endpoint_key_label.as_deref(),
        Some(endpoint_key_label)
    );
    assert_eq!(
        inserted.endpoint_name.as_deref(),
        Some(endpoint_name.as_str())
    );
    assert_eq!(inserted.created_by_user_id, Some(user_id));
    assert_eq!(inserted.created_at, inserted.updated_at);
    let read_inserted = sqlx::query_file_as!(
        ConversationEndpointOverride,
        "src/sql/routes/get_conversation_endpoint_override.sql",
        inserted_conversation_id,
    )
    .fetch_optional(&mut *transaction)
    .await
    .expect("read inserted endpoint override")
    .expect("inserted override can be read");
    assert_eq!(read_inserted.endpoint_id, inserted.endpoint_id);
    assert_eq!(read_inserted.endpoint_key_id, inserted.endpoint_key_id);
    assert_eq!(
        read_inserted.endpoint_key_label,
        inserted.endpoint_key_label
    );
    assert_eq!(read_inserted.endpoint_name, inserted.endpoint_name);
    assert_eq!(read_inserted.created_at, inserted.created_at);
    assert_eq!(read_inserted.updated_at, inserted.updated_at);
    let inserted_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM conversation_endpoint_overrides WHERE conversation_id = $1",
    )
    .bind(inserted_conversation_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("count inserted override");
    assert_eq!(inserted_count, 1);

    let updated_conversation_id = Uuid::new_v4();
    let old_created_at = Utc::now() - Duration::days(1);
    let stored_created_at = sqlx::query_scalar::<_, chrono::DateTime<Utc>>(
        "INSERT INTO conversation_endpoint_overrides \
         (conversation_id, endpoint_id, endpoint_key_id, created_by_user_id, created_at, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $5) RETURNING created_at",
    )
    .bind(updated_conversation_id)
    .bind(endpoint_id)
    .bind(endpoint_key_id)
    .bind(user_id)
    .bind(old_created_at)
    .fetch_one(&mut *transaction)
    .await
    .expect("insert existing override fixture");
    let updated_rows = sqlx::query_file_as!(
        ConversationEndpointOverride,
        "src/sql/routes/upsert_conversation_endpoint_override.sql",
        updated_conversation_id,
        endpoint_id,
        None::<Uuid>,
        user_id,
    )
    .fetch_all(&mut *transaction)
    .await
    .expect("update endpoint override");
    assert_eq!(updated_rows.len(), 1);
    let updated = updated_rows
        .into_iter()
        .next()
        .expect("updated override row");
    assert_eq!(updated.created_at, stored_created_at);
    assert!(updated.updated_at > stored_created_at);
    assert!(updated.endpoint_key_id.is_none());
    assert!(updated.endpoint_key_label.is_none());
    assert_eq!(
        updated.endpoint_name.as_deref(),
        Some(endpoint_name.as_str())
    );
    let read_updated = sqlx::query_file_as!(
        ConversationEndpointOverride,
        "src/sql/routes/get_conversation_endpoint_override.sql",
        updated_conversation_id,
    )
    .fetch_optional(&mut *transaction)
    .await
    .expect("read updated endpoint override")
    .expect("updated override can be read");
    assert_eq!(read_updated.created_at, updated.created_at);
    assert_eq!(read_updated.updated_at, updated.updated_at);
    assert!(read_updated.endpoint_key_id.is_none());
    assert!(read_updated.endpoint_key_label.is_none());
    assert_eq!(
        read_updated.endpoint_name.as_deref(),
        Some(endpoint_name.as_str())
    );
    let updated_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM conversation_endpoint_overrides WHERE conversation_id = $1",
    )
    .bind(updated_conversation_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("count updated override");
    assert_eq!(updated_count, 1);
    assert!(
        sqlx::query_file_as!(
            ConversationEndpointOverride,
            "src/sql/routes/get_conversation_endpoint_override.sql",
            Uuid::new_v4(),
        )
        .fetch_optional(&mut *transaction)
        .await
        .expect("read missing conversation override")
        .is_none()
    );

    transaction
        .rollback()
        .await
        .expect("rollback test fixtures");
    pool.close().await;
}
