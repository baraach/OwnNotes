//! Windows: native title bar, printing, files and clipboard.

use std::path::{Path, PathBuf};

use tauri::{App, AppHandle, Manager, Runtime, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

pub fn data_dir<R: Runtime>(_app: &App<R>) -> PathBuf {
    ownnotes_core::store::data_dir()
}

pub fn restore_window<R: Runtime>(app: &App<R>) {
    let dir = ownnotes_core::store::data_dir();
    let Some((x, y, w, h)) = crate::extras::load(&dir).window else {
        return;
    };
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let _ = window.set_size(tauri::PhysicalSize::new(w.max(700), h.max(480)));
    let on_screen = window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .any(|monitor| {
            let at = monitor.position();
            let size = monitor.size();
            x + 40 >= at.x
                && y + 20 >= at.y
                && x + 40 < at.x + size.width as i32
                && y + 20 < at.y + size.height as i32
        });
    if on_screen {
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

pub fn print(window: &WebviewWindow) -> Result<(), String> {
    window.print().map_err(|e| e.to_string())
}

pub fn set_always_on_top(window: &WebviewWindow, on: bool) -> Result<(), String> {
    window.set_always_on_top(on).map_err(|e| e.to_string())
}

pub fn open_note_window(app: AppHandle, id: u64) -> Result<(), String> {
    let label = format!("note-{id}");
    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.set_focus();
        return Ok(());
    }
    tauri::WebviewWindowBuilder::new(
        &app,
        label,
        tauri::WebviewUrl::App(format!("index.html?note={id}").into()),
    )
    .title("OwnNotes")
    .inner_size(760.0, 640.0)
    .min_inner_size(480.0, 360.0)
    .build()
    .map(|_| ())
    .map_err(|e| e.to_string())
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

pub fn copy_text(text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut command = std::process::Command::new("clip");
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    child
        .stdin
        .take()
        .ok_or("No clipboard input")?
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
    child.wait().map_err(|e| e.to_string())?;
    Ok(())
}
