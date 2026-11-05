//! Windows: native title bar, printing and file opening.

use std::path::{Path, PathBuf};

use tauri::{App, AppHandle, Manager, Runtime, WebviewWindow};

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

pub fn open_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| "That isn't a web address".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only web links can be opened".into());
    }
    system_open(parsed.as_str())
}

pub fn open_path(path: &Path) -> Result<(), String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    const RUNNABLE: &[&str] = &[
        "app", "command", "tool", "sh", "zsh", "bash", "scpt", "applescript", "workflow", "pkg",
        "mpkg", "jar", "terminal", "exe", "bat", "cmd", "com", "msi", "ps1", "vbs", "lnk", "desktop",
        "appimage", "run", "py", "pl", "rb", "webloc", "inetloc", "fileloc",
    ];
    if RUNNABLE.contains(&ext.as_str()) {
        return reveal(path);
    }
    system_open(path)
}

pub fn copy_text(text: &str) -> Result<(), String> {
    use std::io::Write;
    let mut command = std::process::Command::new("clip");
    let mut child = command.stdin(std::process::Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    child.stdin.take().ok_or("No clipboard input")?.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    child.wait().map_err(|e| e.to_string())?;
    Ok(())
}

fn system_open(target: impl AsRef<std::ffi::OsStr>) -> Result<(), String> {
    let mut command = std::process::Command::new("cmd");
    command.args(["/C", "start", ""]).arg(target);
    command.spawn().map(drop).map_err(|e| e.to_string())
}

pub fn reveal(path: &Path) -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map(drop)
        .map_err(|e| e.to_string())
}
