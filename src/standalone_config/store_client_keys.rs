use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect, Set, TransactionTrait,
};

use crate::db::entities::sqlite::standalone_client_keys::{ActiveModel, Column, Entity};
use crate::db::entities::sqlite::standalone_users;
use crate::db::seaorm::sqlite_connection;
use crate::relay_secrets::RelaySecretManager;
use crate::standalone_config::rows;
use crate::standalone_config::{ClientKeyConfig, Result, StandaloneConfig, StandaloneConfigError};

use super::StandaloneConfigStore;

impl StandaloneConfigStore {
    pub async fn list_client_keys_for(
        &self,
        manager: &RelaySecretManager,
        user_id: i64,
        first: i64,
        rows: i64,
    ) -> Result<(i64, Vec<ClientKeyConfig>)> {
        let db = sqlite_connection(&self.pool);
        let total = Entity::find()
            .filter(Column::UserId.eq(user_id))
            .count(&db)
            .await
            .map_err(rows::native_error)? as i64;
        let models = Entity::find()
            .filter(Column::UserId.eq(user_id))
            .order_by_asc(Column::KeyId)
            .limit(rows.clamp(1, 200) as u64)
            .offset(first.max(0) as u64)
            .all(&db)
            .await
            .map_err(rows::native_error)?;
        let mut keys = Vec::with_capacity(models.len());
        for model in models {
            let (mut key, envelope) = rows::client_key_from_model(&model)?;
            key.secret = manager.decrypt(&envelope)?;
            keys.push(key);
        }
        Ok((total, keys))
    }

    pub async fn get_client_key(
        &self,
        manager: &RelaySecretManager,
        key_id: uuid::Uuid,
    ) -> Result<Option<ClientKeyConfig>> {
        let db = sqlite_connection(&self.pool);
        let model = Entity::find()
            .filter(Column::KeyId.eq(key_id.to_string()))
            .one(&db)
            .await
            .map_err(rows::native_error)?;
        let Some(model) = model else { return Ok(None) };
        let (mut key, envelope) = rows::client_key_from_model(&model)?;
        key.secret = manager.decrypt(&envelope)?;
        Ok(Some(key))
    }

    pub async fn save_client_key(
        &self,
        manager: &RelaySecretManager,
        client_key: &ClientKeyConfig,
    ) -> Result<()> {
        let snapshot = StandaloneConfig {
            client_keys: vec![client_key.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        let secret = manager
            .encrypt(&client_key.secret)
            .map_err(StandaloneConfigError::Encryption)?;
        let db = sqlite_connection(&self.pool);
        let transaction = db.begin().await.map_err(rows::native_error)?;
        ensure_legacy_user(&transaction, client_key.user_id).await?;
        Entity::insert(ActiveModel {
            key_id: Set(client_key.key_id.to_string()),
            user_id: Set(client_key.user_id),
            key_prefix: Set(client_key.key_prefix.clone()),
            label: Set(client_key.label.clone()),
            enabled: Set(i32::from(client_key.enabled)),
            secret_ciphertext: Set(secret.ciphertext),
            secret_nonce: Set(secret.nonce),
            secret_key_version: Set(i32::from(secret.key_version)),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::column(Column::KeyId)
                .update_columns([
                    Column::UserId,
                    Column::KeyPrefix,
                    Column::Label,
                    Column::Enabled,
                    Column::SecretCiphertext,
                    Column::SecretNonce,
                    Column::SecretKeyVersion,
                ])
                .value(Column::UpdatedAt, Expr::current_timestamp())
                .to_owned(),
        )
        .exec(&transaction)
        .await
        .map_err(rows::native_error)?;
        transaction.commit().await.map_err(rows::native_error)?;
        Ok(())
    }

    pub async fn update_client_key(
        &self,
        manager: &RelaySecretManager,
        user_id: i64,
        key_id: uuid::Uuid,
        label: Option<String>,
        enabled: Option<bool>,
    ) -> Result<Option<ClientKeyConfig>> {
        let key_id = key_id.to_string();
        let db = sqlite_connection(&self.pool);
        let transaction = db.begin().await.map_err(rows::native_error)?;
        let owned = match Entity::find()
            .filter(Column::UserId.eq(user_id))
            .filter(Column::KeyId.eq(key_id.clone()))
            .one(&transaction)
            .await
        {
            Ok(owned) => owned,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(rows::native_error(error));
            }
        };
        if owned.is_none() {
            let _ = transaction.rollback().await;
            return Ok(None);
        }

        let label = match label {
            Some(label) => {
                let label = label.trim().to_string();
                if label.is_empty() {
                    let _ = transaction.rollback().await;
                    return Err(StandaloneConfigError::InvalidInput {
                        field: "client key label",
                        message: "must not be empty".to_string(),
                    });
                }
                Some(label)
            }
            None => None,
        };
        let mut changes = <ActiveModel as ActiveModelTrait>::default();
        if let Some(label) = label {
            changes.label = Set(label);
        }
        if let Some(enabled) = enabled {
            changes.enabled = Set(i32::from(enabled));
        }
        let updated = match Entity::update_many()
            .set(changes)
            .col_expr(Column::UpdatedAt, Expr::current_timestamp())
            .filter(Column::UserId.eq(user_id))
            .filter(Column::KeyId.eq(key_id.clone()))
            .exec(&transaction)
            .await
        {
            Ok(updated) => updated,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(rows::native_error(error));
            }
        };
        if updated.rows_affected == 0 {
            let _ = transaction.rollback().await;
            return Ok(None);
        }
        let model = match Entity::find()
            .filter(Column::UserId.eq(user_id))
            .filter(Column::KeyId.eq(key_id))
            .one(&transaction)
            .await
        {
            Ok(model) => model,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(rows::native_error(error));
            }
        };
        let Some(model) = model else {
            let _ = transaction.rollback().await;
            return Ok(None);
        };
        let key = match rows::client_key_from_model(&model) {
            Ok((mut key, envelope)) => match manager.decrypt(&envelope) {
                Ok(secret) => {
                    key.secret = secret;
                    key
                }
                Err(error) => {
                    let _ = transaction.rollback().await;
                    return Err(StandaloneConfigError::Encryption(error));
                }
            },
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(error);
            }
        };
        transaction.commit().await.map_err(rows::native_error)?;
        Ok(Some(key))
    }

    pub async fn delete_client_key(&self, user_id: i64, key_id: uuid::Uuid) -> Result<bool> {
        let db = sqlite_connection(&self.pool);
        let result = Entity::delete_many()
            .filter(Column::UserId.eq(user_id))
            .filter(Column::KeyId.eq(key_id.to_string()))
            .exec(&db)
            .await
            .map_err(rows::native_error)?;
        Ok(result.rows_affected > 0)
    }
}

async fn ensure_legacy_user<C>(connection: &C, user_id: i64) -> Result<()>
where
    C: ConnectionTrait,
{
    let result = standalone_users::Entity::insert(standalone_users::ActiveModel {
        user_id: Set(user_id),
        login_name: Set(format!("legacy_user_{user_id}")),
        display_name: Set(format!("Legacy user {user_id}")),
        password_hash: Set("!".to_string()),
        is_admin: Set(0),
        enabled: Set(0),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::column(standalone_users::Column::UserId)
            .do_nothing()
            .to_owned(),
    )
    .exec(connection)
    .await;
    match result {
        Ok(_) | Err(sea_orm::DbErr::RecordNotInserted) => Ok(()),
        Err(error) => Err(rows::native_error(error)),
    }
}
