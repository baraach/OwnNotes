//! Android.

use std::path::{Path, PathBuf};

use tauri::{App, AppHandle, Manager, Runtime, WebviewWindow};

pub fn data_dir<R: Runtime>(app: &App<R>) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| ownnotes_core::store::data_dir())
}

pub fn restore_window<R: Runtime>(_app: &App<R>) {}

pub fn print(_window: &WebviewWindow) -> Result<(), String> {
    Err("Printing is not available on Android.".into())
}

pub fn set_always_on_top(_window: &WebviewWindow, _on: bool) -> Result<(), String> {
    Ok(())
}

pub fn open_note_window(_app: AppHandle, _id: u64) -> Result<(), String> {
    Err("Opening a note in its own window is not available on Android.".into())
}

pub fn open_url(_url: &str) -> Result<(), String> {
    Err("Opening links is not wired up on Android yet.".into())
}

pub fn open_path(_path: &Path) -> Result<(), String> {
    Err("Opening files is not wired up on Android yet.".into())
}

pub fn reveal(_path: &Path) -> Result<(), String> {
    Err("Revealing files is not available on Android.".into())
}

pub fn copy_text(_text: &str) -> Result<(), String> {
    Err("Clipboard access is handled by the web view on this platform.".into())
}
