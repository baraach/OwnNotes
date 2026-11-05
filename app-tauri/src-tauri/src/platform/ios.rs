//! iPhone and iPad: sandboxed data, system opener, mobile UI stubs.

use std::path::{Path, PathBuf};

use tauri::{App, AppHandle, Manager, Runtime, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

pub fn data_dir<R: Runtime>(app: &App<R>) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| ownnotes_core::store::data_dir())
}

pub fn restore_window<R: Runtime>(_app: &App<R>) {}

pub fn print(_window: &WebviewWindow) -> Result<(), String> {
    Err("Printing is not available on mobile.".into())
}

pub fn set_always_on_top(_window: &WebviewWindow, _on: bool) -> Result<(), String> {
    Ok(())
}

pub fn open_note_window(_app: AppHandle, _id: u64) -> Result<(), String> {
    Err("Opening a note in its own window is not available on mobile.".into())
}

pub fn open_url(app: &AppHandle, url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| "That isn't a web address".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only web links can be opened".into());
    }
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|e| e.to_string())
}

pub fn open_path(app: &AppHandle, path: &Path) -> Result<(), String> {
    if crate::platform::is_runnable(path) {
        return reveal(app, path);
    }
    app.opener()
        .open_path(path.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

pub fn reveal(app: &AppHandle, path: &Path) -> Result<(), String> {
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|e| e.to_string())
}

pub fn copy_text(_text: &str) -> Result<(), String> {
    Err("Clipboard access is handled by the web view on this platform.".into())
}
