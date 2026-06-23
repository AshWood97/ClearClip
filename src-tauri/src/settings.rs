use keyring::use_native_store;
use keyring_core::{Entry, Error as KeyringError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use thiserror::Error;
use tokio::sync::RwLock;

const KEYRING_SERVICE: &str = "ClearClip";
const KEYRING_USER: &str = "runninghub-api-key";

static KEYRING_INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("设置文件读写失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("设置文件格式错误：{0}")]
    Json(#[from] serde_json::Error),
    #[error("系统凭据访问失败：{0}")]
    Keyring(String),
}

pub type Result<T> = std::result::Result<T, SettingsError>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub model_overrides: HashMap<String, ModelOverride>,
    #[serde(default)]
    pub api_key_verified_at: Option<String>,
    #[serde(default)]
    pub api_key_last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ModelOverride {
    #[serde(default)]
    pub node_id: String,
    #[serde(default)]
    pub field_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSnapshot {
    pub has_api_key: bool,
    pub settings: AppSettings,
}

pub struct SettingsStore {
    path: PathBuf,
    settings: RwLock<AppSettings>,
}

impl SettingsStore {
    pub async fn new(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let settings = match tokio::fs::read_to_string(&path).await {
            Ok(content) => serde_json::from_str::<AppSettings>(&content)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppSettings::default(),
            Err(error) => return Err(error.into()),
        };

        Ok(Self {
            path,
            settings: RwLock::new(settings),
        })
    }

    pub async fn snapshot(&self) -> Result<SettingsSnapshot> {
        Ok(SettingsSnapshot {
            has_api_key: has_api_key()?,
            settings: self.settings.read().await.clone(),
        })
    }

    pub async fn save_model_override(
        &self,
        app_id: String,
        override_config: Option<ModelOverride>,
    ) -> Result<AppSettings> {
        let mut settings = self.settings.write().await;
        match override_config {
            Some(config)
                if !config.node_id.trim().is_empty() && !config.field_name.trim().is_empty() =>
            {
                settings.model_overrides.insert(
                    app_id,
                    ModelOverride {
                        node_id: config.node_id.trim().to_string(),
                        field_name: config.field_name.trim().to_string(),
                    },
                );
            }
            _ => {
                settings.model_overrides.remove(&app_id);
            }
        }
        self.persist(&settings).await?;
        Ok(settings.clone())
    }

    pub async fn model_override(&self, app_id: &str) -> Option<ModelOverride> {
        self.settings
            .read()
            .await
            .model_overrides
            .get(app_id)
            .cloned()
    }

    pub async fn mark_api_key_verified(&self, verified_at: String) -> Result<AppSettings> {
        let mut settings = self.settings.write().await;
        settings.api_key_verified_at = Some(verified_at);
        settings.api_key_last_error = None;
        self.persist(&settings).await?;
        Ok(settings.clone())
    }

    pub async fn mark_api_key_error(&self, error: String) -> Result<AppSettings> {
        let mut settings = self.settings.write().await;
        settings.api_key_last_error = Some(error);
        self.persist(&settings).await?;
        Ok(settings.clone())
    }

    pub async fn clear_api_key_state(&self) -> Result<AppSettings> {
        let mut settings = self.settings.write().await;
        settings.api_key_verified_at = None;
        settings.api_key_last_error = None;
        self.persist(&settings).await?;
        Ok(settings.clone())
    }

    async fn persist(&self, settings: &AppSettings) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let content = serde_json::to_string_pretty(settings)?;
        tokio::fs::write(&self.path, content).await?;
        Ok(())
    }
}

pub fn save_api_key(api_key: &str) -> Result<()> {
    ensure_keyring()?;
    entry()?
        .set_password(api_key.trim())
        .map_err(map_keyring_error)
}

pub fn get_api_key() -> Result<String> {
    ensure_keyring()?;
    entry()?.get_password().map_err(map_keyring_error)
}

pub fn has_api_key() -> Result<bool> {
    ensure_keyring()?;
    match entry()?.get_password() {
        Ok(value) => Ok(!value.trim().is_empty()),
        Err(KeyringError::NoEntry) => Ok(false),
        Err(error) => Err(map_keyring_error(error)),
    }
}

pub fn clear_api_key() -> Result<()> {
    ensure_keyring()?;
    match entry()?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(map_keyring_error(error)),
    }
}

fn ensure_keyring() -> Result<()> {
    let result =
        KEYRING_INIT.get_or_init(|| use_native_store(false).map_err(|error| error.to_string()));
    result.clone().map_err(SettingsError::Keyring)
}

fn entry() -> Result<Entry> {
    Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(map_keyring_error)
}

fn map_keyring_error(error: KeyringError) -> SettingsError {
    SettingsError::Keyring(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn saves_and_removes_model_override() {
        let path =
            std::env::temp_dir().join(format!("clearclip-settings-{}.json", uuid::Uuid::new_v4()));
        let store = SettingsStore::new(path.clone()).await.expect("store");

        let settings = store
            .save_model_override(
                "app".into(),
                Some(ModelOverride {
                    node_id: " 7 ".into(),
                    field_name: " video ".into(),
                }),
            )
            .await
            .expect("save");
        assert_eq!(settings.model_overrides["app"].node_id, "7");
        assert_eq!(settings.model_overrides["app"].field_name, "video");

        let settings = store
            .save_model_override("app".into(), None)
            .await
            .expect("remove");
        assert!(!settings.model_overrides.contains_key("app"));

        let _ = tokio::fs::remove_file(path).await;
    }

    #[tokio::test]
    async fn records_api_key_validation_state() {
        let path =
            std::env::temp_dir().join(format!("clearclip-settings-{}.json", uuid::Uuid::new_v4()));
        let store = SettingsStore::new(path.clone()).await.expect("store");

        let settings = store
            .mark_api_key_error("API Key 未授权".into())
            .await
            .expect("error state");
        assert_eq!(
            settings.api_key_last_error.as_deref(),
            Some("API Key 未授权")
        );

        let settings = store
            .mark_api_key_verified("123456".into())
            .await
            .expect("verified state");
        assert_eq!(settings.api_key_verified_at.as_deref(), Some("123456"));
        assert!(settings.api_key_last_error.is_none());

        let settings = store.clear_api_key_state().await.expect("clear state");
        assert!(settings.api_key_verified_at.is_none());
        assert!(settings.api_key_last_error.is_none());

        let _ = tokio::fs::remove_file(path).await;
    }
}
