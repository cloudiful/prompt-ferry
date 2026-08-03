use super::*;

pub async fn create_model_endpoint_rule(
    pool: &PgPool,
    input: ModelEndpointRuleCreate,
) -> Result<ModelEndpointRule> {
    let mut tx = pool.begin().await?;
    ensure_model_route_targets(&input)?;
    let legacy_endpoint_id = input.targets[0].endpoint_id;
    let row = sqlx::query_file_as!(
        ModelEndpointRuleRow,
        "src/sql/routes/create_model_endpoint_rule.sql",
        &input.scope,
        input.owner_user_id,
        &input.model_pattern,
        input.routing_strategy.as_str(),
        input.session_affinity_lock_after_turns,
        input.daily_max_requests,
        input.monthly_max_requests,
        legacy_endpoint_id,
        input.enabled,
    )
    .fetch_one(&mut *tx)
    .await?;
    sync_model_route_targets(&mut tx, row.rule_id, &input).await?;
    tx.commit().await?;
    queries::get_model_endpoint_rule(pool, row.rule_id)
        .await?
        .ok_or_else(|| anyhow!("model route not found after insert"))
}

pub async fn update_model_endpoint_rule(
    pool: &PgPool,
    rule_id: uuid::Uuid,
    input: ModelEndpointRuleCreate,
) -> Result<Option<ModelEndpointRule>> {
    let mut tx = pool.begin().await?;
    ensure_model_route_targets(&input)?;
    let legacy_endpoint_id = input.targets[0].endpoint_id;
    let row = sqlx::query_file_as!(
        ModelEndpointRuleRow,
        "src/sql/routes/update_model_endpoint_rule.sql",
        rule_id,
        &input.scope,
        input.owner_user_id,
        &input.model_pattern,
        input.routing_strategy.as_str(),
        input.session_affinity_lock_after_turns,
        input.daily_max_requests,
        input.monthly_max_requests,
        legacy_endpoint_id,
        input.enabled,
    )
    .fetch_optional(&mut *tx)
    .await?;
    let Some(row) = row else {
        tx.rollback().await?;
        return Ok(None);
    };
    sync_model_route_targets(&mut tx, row.rule_id, &input).await?;
    tx.commit().await?;
    queries::get_model_endpoint_rule(pool, row.rule_id).await
}

pub async fn delete_model_endpoint_rule(pool: &PgPool, rule_id: uuid::Uuid) -> Result<bool> {
    let result = sqlx::query_file!("src/sql/routes/delete_model_endpoint_rule.sql", rule_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

async fn sync_model_route_targets(
    tx: &mut Transaction<'_, Postgres>,
    rule_id: uuid::Uuid,
    input: &ModelEndpointRuleCreate,
) -> Result<()> {
    sqlx::query_file!("src/sql/routes/delete_model_route_targets.sql", rule_id)
        .execute(&mut **tx)
        .await?;

    for (position, target) in input.targets.iter().enumerate() {
        sqlx::query_file!(
            "src/sql/routes/insert_model_route_target.sql",
            rule_id,
            target.endpoint_id,
            position as i32,
            target.enabled,
            target.upstream_model,
            target.responses_continuation_policy.as_str(),
            target.chat_reasoning_replay_policy.as_str(),
        )
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

fn ensure_model_route_targets(input: &ModelEndpointRuleCreate) -> Result<()> {
    if input.targets.is_empty() {
        return Err(anyhow!("model route requires at least one target"));
    }
    Ok(())
}
