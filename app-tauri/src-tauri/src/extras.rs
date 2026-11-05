//! The small things UpNote keeps besides notes: templates, the app lock and
//! per-window preferences. They live in `app.json` next to the library.
//! Which notebooks and spaces are locked lives in `locks.json`, which syncs;
//! `app.json` holds the passcode verifier and never leaves the device.

use std::fs;
use std::path::{Path, PathBuf};

use ownnotes_core::store::LOCKS_FILE;
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
    /// Verifier for the app passcode (`pbkdf2$…`, or a legacy unsalted
    /// SHA-256 hex digest), when the app is locked.
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

/// Which notebooks and spaces are locked — the synced half of the lock state.
#[derive(Serialize, Deserialize, Default, Clone)]
pub struct LockLists {
    #[serde(default)]
    pub locked_notebooks: Vec<u64>,
    #[serde(default)]
    pub locked_spaces: Vec<String>,
}

fn path(dir: &Path) -> PathBuf {
    dir.join("app.json")
}

fn locks_path(dir: &Path) -> PathBuf {
    dir.join(LOCKS_FILE)
}

/// Read `app.json`. `Ok(None)` means the file doesn't exist yet (first
/// launch); `Err` means it exists but couldn't be read or parsed — that must
/// be reported, not silently turned into defaults, or a damaged file would
/// drop the app lock on the next save.
pub fn load_checked(dir: &Path) -> Result<Option<Extras>, String> {
    match fs::read_to_string(path(dir)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("app.json could not be read ({e})")),
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("app.json is damaged and was ignored ({e})")),
    }
}

pub fn load(dir: &Path) -> Extras {
    load_checked(dir).ok().flatten().unwrap_or_default()
}

/// Move a damaged `app.json` out of the way, so the next save can't replace
/// what may still be recovered from it. Returns the name it was kept under.
pub fn set_aside(dir: &Path) -> Result<String, String> {
    let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
    let name = format!("app.json.damaged-{stamp}");
    fs::rename(path(dir), dir.join(&name)).map_err(|e| e.to_string())?;
    Ok(name)
}

/// Write `app.json` atomically: a crash or full disk mid-write can't leave a
/// half-written file behind.
pub fn save(dir: &Path, extras: &Extras) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(extras).map_err(|e| e.to_string())?;
    write_atomic(&path(dir), &text)
}

/// The effective lock lists: `app.json` merged with the synced `locks.json`,
/// so a lock set on another device applies here too.
pub fn lock_lists(dir: &Path) -> LockLists {
    let local = load(dir);
    let mut lists = LockLists {
        locked_notebooks: local.locked_notebooks,
        locked_spaces: local.locked_spaces,
    };
    if let Ok(text) = fs::read_to_string(locks_path(dir)) {
        if let Ok(synced) = serde_json::from_str::<LockLists>(&text) {
            for id in synced.locked_notebooks {
                if !lists.locked_notebooks.contains(&id) {
                    lists.locked_notebooks.push(id);
                }
            }
            for space in synced.locked_spaces {
                if !lists.locked_spaces.contains(&space) {
                    lists.locked_spaces.push(space);
                }
            }
        }
    }
    lists
}

/// Write the synced `locks.json` atomically.
pub fn save_lock_lists(dir: &Path, lists: &LockLists) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(lists).map_err(|e| e.to_string())?;
    write_atomic(&locks_path(dir), &text)
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_damaged_file_survives_the_next_save() {
        let dir = std::env::temp_dir().join(format!("ownnotes-test-extras-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(path(&dir), "{ \"lock\": \"half a fi").unwrap();

        assert!(load_checked(&dir).is_err(), "damage is reported");
        let kept = set_aside(&dir).unwrap();
        assert!(matches!(load_checked(&dir), Ok(None)), "the name is free again");

        save(&dir, &load(&dir)).unwrap();
        assert_eq!(fs::read_to_string(dir.join(kept)).unwrap(), "{ \"lock\": \"half a fi");
        let _ = fs::remove_dir_all(&dir);
    }
}
