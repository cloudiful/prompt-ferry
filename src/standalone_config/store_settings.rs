use crate::standalone_config::rows;
use crate::standalone_config::{Result, SettingConfig, StandaloneConfig};

use super::StandaloneConfigStore;

impl StandaloneConfigStore {
    pub async fn get_setting(&self, key: &str) -> Result<Option<SettingConfig>> {
        let row = standalone_query!("src/sql/standalone/get_setting.sql")
            .bind(key)
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else { return Ok(None) };
        Ok(Some(rows::setting(&row)?))
    }

    pub async fn set_setting(&self, setting: &SettingConfig) -> Result<()> {
        let snapshot = StandaloneConfig {
            settings: vec![setting.clone()],
            ..StandaloneConfig::default()
        };
        snapshot.validate()?;
        standalone_query!("src/sql/standalone/save_setting.sql")
            .bind(&setting.key)
            .bind(setting.version)
            .bind(serde_json::to_string(&setting.value)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_setting(&self, key: &str) -> Result<bool> {
        let result = standalone_query!("src/sql/standalone/delete_setting.sql")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
