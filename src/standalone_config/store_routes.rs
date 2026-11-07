use sqlx::Row;

use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::rows;
use crate::standalone_config::write;
use crate::standalone_config::{
    ModelRouteConfig, ModelRouteTargetConfig, Result, StandaloneConfig, StandaloneConfigError,
};

use super::StandaloneConfigStore;

impl StandaloneConfigStore {
    pub async fn list_routes_page(
        &self,
        manager: &RelaySecretManager,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<ModelRouteConfig>)> {
        let total = standalone_query!("src/sql/standalone/count_model_routes.sql")
            .fetch_one(&self.pool)
            .await?
            .try_get::<i64, _>("total")?;
        let route_rows = standalone_query!("src/sql/standalone/list_routes_page.sql")
            .bind(rows.clamp(1, 200))
            .bind(first.max(0))
            .fetch_all(&self.pool)
            .await?;
        let mut routes = Vec::with_capacity(route_rows.len());
        for row in route_rows {
            routes.push(rows::route(&row)?);
        }
        for row in standalone_query!("src/sql/standalone/list_route_targets.sql")
            .fetch_all(&self.pool)
            .await?
        {
            let (rule_id, mut target, proxy_envelope) = rows::route_target(&row)?;
            target.proxy_url_override = write::decrypt_optional(manager, proxy_envelope.as_ref())?
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            routes
                .iter_mut()
                .find(|route| route.rule_id == rule_id)
                .ok_or_else(|| {
                    StandaloneConfigError::CorruptDatabase(
                        "route target references a missing route".to_string(),
                    )
                })?
                .targets
                .push(target);
        }
        Ok((total, routes))
    }

    pub async fn get_route(
        &self,
        manager: &RelaySecretManager,
        rule_id: uuid::Uuid,
    ) -> Result<Option<ModelRouteConfig>> {
        let row = standalone_query!("src/sql/standalone/get_model_route.sql")
            .bind(rule_id.to_string())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        let mut route = rows::route(&row)?;
        for row in standalone_query!("src/sql/standalone/list_route_targets_for.sql")
            .bind(rule_id.to_string())
            .fetch_all(&self.pool)
            .await?
        {
            let (_, mut target, proxy_envelope) = rows::route_target(&row)?;
            target.proxy_url_override = write::decrypt_optional(manager, proxy_envelope.as_ref())?
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            route.targets.push(target);
        }
        Ok(Some(route))
    }

    pub async fn list_route_targets_for(
        &self,
        manager: &RelaySecretManager,
        rule_id: uuid::Uuid,
    ) -> Result<Vec<ModelRouteTargetConfig>> {
        let rows = standalone_query!("src/sql/standalone/list_route_targets_for.sql")
            .bind(rule_id.to_string())
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                let (_, mut target, proxy_envelope) = rows::route_target(&row)?;
                target.proxy_url_override =
                    write::decrypt_optional(manager, proxy_envelope.as_ref())?
                        .map(|v| v.trim().to_string())
                        .filter(|v| !v.is_empty());
                Ok(target)
            })
            .collect()
    }

    pub async fn save_route(
        &self,
        manager: &RelaySecretManager,
        route: &ModelRouteConfig,
    ) -> Result<()> {
        let snapshot = StandaloneConfig {
            routes: vec![route.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/delete_route_targets.sql")
            .bind(route.rule_id.to_string())
            .execute(&mut *transaction)
            .await?;
        write::insert_route(&mut transaction, manager, route).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Insert or replace a single model route and its targets without running
    /// the full snapshot validator. Used by the unified configuration
    /// repository when an endpoint has just been created in the same
    /// transaction context.
    pub async fn save_route_direct(
        &self,
        manager: &RelaySecretManager,
        route: &ModelRouteConfig,
    ) -> Result<()> {
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/delete_route_targets.sql")
            .bind(route.rule_id.to_string())
            .execute(&mut *transaction)
            .await?;
        write::insert_route(&mut transaction, manager, route).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn delete_route(&self, rule_id: uuid::Uuid) -> Result<bool> {
        let mut transaction = self.pool.begin().await?;
        standalone_query!("src/sql/standalone/delete_route_targets.sql")
            .bind(rule_id.to_string())
            .execute(&mut *transaction)
            .await?;
        let result = standalone_query!("src/sql/standalone/delete_model_route.sql")
            .bind(rule_id.to_string())
            .execute(&mut *transaction)
            .await?;
        let removed = result.rows_affected() > 0;
        transaction.commit().await?;
        Ok(removed)
    }
}
