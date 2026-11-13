use sea_orm::sea_query::{Alias, Expr, ExprTrait};
use sea_orm::{
    ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
    TransactionTrait,
};

use crate::db::entities::sqlite::standalone_model_route_targets::{
    Column as TargetColumn, Entity as TargetEntity,
};
use crate::db::entities::sqlite::standalone_model_routes::{
    Column as RouteColumn, Entity as RouteEntity,
};
use crate::db::seaorm::sqlite_connection;
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
        let db = sqlite_connection(&self.pool);
        let counts = sea_orm::sea_query::Query::select()
            .expr_as(Expr::col(RouteColumn::RuleId).count(), Alias::new("total"))
            .from(RouteEntity)
            .to_owned();
        let count_row = db
            .query_one(&counts)
            .await
            .map_err(rows::native_error)?
            .ok_or_else(|| {
                StandaloneConfigError::CorruptDatabase(
                    "standalone route counts returned no row".to_string(),
                )
            })?;
        let total: i64 = count_row.try_get("", "total").map_err(rows::native_error)?;
        let route_models = RouteEntity::find()
            .order_by_asc(RouteColumn::ModelPattern)
            .order_by_asc(RouteColumn::RuleId)
            .limit(rows.clamp(1, 200) as u64)
            .offset(std::cmp::Ord::max(first, 0) as u64)
            .all(&db)
            .await
            .map_err(rows::native_error)?;
        let mut routes = Vec::with_capacity(route_models.len());
        for model in &route_models {
            routes.push(rows::route_from_model(model)?);
        }
        let rule_ids = route_models
            .iter()
            .map(|model| model.rule_id.clone())
            .collect::<Vec<_>>();
        if !rule_ids.is_empty() {
            let target_models = TargetEntity::find()
                .filter(TargetColumn::RuleId.is_in(rule_ids))
                .order_by_asc(TargetColumn::RuleId)
                .order_by_asc(TargetColumn::Position)
                .all(&db)
                .await
                .map_err(rows::native_error)?;
            for model in &target_models {
                let (rule_id, mut target, proxy_envelope) = rows::route_target_from_model(model)?;
                target.proxy_url_override =
                    write::decrypt_optional(manager, proxy_envelope.as_ref())?
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty());
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
        }
        Ok((total, routes))
    }

    pub async fn get_route(
        &self,
        manager: &RelaySecretManager,
        rule_id: uuid::Uuid,
    ) -> Result<Option<ModelRouteConfig>> {
        let db = sqlite_connection(&self.pool);
        let model = RouteEntity::find_by_id(rule_id.to_string())
            .one(&db)
            .await
            .map_err(rows::native_error)?;
        let Some(model) = model else {
            return Ok(None);
        };
        let mut route = rows::route_from_model(&model)?;
        for model in TargetEntity::find()
            .filter(TargetColumn::RuleId.eq(rule_id.to_string()))
            .order_by_asc(TargetColumn::Position)
            .all(&db)
            .await
            .map_err(rows::native_error)?
        {
            let (_, mut target, proxy_envelope) = rows::route_target_from_model(&model)?;
            target.proxy_url_override = write::decrypt_optional(manager, proxy_envelope.as_ref())?
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            route.targets.push(target);
        }
        Ok(Some(route))
    }

    pub async fn list_route_targets_for(
        &self,
        manager: &RelaySecretManager,
        rule_id: uuid::Uuid,
    ) -> Result<Vec<ModelRouteTargetConfig>> {
        let db = sqlite_connection(&self.pool);
        let models = TargetEntity::find()
            .filter(TargetColumn::RuleId.eq(rule_id.to_string()))
            .order_by_asc(TargetColumn::Position)
            .all(&db)
            .await
            .map_err(rows::native_error)?;
        let mut targets = Vec::with_capacity(models.len());
        for model in &models {
            let (_, mut target, proxy_envelope) = rows::route_target_from_model(model)?;
            target.proxy_url_override = write::decrypt_optional(manager, proxy_envelope.as_ref())?
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty());
            targets.push(target);
        }
        Ok(targets)
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
        self.save_route_direct(manager, route).await
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
        let db = sqlite_connection(&self.pool);
        let transaction = db.begin().await.map_err(rows::native_error)?;
        delete_route_targets(&transaction, route.rule_id).await?;
        write::insert_route(&transaction, manager, route).await?;
        transaction.commit().await.map_err(rows::native_error)?;
        Ok(())
    }

    pub async fn delete_route(&self, rule_id: uuid::Uuid) -> Result<bool> {
        let db = sqlite_connection(&self.pool);
        let transaction = db.begin().await.map_err(rows::native_error)?;
        delete_route_targets(&transaction, rule_id).await?;
        let result = RouteEntity::delete_many()
            .filter(RouteColumn::RuleId.eq(rule_id.to_string()))
            .exec(&transaction)
            .await
            .map_err(rows::native_error)?;
        let removed = result.rows_affected > 0;
        transaction.commit().await.map_err(rows::native_error)?;
        Ok(removed)
    }
}

/// Removes the targets of one route so the next save replaces them whole.
async fn delete_route_targets(
    transaction: &sea_orm::DatabaseTransaction,
    rule_id: uuid::Uuid,
) -> Result<()> {
    write::delete_route_targets(transaction, Some(rule_id)).await
}
