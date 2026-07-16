pub mod yfs;

use yfs::commands::{
    yfs_compact, yfs_delete, yfs_list, yfs_open, yfs_read, yfs_read_bytes, yfs_rename, yfs_stat,
    yfs_write, yfs_write_bytes,
};
use yfs::YfsState;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(YfsState::new())
        .invoke_handler(tauri::generate_handler![
            greet,
            yfs_open,
            yfs_list,
            yfs_stat,
            yfs_read,
            yfs_read_bytes,
            yfs_write,
            yfs_write_bytes,
            yfs_delete,
            yfs_rename,
            yfs_compact,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
