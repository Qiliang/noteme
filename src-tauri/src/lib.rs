mod crypto;
mod fs_store;
mod git_sync;
mod settings;

use settings::SettingsState;
use tauri::Manager;

/// `tauri dev` runs a bare binary (no .app bundle), so macOS Dock
/// ignores `bundle.icon`. Set the Dock image at runtime from the
/// same PNG used for packaging.
#[cfg(target_os = "macos")]
fn set_macos_dock_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    const ICON_PNG: &[u8] = include_bytes!("../icons/icon.png");

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(ICON_PNG);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    // SAFETY: called from Tauri setup on the main thread.
    unsafe {
        NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            set_macos_dock_icon();
            let state = SettingsState::new();
            settings::load_into(app.handle(), &state).map_err(|e| {
                Box::<dyn std::error::Error>::from(e)
            })?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            settings::settings_get,
            settings::settings_set,
            crypto::crypto_encrypt,
            crypto::crypto_decrypt,
            fs_store::fs_list,
            fs_store::fs_stat,
            fs_store::fs_read,
            fs_store::fs_read_bytes,
            fs_store::fs_write,
            fs_store::fs_write_bytes,
            fs_store::fs_delete,
            fs_store::fs_rename,
            fs_store::fs_find_orphan_assets,
            git_sync::git_status,
            git_sync::git_init,
            git_sync::git_set_remote,
            git_sync::git_commit,
            git_sync::git_pull,
            git_sync::git_push,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
