use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use super::error::Error;
use super::format::{FILE_TYPE_MARKDOWN, FILE_TYPE_TEXT};
use super::store::{Meta, Store};

pub struct YfsState {
    pub store: Mutex<Option<Store>>,
}

impl YfsState {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(None),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub name: String,
    pub size: u64,
    pub ctime_ms: u64,
    pub mtime_ms: u64,
    pub atime_ms: u64,
    pub file_type: u16,
    pub content_hash: String,
}

impl From<Meta> for FileInfo {
    fn from(m: Meta) -> Self {
        Self {
            name: m.name,
            size: m.size,
            ctime_ms: m.ctime_ms,
            mtime_ms: m.mtime_ms,
            atime_ms: m.atime_ms,
            file_type: m.file_type,
            content_hash: hex_encode(&m.content_hash),
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn map_err(err: Error) -> String {
    err.to_string()
}

fn with_store<F, T>(state: &YfsState, f: F) -> Result<T, String>
where
    F: FnOnce(&Store) -> super::Result<T>,
{
    let guard = state
        .store
        .lock()
        .map_err(|_| "yfs state lock poisoned".to_string())?;
    let store = guard.as_ref().ok_or_else(|| "yfs store not open".to_string())?;
    f(store).map_err(map_err)
}

/// Open (or create) the app-local yfs store under `{app_data}/yfs`.
#[tauri::command]
pub fn yfs_open(app: AppHandle, state: State<'_, YfsState>) -> Result<String, String> {
    let mut guard = state
        .store
        .lock()
        .map_err(|_| "yfs state lock poisoned".to_string())?;
    if let Some(store) = guard.as_ref() {
        return store.path().map(|p| p.display().to_string()).map_err(map_err);
    }

    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app data dir: {e}"))?;
    let dir = base.join("yfs");
    // 4096 slots is enough for notes MVP and keeps first create fast.
    let store = Store::open_with_entry_count(&dir, 4096).map_err(map_err)?;
    let path = dir.display().to_string();
    *guard = Some(store);
    Ok(path)
}

#[tauri::command]
pub fn yfs_list(state: State<'_, YfsState>, prefix: String) -> Result<Vec<FileInfo>, String> {
    with_store(&state, |store| {
        Ok(store.list(&prefix)?.into_iter().map(FileInfo::from).collect())
    })
}

#[tauri::command]
pub fn yfs_stat(state: State<'_, YfsState>, name: String) -> Result<FileInfo, String> {
    with_store(&state, |store| Ok(FileInfo::from(store.stat(&name)?)))
}

#[tauri::command]
pub fn yfs_read(state: State<'_, YfsState>, name: String) -> Result<String, String> {
    with_store(&state, |store| {
        let bytes = store.read(&name)?;
        String::from_utf8(bytes).map_err(|e| Error::Corrupt(format!("not utf-8: {e}")))
    })
}

/// Read raw bytes (images / attachments). Serialized as a number array over IPC.
#[tauri::command]
pub fn yfs_read_bytes(state: State<'_, YfsState>, name: String) -> Result<Vec<u8>, String> {
    with_store(&state, |store| store.read(&name))
}

#[tauri::command]
pub fn yfs_write(
    state: State<'_, YfsState>,
    name: String,
    content: String,
) -> Result<(), String> {
    with_store(&state, |store| {
        let file_type = if name.ends_with(".md") {
            FILE_TYPE_MARKDOWN
        } else {
            FILE_TYPE_TEXT
        };
        store.write_with_type(&name, content.as_bytes(), file_type)
    })
}

/// Import arbitrary binary content into yfs (from file picker / drag-drop).
#[tauri::command]
pub fn yfs_write_bytes(
    state: State<'_, YfsState>,
    name: String,
    data: Vec<u8>,
) -> Result<FileInfo, String> {
    with_store(&state, |store| {
        store.write(&name, &data)?;
        Ok(FileInfo::from(store.stat(&name)?))
    })
}

#[tauri::command]
pub fn yfs_delete(state: State<'_, YfsState>, name: String) -> Result<(), String> {
    with_store(&state, |store| store.delete(&name))
}

#[tauri::command]
pub fn yfs_rename(
    state: State<'_, YfsState>,
    old: String,
    new: String,
) -> Result<(), String> {
    with_store(&state, |store| store.rename(&old, &new))
}

#[tauri::command]
pub fn yfs_compact(state: State<'_, YfsState>) -> Result<(), String> {
    with_store(&state, |store| store.compact())
}
