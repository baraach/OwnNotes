//! Platform layers.
//!
//! Shared note, notebook, sync and storage code lives in `lib.rs`. Only the
//! things that genuinely differ per operating system live here, so a fix for
//! one platform does not touch the others:
//!
//! * `macos.rs`    — macOS windows, printing, open/reveal and clipboard.
//! * `windows.rs`  — Windows windows, printing, open/reveal and clipboard.
//! * `linux.rs`    — Linux windows, printing, open/reveal and clipboard.
//! * `ios.rs`      — iPhone/iPad: sandbox data directory, mobile stubs.
//! * `android.rs`  — Android: sandbox data directory, mobile stubs.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "android")]
mod android;

#[cfg(target_os = "macos")]
pub use macos::*;
#[cfg(target_os = "windows")]
pub use windows::*;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "ios")]
pub use ios::*;
#[cfg(target_os = "android")]
pub use android::*;

/// Extensions that must be shown in the file manager rather than launched.
pub(crate) fn is_runnable(path: &std::path::Path) -> bool {
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
    RUNNABLE.contains(&ext.as_str())
}
