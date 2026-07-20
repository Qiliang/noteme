use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitSettings {
    #[serde(default)]
    pub remote_url: String,
    #[serde(default)]
    pub https_token: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CryptoLabel {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub passphrase: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub document_root: String,
    #[serde(default)]
    pub git: GitSettings,
    /// Named passphrases stored only on this machine (settings.json).
    #[serde(default)]
    pub crypto_labels: Vec<CryptoLabel>,
}

pub struct SettingsState {
    pub inner: Mutex<AppSettings>,
    pub path: Mutex<Option<PathBuf>>,
}

impl SettingsState {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(AppSettings::default()),
            path: Mutex::new(None),
        }
    }
}

fn settings_file(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取应用数据目录: {e}"))?;
    fs::create_dir_all(&base).map_err(|e| format!("创建应用数据目录失败: {e}"))?;
    Ok(base.join("settings.json"))
}

pub fn load_into(app: &AppHandle, state: &SettingsState) -> Result<(), String> {
    let path = settings_file(app)?;
    let settings = if path.exists() {
        let raw = fs::read_to_string(&path).map_err(|e| format!("读取设置失败: {e}"))?;
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        AppSettings::default()
    };
    *state
        .inner
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())? = settings;
    *state
        .path
        .lock()
        .map_err(|_| "settings path lock poisoned".to_string())? = Some(path);
    Ok(())
}

pub fn persist(state: &SettingsState) -> Result<(), String> {
    let path = state
        .path
        .lock()
        .map_err(|_| "settings path lock poisoned".to_string())?
        .clone()
        .ok_or_else(|| "设置尚未初始化".to_string())?;
    let settings = state
        .inner
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?
        .clone();
    let raw = serde_json::to_string_pretty(&settings).map_err(|e| format!("序列化设置失败: {e}"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建设置目录失败: {e}"))?;
    }
    fs::write(&path, raw).map_err(|e| format!("写入设置失败: {e}"))?;
    Ok(())
}

pub fn get_settings(state: &SettingsState) -> Result<AppSettings, String> {
    Ok(state
        .inner
        .lock()
        .map_err(|_| "settings lock poisoned".to_string())?
        .clone())
}

pub fn document_root(state: &SettingsState) -> Result<PathBuf, String> {
    let settings = get_settings(state)?;
    let root = settings.document_root.trim();
    if root.is_empty() {
        return Err("尚未配置文档根目录，请先在设置中选择".to_string());
    }
    let path = PathBuf::from(root);
    if !path.is_dir() {
        return Err(format!("文档根目录不存在或不是目录: {root}"));
    }
    Ok(path)
}

#[tauri::command]
pub fn settings_get(state: State<'_, SettingsState>) -> Result<AppSettings, String> {
    get_settings(&state)
}

#[tauri::command]
pub fn settings_set(
    state: State<'_, SettingsState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    {
        let mut guard = state
            .inner
            .lock()
            .map_err(|_| "settings lock poisoned".to_string())?;
        *guard = settings;
    }
    persist(&state)?;
    get_settings(&state)
}

