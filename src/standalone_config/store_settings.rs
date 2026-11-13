use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, Set};

use crate::db::entities::sqlite::standalone_settings::{ActiveModel, Column, Entity};
use crate::db::seaorm::sqlite_connection;
use crate::standalone_config::rows;
use crate::standalone_config::{Result, SettingConfig, StandaloneConfig};

use super::StandaloneConfigStore;

impl StandaloneConfigStore {
    pub async fn get_setting(&self, key: &str) -> Result<Option<SettingConfig>> {
        let db = sqlite_connection(&self.pool);
        let model = Entity::find_by_id(key)
            .one(&db)
            .await
            .map_err(rows::native_error)?;
        model.as_ref().map(rows::setting_from_model).transpose()
    }

    pub async fn set_setting(&self, setting: &SettingConfig) -> Result<()> {
        let snapshot = StandaloneConfig {
            settings: vec![setting.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        let db = sqlite_connection(&self.pool);
        Entity::insert(ActiveModel {
            setting_key: Set(setting.key.clone()),
            value_version: Set(setting.version),
            value_json: Set(serde_json::to_string(&setting.value)?),
            ..Default::default()
        })
        .on_conflict(
            OnConflict::column(Column::SettingKey)
                .update_columns([Column::ValueVersion, Column::ValueJson])
                .value(Column::UpdatedAt, Expr::current_timestamp())
                .to_owned(),
        )
        .exec(&db)
        .await
        .map_err(rows::native_error)?;
        Ok(())
    }

    pub async fn delete_setting(&self, key: &str) -> Result<bool> {
        let db = sqlite_connection(&self.pool);
        let result = Entity::delete_many()
            .filter(Column::SettingKey.eq(key))
            .exec(&db)
            .await
            .map_err(rows::native_error)?;
        Ok(result.rows_affected > 0)
    }
}
