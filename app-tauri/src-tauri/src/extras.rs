//! The small things UpNote keeps besides notes: templates, the app lock and
//! per-window preferences. They live in `app.json` next to the library.

use std::fs;
use std::path::{Path, PathBuf};

use ownnotes_core::cloud::engine::hash;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct Template {
    pub name: String,
    pub body: String,
}

#[derive(Serialize, Deserialize, Default)]
pub struct Extras {
    #[serde(default)]
    pub templates: Vec<Template>,
    /// SHA-256 of the passcode, when the app is locked.
    #[serde(default)]
    pub lock: Option<String>,
    #[serde(default)]
    pub always_on_top: bool,
    /// Small interface choices the UI keeps for itself.
    #[serde(default)]
    pub ui: serde_json::Map<String, serde_json::Value>,
    /// Notebooks hidden behind the app passcode (on this computer).
    #[serde(default)]
    pub locked_notebooks: Vec<u64>,
    /// Spaces hidden behind the app passcode (on this computer).
    #[serde(default)]
    pub locked_spaces: Vec<String>,
    /// Where the window was, and how big: x, y, width, height.
    #[serde(default)]
    pub window: Option<(i32, i32, u32, u32)>,
}

fn path(dir: &Path) -> PathBuf {
    dir.join("app.json")
}

pub fn load(dir: &Path) -> Extras {
    fs::read_to_string(path(dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(dir: &Path, extras: &Extras) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(extras).map_err(|e| e.to_string())?;
    fs::write(path(dir), text).map_err(|e| e.to_string())
}

pub fn hash_passcode(passcode: &str) -> String {
    hash(passcode)
}
