use super::*;

pub fn model_pattern_matches(pattern: &str, model: &str) -> bool {
    model_pattern_specificity(pattern, model).is_some()
}

pub async fn get_route(pool: &PgPool, route_id: uuid::Uuid) -> Result<Option<RouteConfig>> {
    let route = sqlx::query_file!("src/sql/routes/get_route.sql", route_id,)
        .fetch_optional(pool)
        .await?
        .map(|row| RouteConfig {
            route_id: row.route_id,
            user_id: row.user_id,
            model_route_rule_id: None,
            base_url: row.base_url,
            api_key: row.api_key,
            endpoint_key_id: None,
            endpoint_key_label: None,
            api_keys: Vec::new(),
            key_lb_enabled: row.key_lb_enabled,
            native_api: parse_native_api(&row.native_api),
            upstream_model: None,
            responses_continuation_policy: crate::db::ResponsesContinuationPolicy::ForceReplay,
            route_selection_reason: crate::db::RouteSelectionReason::Default,
        });
    match route {
        Some(route) => Ok(
            crate::db::endpoints::attach_route_config_api_keys(pool, vec![route])
                .await?
                .into_iter()
                .next(),
        ),
        None => Ok(None),
    }
}

pub async fn cleanup_orphan_model_routes(pool: &PgPool) -> Result<u64> {
    let result = sqlx::query_file!("src/sql/routes/cleanup_orphan_model_routes.sql")
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

pub(super) fn model_pattern_specificity(pattern: &str, model: &str) -> Option<(u8, usize)> {
    let pattern = pattern.trim();
    if pattern == "*" {
        return Some((0, 0));
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return model.starts_with(prefix).then_some((1, prefix.len()));
    }
    (pattern == model).then_some((2, pattern.len()))
}

pub(super) fn route_precedence_key(
    candidate: &ModelRouteCandidate,
    specificity: (u8, usize),
) -> (u8, u8, usize, DateTime<Utc>) {
    (
        if candidate.scope == "user" { 1 } else { 0 },
        specificity.0,
        specificity.1,
        candidate.updated_at,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn model_pattern_supports_exact_prefix_and_wildcard() {
        assert_eq!(
            model_pattern_specificity("gpt-5.5", "gpt-5.5"),
            Some((2, 7))
        );
        assert_eq!(
            model_pattern_specificity("gpt-5.4-*", "gpt-5.4-mini"),
            Some((1, 8))
        );
        assert_eq!(model_pattern_specificity("*", "anything"), Some((0, 0)));
        assert_eq!(model_pattern_specificity("gpt-5.4-*", "gpt-5.5"), None);
    }

    #[test]
    fn route_precedence_prefers_user_scope_before_admin() {
        let user_candidate = ModelRouteCandidate {
            rule_id: uuid::Uuid::new_v4(),
            scope: "user".to_string(),
            owner_user_id: Some(1),
            model_pattern: "gpt-*".to_string(),
            routing_strategy: crate::db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            session_affinity_lock_after_turns: 5,
            daily_max_requests: None,
            monthly_max_requests: None,
            updated_at: Utc::now(),
            targets: Vec::new(),
        };
        let admin_candidate = ModelRouteCandidate {
            scope: "admin".to_string(),
            owner_user_id: None,
            ..user_candidate.clone()
        };

        let user_key = route_precedence_key(&user_candidate, (1, 4));
        let admin_key = route_precedence_key(&admin_candidate, (2, 5));
        assert!(user_key > admin_key);
    }

    #[test]
    fn route_precedence_prefers_exact_then_longer_prefix() {
        let candidate = ModelRouteCandidate {
            rule_id: uuid::Uuid::new_v4(),
            scope: "admin".to_string(),
            owner_user_id: None,
            model_pattern: String::new(),
            routing_strategy: crate::db::ModelRouteRoutingStrategy::ClientKeyRendezvous,
            session_affinity_lock_after_turns: 5,
            daily_max_requests: None,
            monthly_max_requests: None,
            updated_at: Utc::now(),
            targets: Vec::new(),
        };

        assert!(
            route_precedence_key(&candidate, (2, 7)) > route_precedence_key(&candidate, (1, 10))
        );
        assert!(
            route_precedence_key(&candidate, (1, 10)) > route_precedence_key(&candidate, (1, 5))
        );
        assert!(
            route_precedence_key(&candidate, (1, 5)) > route_precedence_key(&candidate, (0, 0))
        );
    }
}
