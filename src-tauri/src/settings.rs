use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("无法确定配置目录")]
    MissingConfigDir,
    #[error("无法读写设置：{0}")]
    Io(#[from] std::io::Error),
    #[error("设置格式无效：{0}")]
    Json(#[from] serde_json::Error),
    #[error("API 地址必须以 http:// 或 https:// 开头")]
    InvalidEndpoint,
    #[error("模型名称不能为空")]
    EmptyModel,
    #[error("目标语言不能为空")]
    EmptyTarget,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub source_language: String,
    pub target_language: String,
    pub launch_at_login: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            endpoint: "https://api.openai.com/v1".into(),
            api_key: String::new(),
            model: "gpt-4.1-mini".into(),
            source_language: "自动检测".into(),
            target_language: "简体中文".into(),
            launch_at_login: false,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicSettings {
    endpoint: String,
    model: String,
    source_language: String,
    target_language: String,
    launch_at_login: bool,
    has_api_key: bool,
}

impl From<Settings> for PublicSettings {
    fn from(value: Settings) -> Self {
        Self {
            endpoint: value.endpoint,
            model: value.model,
            source_language: value.source_language,
            target_language: value.target_language,
            launch_at_login: value.launch_at_login,
            has_api_key: !value.api_key.is_empty(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsInput {
    endpoint: String,
    api_key: String,
    model: String,
    source_language: String,
    target_language: String,
    launch_at_login: bool,
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, SettingsError> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|_| SettingsError::MissingConfigDir)?;
    Ok(dir.join("settings.json"))
}

pub fn load_settings(app: &AppHandle) -> Result<Settings, SettingsError> {
    let path = settings_path(app)?;
    if !path.exists() {
        return Ok(Settings::default());
    }
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

pub fn save_settings(app: &AppHandle, input: SettingsInput) -> Result<Settings, SettingsError> {
    let mut current = load_settings(app)?;
    let endpoint = input.endpoint.trim().trim_end_matches('/').to_owned();
    if !(endpoint.starts_with("https://") || endpoint.starts_with("http://")) {
        return Err(SettingsError::InvalidEndpoint);
    }
    if input.model.trim().is_empty() {
        return Err(SettingsError::EmptyModel);
    }
    if input.target_language.trim().is_empty() {
        return Err(SettingsError::EmptyTarget);
    }

    current.endpoint = endpoint;
    current.model = input.model.trim().to_owned();
    current.source_language = input.source_language.trim().to_owned();
    current.target_language = input.target_language.trim().to_owned();
    current.launch_at_login = input.launch_at_login;
    if !input.api_key.trim().is_empty() {
        current.api_key = input.api_key.trim().to_owned();
    }

    let path = settings_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(&current)?)?;
    Ok(current)
}
