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
