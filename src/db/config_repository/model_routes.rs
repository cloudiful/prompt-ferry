//! Model route CRUD for the unified configuration repository.

use anyhow::{Context, Result};
use serde::Serialize;
use uuid::Uuid;

use super::model_routes_map;
use super::{PostgresConfigRepository, SqliteConfigRepository};
use crate::db::{
    ModelEndpointRuleCreate, ModelRoutePage as PgModelRoutePage,
    ModelRouteTarget as PgModelRouteTarget,
};

#[derive(Debug, Clone, Serialize)]
pub struct UnifiedModelRoute {
    pub rule_id: Uuid,
    pub scope: String,
    pub owner_user_id: Option<i64>,
    pub model_pattern: String,
    pub routing_strategy: crate::db::ModelRouteRoutingStrategy,
    pub daily_max_requests: Option<i32>,
    pub monthly_max_requests: Option<i32>,
    pub enabled: bool,
    pub targets: Vec<UnifiedModelRouteTarget>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnifiedModelRouteTarget {
    pub target_id: Uuid,
    pub rule_id: Uuid,
    pub endpoint_id: Uuid,
    pub endpoint_name: Option<String>,
    pub endpoint_enabled: bool,
    pub position: i32,
    pub enabled: bool,
    pub upstream_model: Option<String>,
    pub responses_continuation_policy: crate::db::ResponsesContinuationPolicy,
}

#[derive(Debug, Clone, Serialize)]
pub struct UnifiedModelRoutePage {
    pub total: i64,
    pub routes: Vec<UnifiedModelRoute>,
    pub first: i64,
    pub rows: i64,
}

impl From<UnifiedModelRoutePage> for crate::worker_admin_types::ModelRoutePageResponse {
    fn from(value: UnifiedModelRoutePage) -> Self {
        Self {
            total: value.total,
            routes: value.routes.into_iter().map(rule_to_pg).collect(),
            first: value.first,
            rows: value.rows,
        }
    }
}

fn rule_to_pg(route: UnifiedModelRoute) -> crate::db::ModelEndpointRule {
    let now = chrono::Utc::now();
    crate::db::ModelEndpointRule {
        rule_id: route.rule_id,
        scope: route.scope,
        owner_user_id: route.owner_user_id,
        model_pattern: route.model_pattern,
        routing_strategy: route.routing_strategy,
        daily_max_requests: route.daily_max_requests,
        monthly_max_requests: route.monthly_max_requests,
        enabled: route.enabled,
        created_at: now,
        updated_at: now,
        targets: route.targets.into_iter().map(target_to_pg).collect(),
    }
}

fn target_to_pg(target: UnifiedModelRouteTarget) -> PgModelRouteTarget {
    PgModelRouteTarget {
        target_id: target.target_id,
        rule_id: target.rule_id,
        endpoint_id: target.endpoint_id,
        endpoint_name: target.endpoint_name,
        endpoint_enabled: target.endpoint_enabled,
        position: target.position,
        enabled: target.enabled,
        upstream_model: target.upstream_model,
        responses_continuation_policy: target.responses_continuation_policy,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

impl super::ConfigRepository {
    pub async fn list_model_routes_page(
        &self,
        first: i64,
        rows: i64,
    ) -> Result<UnifiedModelRoutePage> {
        match self {
            Self::Postgres(repo) => repo.list_model_routes_page(first, rows).await,
            Self::Sqlite(repo) => repo.list_model_routes_page(first, rows).await,
        }
    }

    pub async fn get_model_route(&self, rule_id: Uuid) -> Result<Option<UnifiedModelRoute>> {
        match self {
            Self::Postgres(repo) => repo.get_model_route(rule_id).await,
            Self::Sqlite(repo) => repo.get_model_route(rule_id).await,
        }
    }

    pub async fn create_model_route(
        &self,
        rule_id: Uuid,
        input: ModelEndpointRuleCreate,
    ) -> Result<UnifiedModelRoute> {
        match self {
            Self::Postgres(repo) => repo.create_model_route(input).await,
            Self::Sqlite(repo) => repo.create_model_route(rule_id, input).await,
        }
    }

    pub async fn update_model_route(
        &self,
        rule_id: Uuid,
        input: ModelEndpointRuleCreate,
    ) -> Result<Option<UnifiedModelRoute>> {
        match self {
            Self::Postgres(repo) => repo.update_model_route(rule_id, input).await,
            Self::Sqlite(repo) => repo.update_model_route(rule_id, input).await,
        }
    }

    pub async fn delete_model_route(&self, rule_id: Uuid) -> Result<bool> {
        match self {
            Self::Postgres(repo) => {
                crate::db::delete_model_endpoint_rule(repo.pool(), rule_id).await
            }
            Self::Sqlite(repo) => repo.delete_model_route(rule_id).await,
        }
    }
}

impl PostgresConfigRepository {
    async fn list_model_routes_page(&self, first: i64, rows: i64) -> Result<UnifiedModelRoutePage> {
        let page: PgModelRoutePage =
            crate::db::list_model_endpoint_rules_page(&self.pool, first, rows).await?;
        Ok(UnifiedModelRoutePage {
            total: page.total,
            routes: page
                .routes
                .into_iter()
                .map(model_routes_map::from_postgres)
                .collect(),
            first: page.first,
            rows: page.rows,
        })
    }

    async fn get_model_route(&self, rule_id: Uuid) -> Result<Option<UnifiedModelRoute>> {
        Ok(crate::db::get_model_endpoint_rule(&self.pool, rule_id)
            .await?
            .map(model_routes_map::from_postgres))
    }

    async fn create_model_route(
        &self,
        input: ModelEndpointRuleCreate,
    ) -> Result<UnifiedModelRoute> {
        let rule = crate::db::create_model_endpoint_rule(&self.pool, input)
            .await
            .context("failed to create model route")?;
        Ok(model_routes_map::from_postgres(rule))
    }

    async fn update_model_route(
        &self,
        rule_id: Uuid,
        input: ModelEndpointRuleCreate,
    ) -> Result<Option<UnifiedModelRoute>> {
        Ok(
            crate::db::update_model_endpoint_rule(&self.pool, rule_id, input)
                .await?
                .map(model_routes_map::from_postgres),
        )
    }
}

impl SqliteConfigRepository {
    async fn list_model_routes_page(&self, first: i64, rows: i64) -> Result<UnifiedModelRoutePage> {
        let (total, routes) = self
            .store
            .list_routes_page(first, rows)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let snapshot = self
            .store
            .load_snapshot(&self.manager)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let endpoint_lookup = |endpoint_id: Uuid| -> (String, bool) {
            snapshot
                .endpoints
                .iter()
                .find(|endpoint| endpoint.endpoint_id == endpoint_id)
                .map(|endpoint| (endpoint.name.clone(), endpoint.enabled))
                .unwrap_or_default()
        };
        let unified = routes
            .into_iter()
            .map(|route| model_routes_map::from_sqlite(route, &endpoint_lookup))
            .collect();
        Ok(UnifiedModelRoutePage {
            total,
            routes: unified,
            first,
            rows,
        })
    }

    async fn get_model_route(&self, rule_id: Uuid) -> Result<Option<UnifiedModelRoute>> {
        let route = self
            .store
            .get_route(rule_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let Some(route) = route else { return Ok(None) };
        let snapshot = self
            .store
            .load_snapshot(&self.manager)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let endpoint_lookup = |endpoint_id: Uuid| -> (String, bool) {
            snapshot
                .endpoints
                .iter()
                .find(|endpoint| endpoint.endpoint_id == endpoint_id)
                .map(|endpoint| (endpoint.name.clone(), endpoint.enabled))
                .unwrap_or_default()
        };
        Ok(Some(model_routes_map::from_sqlite(route, &endpoint_lookup)))
    }

    async fn create_model_route(
        &self,
        rule_id: Uuid,
        input: ModelEndpointRuleCreate,
    ) -> Result<UnifiedModelRoute> {
        let config = model_routes_map::sqlite_route_from_create(rule_id, input)?;
        self.store
            .save_route_direct(&config)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let route = self
            .store
            .get_route(rule_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?
            .ok_or_else(|| anyhow::anyhow!("route not found after insert"))?;
        let snapshot = self
            .store
            .load_snapshot(&self.manager)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let endpoint_lookup = |endpoint_id: Uuid| -> (String, bool) {
            snapshot
                .endpoints
                .iter()
                .find(|endpoint| endpoint.endpoint_id == endpoint_id)
                .map(|endpoint| (endpoint.name.clone(), endpoint.enabled))
                .unwrap_or_default()
        };
        Ok(model_routes_map::from_sqlite(route, &endpoint_lookup))
    }

    async fn update_model_route(
        &self,
        rule_id: Uuid,
        input: ModelEndpointRuleCreate,
    ) -> Result<Option<UnifiedModelRoute>> {
        let existing = self
            .store
            .get_route(rule_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        if existing.is_none() {
            return Ok(None);
        }
        let config = model_routes_map::sqlite_route_from_create(rule_id, input)?;
        self.store
            .save_route_direct(&config)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let route = self
            .store
            .get_route(rule_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let snapshot = self
            .store
            .load_snapshot(&self.manager)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))?;
        let endpoint_lookup = |endpoint_id: Uuid| -> (String, bool) {
            snapshot
                .endpoints
                .iter()
                .find(|endpoint| endpoint.endpoint_id == endpoint_id)
                .map(|endpoint| (endpoint.name.clone(), endpoint.enabled))
                .unwrap_or_default()
        };
        Ok(route.map(|route| model_routes_map::from_sqlite(route, &endpoint_lookup)))
    }

    async fn delete_model_route(&self, rule_id: Uuid) -> Result<bool> {
        self.store
            .delete_route(rule_id)
            .await
            .map_err(|err| anyhow::anyhow!("{err}"))
    }
}
