// Persistence: keeping notes and settings between launches.
//
// Everything lives in one folder on this computer (see `data_dir`):
//
//   notes/note-<id>.md   one plain Markdown file per note, with a small header
//   library.json         the notebooks
//   settings.json        appearance, editor and sync choices
//
// Notes stay readable in any other app. Files are written atomically
// (write to a temporary file, then rename) so a crash never leaves half a note.
// Only notes that changed since the last save are rewritten.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration as ChronoDuration, Local};
use serde::{Deserialize, Serialize};

use crate::prefs::Prefs;
use crate::model::{Document, Note, Notebook};
use crate::sync::SyncSettings;

/// Where OwnNotes keeps its files. `OWNNOTES_DATA_DIR` overrides it.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("OWNNOTES_DATA_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/OwnNotes")
    } else if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join("OwnNotes")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("ownnotes")
    }
}

#[derive(Serialize, Deserialize)]
struct Library {
    version: u32,
    notebooks: Vec<Notebook>,
}

/// Settings saved to `settings.json`. The WebDAV password is never written.
#[derive(Serialize, Deserialize, Default)]
pub struct SavedSettings {
    #[serde(default)]
    pub prefs: Prefs,
    #[serde(default)]
    pub sync: SyncSettings,
}

pub struct Store {
    dir: PathBuf,
    /// Where each note is and what was last written there, so unchanged notes
    /// are skipped and a moved note (renamed notebook) doesn't leave a copy.
    written: HashMap<u64, (String, String)>,
    /// Extra copies of a note that lives in several notebooks: path → text.
    copies: HashMap<String, String>,
    library_written: String,
}

impl Store {
    /// Open the data folder and load what is in it. On first launch (an empty
    /// folder) this returns the sample notes, which are saved straight away.
    pub fn open() -> (Store, Document, SavedSettings, Vec<String>) {
        Store::open_at(data_dir())
    }

    pub fn open_at(dir: PathBuf) -> (Store, Document, SavedSettings, Vec<String>) {
        let mut store = Store {
            dir,
            written: HashMap::new(),
            copies: HashMap::new(),
            library_written: String::new(),
        };
        let mut problems = Vec::new();
        sweep_temp_files(&store.notes_dir());
        // Attachments use the same `.tmp.<pid>` convention in their own folder.
        sweep_temp_files(&store.dir.join(crate::attachments::LOCAL_FOLDER));

        let settings = match fs::read_to_string(store.dir.join("settings.json")) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                problems.push(format!("settings.json could not be read ({e}); using defaults"));
                SavedSettings::default()
            }),
            Err(_) => SavedSettings::default(),
        };

        let library: Option<Library> = fs::read_to_string(store.dir.join("library.json"))
            .ok()
            .and_then(|text| match serde_json::from_str(&text) {
                Ok(lib) => Some(lib),
                Err(e) => {
                    problems.push(format!("library.json could not be read ({e})"));
                    None
                }
            });

        // Notes sit in a folder per notebook, one level down. Older versions
        // kept them all in the root, so both are read.
        let mut notes = Vec::new();
        let root = store.notes_dir();
        for (path, rel) in note_files(&root) {
            match fs::read_to_string(&path).ok().and_then(|t| parse_note(&t)) {
                Some(mut note) => {
                    // A Markdown note is converted once, in place, and the old
                    // file is dropped when it is written back as HTML.
                    // (Sealed text is left exactly as it is, or it could never
                    // be opened again.)
                    if path.extension().is_some_and(|e| e == "md")
                        && !crate::crypto::is_encrypted(&note.body)
                    {
                        note.body = crate::html::markdown_to_html(&note.body);
                    }
                    // A note that lives in several notebooks has a file in
                    // each of their folders. The first one wins; the rest are
                    // recorded as copies so they are kept in step, not loaded
                    // as separate notes.
                    if notes.iter().any(|other: &Note| other.id == note.id) {
                        // Each copy carries the same header, so the one
                        // already loaded knows the whole list of notebooks.
                        store.copies.insert(rel, render_note(&note));
                        continue;
                    }
                    // Notes in the Deleted folder are marked as trashed.
                    let mut note = note;
                    if is_in_trash(&rel) {
                        note.trashed = true;
                    }
                    store.written.insert(note.id, (rel, render_note(&note)));
                    notes.push(note);
                }
                None => problems.push(format!("Skipped {} (not an OwnNotes note)", path.display())),
            }
        }

        let doc = match library {
            None if notes.is_empty() => {
                // First launch: start with the sample notes and save them.
                let doc = Document::sample();
                if let Err(e) = store.save(&doc) {
                    problems.push(format!("Could not save to {} ({e})", store.dir.display()));
                }
                doc
            }
            library => {
                let mut notebooks = library.map(|l| l.notebooks).unwrap_or_default();
                // Notes whose notebook is missing still need a home.
                for note in &notes {
                    if !notebooks.iter().any(|n| n.id == note.notebook_id) {
                        notebooks.push(Notebook {
                            parent: None,
                            id: note.notebook_id,
                            name: "Recovered".to_string(),
                            color: notebooks.len(),
                        });
                    }
                }
                if notebooks.is_empty() {
                    notebooks.push(Notebook { parent: None, id: 1, name: "Notes".to_string(), color: 0 });
                }
                let doc = Document::from_parts(notebooks, notes);
                store.library_written = render_library(&doc);
                store.settle_copies(&doc);
                doc
            }
        };
        // History left behind by older builds, or by a note removed during a
        // sync, belongs to no note any more. Skip this when the notes folder
        // itself had problems, so an unreadable note can't cost it its history.
        if problems.is_empty() {
            store.prune_orphan_versions(&doc);
        }
        (store, doc, settings, problems)
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Files are read in whatever order the disk lists them, so for a note
    /// in several notebooks the copy may have been read first. Make the file
    /// in the note's own notebook the one that counts, or the next save would
    /// treat the copy as a stale location and delete it.
    fn settle_copies(&mut self, doc: &Document) {
        for note in &doc.notes {
            let own = note_rel_path(doc.notebook_name(note.notebook_id), note.id, note.trashed);
            let Some((loaded, text)) = self.written.get(&note.id).cloned() else { continue };
            if loaded == own {
                continue;
            }
            if let Some(own_text) = self.copies.remove(&own) {
                self.copies.insert(loaded, text);
                self.written.insert(note.id, (own, own_text));
            }
        }
    }

    fn notes_dir(&self) -> PathBuf {
        self.dir.join("notes")
    }

    /// Whether two paths in the notes folder are the same file on disk (they
    /// can differ only in letter case on macOS and Windows).
    fn same_file(&self, a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        let (pa, pb) = (self.notes_dir().join(a), self.notes_dir().join(b));
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            match (fs::metadata(&pa), fs::metadata(&pb)) {
                (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
                _ => false,
            }
        }
        #[cfg(not(unix))]
        {
            pa.exists() && pb.exists() && a.to_lowercase() == b.to_lowercase()
        }
    }

    /// Delete a note file by its path relative to the notes folder.
    fn remove_note_file(&self, rel: &str) -> io::Result<()> {
        let path = self.notes_dir().join(rel);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        self.remove_empty_folder(&path);
        Ok(())
    }

    /// Drop the notebook folder once its last note is gone.
    /// Drop empty folders up the tree, stopping at the notes root. A note in
    /// `Work/.deleted/` leaves both `.deleted` and `Work` to be cleaned if
    /// they are empty.
    fn remove_empty_folder(&self, note_path: &Path) {
        let notes = self.notes_dir();
        let mut dir = note_path.parent();
        while let Some(d) = dir {
            if d == notes {
                break;
            }
            if fs::remove_dir(d).is_err() {
                // Not empty, or already gone — stop climbing.
                break;
            }
            dir = d.parent();
        }
    }

    /// Write every note that changed, remove files of deleted notes, and
    /// update the notebook list.
    pub fn save(&mut self, doc: &Document) -> io::Result<()> {
        fs::create_dir_all(self.notes_dir())?;
        let mut wanted: HashSet<String> = HashSet::new();
        for note in &doc.notes {
            let rel = note_rel_path(doc.notebook_name(note.notebook_id), note.id, note.trashed);
            let text = render_note(note);
            wanted.insert(rel.clone());

            // Copies in the other notebooks this note belongs to.
            for other in doc.notebooks_of(note) {
                if other == note.notebook_id {
                    continue;
                }
                let copy_rel = note_rel_path(doc.notebook_name(other), note.id, note.trashed);
                wanted.insert(copy_rel.clone());
                if self.copies.get(&copy_rel) == Some(&text) {
                    continue;
                }
                let path = self.notes_dir().join(&copy_rel);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                write_atomic(&path, &text)?;
                self.copies.insert(copy_rel, text.clone());
            }
            let unchanged = self
                .written
                .get(&note.id)
                .is_some_and(|(old_rel, old_text)| *old_rel == rel && *old_text == text);
            if unchanged {
                continue;
            }
            let path = self.notes_dir().join(&rel);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            write_atomic(&path, &text)?;
            // A renamed notebook moves the note; drop the file at the old
            // path — unless that path is a copy we still want, which happens
            // when the note's own file went missing and the copy was what
            // loaded.
            if let Some((old_rel, _)) = self.written.get(&note.id) {
                if *old_rel != rel
                    && !wanted.contains(old_rel.as_str())
                    && !self.same_file(old_rel, &rel)
                {
                    if let Err(e) = self.remove_note_file(old_rel) {
                        eprintln!("Could not remove old note file {old_rel}: {e}");
                    }
                }
            }
            self.written.insert(note.id, (rel, text));
            if let Err(e) = self.snapshot(doc.notebook_name(note.notebook_id), note, false) {
                eprintln!("Could not save a version of note {}: {e}", note.id);
            }
        }

        // Copies whose notebook was removed, or whose note is gone.
        let stale: Vec<String> = self
            .copies
            .keys()
            .filter(|rel| !wanted.contains(*rel))
            .cloned()
            .collect();
        for rel in stale {
            self.copies.remove(&rel);
            // On a disk that ignores letter case, "work/…" and "Work/…" are
            // one file: don't delete a file that is still wanted.
            if wanted.iter().any(|keep| self.same_file(keep, &rel)) {
                continue;
            }
            let path = self.notes_dir().join(&rel);
            match fs::remove_file(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            self.remove_empty_folder(&path);
        }
        let gone: Vec<u64> = self
            .written
            .keys()
            .copied()
            .filter(|id| doc.note(*id).is_none())
            .collect();
        for id in gone {
            if let Some((rel, _)) = self.written.remove(&id) {
                if let Err(e) = self.remove_note_file(&rel) {
                    eprintln!("Could not remove deleted note file {rel}: {e}");
                }
            }
        }
        let library = render_library(doc);
        if library != self.library_written {
            write_atomic(&self.dir.join(LIBRARY_FILE), &library)?;
            self.library_written = library;
        }
        Ok(())
    }

    pub fn save_settings(&self, settings: &SavedSettings) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let text = serde_json::to_string_pretty(settings).map_err(io::Error::other)?;
        write_atomic(&self.dir.join("settings.json"), &text)
    }
}

/// The file name a note is saved under, locally and in the cloud.
pub fn note_file_name(id: u64) -> String {
    format!("note-{id}.html")
}


/// The folder a notebook's notes live in — on disk and on the server. The
/// name is made safe for a path, so "/" and friends can't make a mess.
pub fn workspace_folder(name: &str) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                ' '
            }
        })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_matches('.').trim();
    if cleaned.is_empty() {
        "Notes".to_string()
    } else {
        cleaned.chars().take(60).collect()
    }
}

/// Where a note lives, relative to the notes folder and to the WebDAV folder.
pub fn note_rel_path(notebook_name: &str, id: u64, trashed: bool) -> String {
    let folder = workspace_folder(notebook_name);
    let base_path = if folder.trim().is_empty() {
        String::new()
    } else {
        format!("{folder}/")
    };
    let subfolder = if trashed { TRASH_FOLDER.to_owned() + "/" } else { String::new() };
    format!("{base_path}{subfolder}{}", note_file_name(id))
}

pub const LIBRARY_FILE: &str = "library.json";
/// Which notebooks and spaces are locked. This file syncs (unlike `app.json`,
/// which holds the passcode verifier and must never leave the device), so
/// every device knows which containers are locked.
pub const LOCKS_FILE: &str = "locks.json";

/// The folder where trashed notes are stored.
/// The folder trashed notes move into. A notebook can never be named this:
/// `workspace_folder` trims leading dots, so ".deleted" is not reachable
/// from a notebook name and can't collide with one.
pub const TRASH_FOLDER: &str = ".deleted";

/// Whether a path relative to the notes folder is in the trash.
pub fn is_in_trash(rel_path: &str) -> bool {
    rel_path
        .split('/')
        .any(|part| part == TRASH_FOLDER)
}

/// The notebooks inside a library.json text.
pub fn parse_library(text: &str) -> Option<Vec<Notebook>> {
    serde_json::from_str::<Library>(text).ok().map(|l| l.notebooks)
}

/// What the last sync looked like, kept in `sync-webdav.json` so the next
/// sync knows what changed on each side.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct SyncState {
    /// The server and folder this state belongs to. Pointing OwnNotes at a
    /// different folder starts from scratch instead of mistaking every file
    /// there for something that was deleted.
    pub source: String,
    pub files: HashMap<String, SyncedFile>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct SyncedFile {
    /// The file's URL on the server (the name is enough for WebDAV).
    pub remote_id: String,
    /// The server's revision for the file — its ETag.
    pub rev: String,
    /// Hash of the content both sides had after the last sync.
    pub hash: String,
}

pub const SYNC_STATE_FILE: &str = "sync-webdav.json";

/// An earlier version of one note, kept on this computer.
#[derive(Clone)]
pub struct Version {
    pub at: DateTime<Local>,
    pub body: String,
    pub title: String,
    /// Where the version file lives, so it can be removed whatever its
    /// extension or folder is.
    pub path: PathBuf,
}

/// How many versions of one note to keep, and how far apart they are taken.
const MAX_VERSIONS: usize = 40;
const VERSION_GAP_MINUTES: i64 = 2;

/// Old versions live in this folder — on this computer, and inside the
/// `OwnNotes` folder on the server, so they travel with the notes.
pub const VERSIONS_FOLDER: &str = "Versions";

impl Store {
    fn versions_dir(&self) -> PathBuf {
        self.dir.join("versions")
    }

    /// Old versions sit in `versions/<workspace>/note-<id>-<millis>.md`,
    /// which is `Versions/<workspace>/note-<id>-<millis>.md` on the server.
    fn note_versions_dir(&self, notebook_name: &str) -> PathBuf {
        self.versions_dir().join(workspace_folder(notebook_name))
    }

    fn version_name(id: u64, millis: i64) -> String {
        format!("note-{id}-{millis}.html")
    }

    /// Every saved version of a note, newest first. Versions are looked for
    /// in every workspace folder, not just the note's current notebook: a
    /// renamed notebook or a moved note must not hide its history.
    pub fn versions(&self, _notebook_name: &str, id: u64) -> Vec<Version> {
        let prefix = format!("note-{id}-");
        let Ok(folders) = fs::read_dir(self.versions_dir()) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for folder in folders.flatten() {
            let dir = folder.path();
            if !dir.is_dir() {
                continue;
            }
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(file) = path.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                if !file.starts_with(&prefix)
                    || path
                        .extension()
                        .is_none_or(|e| e != "md" && e != "html")
                {
                    continue;
                }
                let Some(millis) = file
                    .trim_start_matches(&prefix)
                    .trim_end_matches(".html")
                    .trim_end_matches(".md")
                    .parse::<i64>()
                    .ok()
                else {
                    continue;
                };
                let Some(at) = DateTime::from_timestamp_millis(millis).map(|t| t.with_timezone(&Local)) else {
                    continue;
                };
                let Some(note) = fs::read_to_string(&path).ok().and_then(|t| parse_note(&t)) else {
                    continue;
                };
                out.push(Version {
                    at,
                    body: note.body,
                    title: note.title,
                    path,
                });
            }
        }
        out.sort_by_key(|a| std::cmp::Reverse(a.at));
        out
    }

    /// Keep a copy of the note as it was, at most once every couple of
    /// minutes, and only the last few dozen.
    fn snapshot(&self, notebook_name: &str, note: &Note, force: bool) -> io::Result<()> {
        let dir = self.note_versions_dir(notebook_name);
        let prefix = format!("note-{}-", note.id);
        let mut existing: Vec<(i64, PathBuf)> = fs::read_dir(&dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|entry| {
                        let path = entry.path();
                        let file = path.file_name()?.to_str()?;
                        if !file.starts_with(&prefix) {
                            return None;
                        }
                        let millis = file
                            .trim_start_matches(&prefix)
                            .trim_end_matches(".html")
                .trim_end_matches(".md")
                            .parse::<i64>()
                            .ok()?;
                        Some((millis, path))
                    })
                    .collect()
            })
            .unwrap_or_default();

        if !force {
            if let Some((millis, _)) = existing.iter().max_by_key(|(millis, _)| *millis) {
                if let Some(at) = DateTime::from_timestamp_millis(*millis) {
                    let age = Local::now().signed_duration_since(at.with_timezone(&Local));
                    if age < ChronoDuration::minutes(VERSION_GAP_MINUTES) {
                        return Ok(());
                    }
                }
            }
        }

        fs::create_dir_all(&dir)?;
        let millis = Local::now().timestamp_millis();
        let path = dir.join(Self::version_name(note.id, millis));
        write_atomic(&path, &render_note(note))?;
        existing.push((millis, path));

        if existing.len() > MAX_VERSIONS {
            existing.sort_by_key(|(millis, _)| *millis);
            let drop = existing.len() - MAX_VERSIONS;
            for (_, path) in existing.into_iter().take(drop) {
                let _ = fs::remove_file(path);
            }
        }
        Ok(())
    }

    /// Every version file, named as it is on the server:
    /// `Versions/<workspace>/note-<id>-<millis>.md`.
    pub fn version_files(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let root = self.versions_dir();
        let Ok(folders) = fs::read_dir(&root) else {
            return out;
        };
        for folder in folders.flatten() {
            let dir = folder.path();
            if !dir.is_dir() {
                continue;
            }
            let Some(workspace) = dir.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(files) = fs::read_dir(&dir) else { continue };
            for file in files.flatten() {
                let path = file.path();
                if path
                    .extension()
                    .is_none_or(|e| e != "md" && e != "html")
                {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
                    continue;
                };
                let Ok(text) = fs::read_to_string(&path) else {
                    continue;
                };
                out.push((format!("{VERSIONS_FOLDER}/{workspace}/{name}"), text));
            }
        }
        out
    }

    /// Write a version that arrived from the server.
    pub fn write_version_file(&self, rel: &str, content: &str) {
        let Some(rest) = rel.strip_prefix(&format!("{VERSIONS_FOLDER}/")) else {
            return;
        };
        // The name comes from the server: no "..", no absolute or odd parts.
        let safe = rest.split('/').all(|part| {
            !part.is_empty() && part != "." && part != ".." && !part.contains(['\\', ':'])
        });
        if !safe {
            return;
        }
        let path = self.versions_dir().join(rest);
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = write_atomic(&path, content);
    }

    /// Remove a version that was removed elsewhere.
    pub fn remove_version_file(&self, rel: &str) {
        let Some(rest) = rel.strip_prefix(&format!("{VERSIONS_FOLDER}/")) else {
            return;
        };
        let _ = fs::remove_file(self.versions_dir().join(rest));
    }

    /// Remove a version file at its actual path (whatever folder and
    /// extension it has).
    pub fn remove_version_path(&self, path: &Path) {
        if let Err(error) = fs::remove_file(path) {
            if error.kind() != io::ErrorKind::NotFound {
                eprintln!("Could not remove version file {}: {error}", path.display());
            }
        }
    }

    /// Re-seal every stored version that opens with `from` using `to`
    /// (a passcode change), so history doesn't stay locked with the old one.
    /// Returns how many version files were re-sealed.
    pub fn reseal_versions(&self, from: &str, to: &str) -> usize {
        let mut done = 0;
        let Ok(folders) = fs::read_dir(self.versions_dir()) else {
            return 0;
        };
        for folder in folders.flatten() {
            let Ok(files) = fs::read_dir(folder.path()) else { continue };
            for file in files.flatten() {
                let path = file.path();
                if path.extension().is_none_or(|e| e != "md" && e != "html") {
                    continue;
                }
                let Ok(text) = fs::read_to_string(&path) else { continue };
                let Some(mut note) = parse_note(&text) else { continue };
                if !crate::crypto::is_encrypted(&note.body) {
                    continue;
                }
                let Some(plain) = crate::crypto::decrypt(from, &note.body) else {
                    continue;
                };
                note.body = crate::crypto::encrypt(to, &plain);
                if write_atomic(&path, &render_note(&note)).is_ok() {
                    done += 1;
                }
            }
        }
        done
    }

    /// Delete every stored version of a note (when it is locked, its older
    /// versions are readable and must not stay behind).
    pub fn forget_versions(&self, id: u64) {
        let prefix = format!("note-{id}-");
        let Ok(folders) = fs::read_dir(self.versions_dir()) else { return };
        for folder in folders.flatten() {
            let Ok(files) = fs::read_dir(folder.path()) else { continue };
            for file in files.flatten() {
                if file.file_name().to_str().is_some_and(|name| name.starts_with(&prefix)) {
                    let _ = fs::remove_file(file.path());
                }
            }
        }
    }

    /// Remove version files whose note no longer exists. Deleting through the
    /// interface already calls forget_versions; this also clears history left
    /// by older builds and by notes removed during a sync.
    pub fn prune_orphan_versions(&self, doc: &Document) {
        let live: HashSet<u64> = doc.notes.iter().map(|note| note.id).collect();
        let Ok(folders) = fs::read_dir(self.versions_dir()) else { return };
        for folder in folders.flatten() {
            let dir = folder.path();
            if !dir.is_dir() {
                continue;
            }
            let Ok(files) = fs::read_dir(&dir) else { continue };
            let mut kept = 0usize;
            for file in files.flatten() {
                let path = file.path();
                let is_version = path.extension().is_some_and(|ext| ext == "html" || ext == "md");
                let id = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .and_then(|name| name.strip_prefix("note-"))
                    .and_then(|rest| rest.split('-').next())
                    .and_then(|id| id.parse::<u64>().ok());
                match (is_version, id) {
                    (true, Some(id)) if !live.contains(&id) => {
                        let _ = fs::remove_file(&path);
                    }
                    _ => kept += 1,
                }
            }
            if kept == 0 {
                let _ = fs::remove_dir(&dir);
            }
        }
    }

    /// Save a version of a note right now (used before restoring one).
    pub fn snapshot_now(&self, notebook_name: &str, note: &Note) -> io::Result<()> {
        self.snapshot(notebook_name, note, true)
    }

    fn sync_state_path(&self) -> PathBuf {
        self.dir.join(SYNC_STATE_FILE)
    }

    /// The saved state for this exact server + folder, or a fresh one.
    pub fn load_sync_state(&self, source: &str) -> SyncState {
        fs::read_to_string(self.sync_state_path())
            .ok()
            .and_then(|t| serde_json::from_str::<SyncState>(&t).ok())
            .filter(|s| s.source == source)
            .unwrap_or_else(|| SyncState {
                source: source.to_string(),
                ..Default::default()
            })
    }

    pub fn save_sync_state(&self, state: &SyncState) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let text = serde_json::to_string_pretty(state).map_err(io::Error::other)?;
        write_atomic(&self.sync_state_path(), &text)
    }

    pub fn forget_sync_state(&self) {
        let _ = fs::remove_file(self.sync_state_path());
    }
}

/// Every note file under `root`, with its path relative to `root`. Notes sit
/// one folder deep (one per notebook); older versions kept them in the root.
fn note_files(root: &Path) -> Vec<(PathBuf, String)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, String)>, depth: usize) {
        if depth > 2 {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out, depth + 1);
                continue;
            }
            if path
                .extension()
                .is_none_or(|e| e != "md" && e != "html")
            {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((path, rel));
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out, 0);
    out
}

/// A write that was killed between its temporary file and the rename leaves
/// the temporary file behind. Clearing them at startup is safe: anything that
/// old belongs to a process that is gone.
fn sweep_temp_files(root: &Path) {
    fn walk(dir: &Path, depth: usize) {
        if depth > 2 {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, depth + 1);
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            // Temp files end in `.tmp.<pid>` (e.g. `note-123.tmp.4242`);
            // a plain ".tmp." anywhere in the name is not ours.
            let is_temp = name
                .rsplit_once(".tmp.")
                .is_some_and(|(_, pid)| !pid.is_empty() && pid.bytes().all(|b| b.is_ascii_digit()));
            if !is_temp {
                continue;
            }
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|when| when.elapsed().ok())
                .is_some_and(|age| age.as_secs() > 60);
            if old {
                let _ = fs::remove_file(&path);
            }
        }
    }
    walk(root, 0);
}

fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    let tmp = path.with_extension(format!(".tmp.{}", std::process::id()));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

pub fn render_library(doc: &Document) -> String {
    let lib = Library {
        version: 1,
        notebooks: doc.notebooks.clone(),
    };
    serde_json::to_string_pretty(&lib).unwrap_or_default()
}

/// A note file: a header between `---` lines, then the Markdown body.
///
/// ```text
/// ---
/// id: 7
/// title: "Groceries"
/// notebook: 1
/// pinned: false
/// starred: true
/// trashed: false
/// created: 2026-11-03T18:57:00+03:00
/// updated: 2026-11-03T21:40:12+03:00
/// ---
/// - [ ] Eggs
/// ```
pub fn render_note(n: &Note) -> String {
    let title = serde_json::to_string(&n.title).unwrap_or_else(|_| "\"\"".into());
    format!(
        "---\nid: {}\ntitle: {}\nnotebook: {}\nspace: {}\npinned: {}\nstarred: {}\ntrashed: {}\nlocked: {}\nalso: {}\ncreated: {}\nupdated: {}\n---\n{}",
        n.id,
        title,
        n.notebook_id,
        n.space,
        n.pinned,
        n.starred,
        n.trashed,
        n.locked,
        n.also_in
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(","),
        n.created.to_rfc3339(),
        n.updated.to_rfc3339(),
        n.body,
    )
}

pub fn parse_note(text: &str) -> Option<Note> {
    // Notes are meant to be opened in other apps. If one comes back with
    // Windows line endings, read it all the same — a note that fails to load
    // looks exactly like a deleted note to the sync engine.
    let normalised;
    let text = if text.contains('\r') {
        normalised = text.replace("\r\n", "\n").replace('\r', "\n");
        &normalised
    } else {
        text
    };
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    let (header, body) = (&rest[..end], &rest[end + 5..]);
    let mut fields = HashMap::new();
    for line in header.lines() {
        if let Some((k, v)) = line.split_once(": ") {
            fields.insert(k.trim(), v.trim());
        }
    }
    let flag = |k: &str| fields.get(k).is_some_and(|v| *v == "true");
    let time = |k: &str| {
        fields
            .get(k)
            .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
            .map(|t| t.with_timezone(&Local))
    };
    let updated = time("updated").unwrap_or_else(Local::now);
    Some(Note {
        id: fields.get("id")?.parse().ok()?,
        notebook_id: fields.get("notebook")?.parse().ok()?,
        space: fields
            .get("space")
            .map(|space| space.to_string())
            .filter(|space| !space.is_empty())
            .unwrap_or_else(|| crate::model::DEFAULT_SPACE.to_string()),
        title: fields
            .get("title")
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_default(),
        body: body.to_string(),
        pinned: flag("pinned"),
        starred: flag("starred"),
        trashed: flag("trashed"),
        locked: flag("locked"),
        also_in: fields
            .get("also")
            .map(|list| {
                list.split(',')
                    .filter_map(|id| id.trim().parse::<u64>().ok())
                    .collect()
            })
            .unwrap_or_default(),
        created: time("created").unwrap_or(updated),
        updated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ownnotes-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn note_round_trips_through_its_file() {
        let mut note = Note::new(42, 3, "Ameliyat: \"zor\" vaka — İnönü", "line one\n---\nnot the header end\n");
        note.pinned = true;
        note.trashed = true;
        let parsed = parse_note(&render_note(&note)).unwrap();
        assert_eq!(parsed.id, 42);
        assert_eq!(parsed.notebook_id, 3);
        assert_eq!(parsed.title, note.title);
        assert_eq!(parsed.body, note.body);
        assert!(parsed.pinned && parsed.trashed && !parsed.starred);
        assert_eq!(parsed.updated.timestamp(), note.updated.timestamp());
    }

    #[test]
    fn notes_live_in_a_folder_per_notebook() {
        let dir = temp_dir("folders");
        let (mut store, mut doc, _, problems) = Store::open_at(dir.clone());
        assert!(problems.is_empty(), "{problems:?}");
        let notebook = doc.notebooks[0].id;
        let folder = workspace_folder(doc.notebook_name(notebook));
        let id = doc.add_note(notebook, "In a folder", "body");
        store.save(&doc).unwrap();

        let first = dir.join("notes").join(&folder).join(note_file_name(id));
        assert!(first.exists(), "saved in its notebook's folder: {first:?}");

        // Renaming the notebook takes the notes with it.
        doc.rename_notebook(notebook, "Renamed");
        store.save(&doc).unwrap();
        let moved = dir.join("notes/Renamed").join(note_file_name(id));
        assert!(moved.exists(), "moved to the new folder: {moved:?}");
        assert!(!first.exists(), "the old copy is gone");

        // And they load back from wherever they sit.
        let (_, again, _, problems) = Store::open_at(dir.clone());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(again.note(id).unwrap().body, "body");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_note_in_two_notebooks_has_a_file_in_each() {
        let dir = temp_dir("copies");
        let (mut store, _, _, _) = Store::open_at(dir.clone());
        let work = 1;
        let ideas = 2;
        let mut doc = Document::from_parts(
            vec![
                Notebook { parent: None, id: work, name: "Work".into(), color: 0 },
                Notebook { parent: None, id: ideas, name: "Ideas".into(), color: 1 },
            ],
            Vec::new(),
        );
        let id = doc.add_note(work, "Both places", "body");
        doc.note_mut(id).unwrap().also_in = vec![ideas];
        store.save(&doc).unwrap();

        let first = dir
            .join("notes")
            .join(workspace_folder(doc.notebook_name(work)))
            .join(note_file_name(id));
        let second = dir
            .join("notes")
            .join(workspace_folder(doc.notebook_name(ideas)))
            .join(note_file_name(id));
        assert!(first.is_file(), "its own notebook has it");
        assert!(second.is_file(), "the other notebook has a copy too");

        // It still loads as one note, not two.
        let (_, again, _, _) = Store::open_at(dir.clone());
        assert_eq!(again.notes.iter().filter(|n| n.id == id).count(), 1);
        assert!(again.note(id).unwrap().also_in.contains(&ideas));

        // Counted once overall, once in each notebook it sits in.
        assert_eq!(again.notes.len(), 1, "All Notes counts it once");
        assert_eq!(again.count_in(work), 1);
        assert_eq!(again.count_in(ideas), 1);

        // Taking it out of the second notebook removes that copy.
        doc.note_mut(id).unwrap().also_in.clear();
        store.save(&doc).unwrap();
        assert!(first.is_file());
        assert!(!second.exists(), "the copy in Ideas is gone");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_copy_survives_whichever_file_is_read_first() {
        // "Alpha" sorts before "Zeta", so the copy is likely to be read first.
        let dir = temp_dir("copy-order");
        let (mut store, _, _, _) = Store::open_at(dir.clone());
        let mut doc = Document::from_parts(
            vec![
                Notebook { parent: None, id: 1, name: "Zeta".into(), color: 0 },
                Notebook { parent: None, id: 2, name: "Alpha".into(), color: 1 },
            ],
            Vec::new(),
        );
        let id = doc.add_note(1, "Both places", "body");
        doc.note_mut(id).unwrap().also_in = vec![2];
        store.save(&doc).unwrap();

        let copy = dir.join("notes").join("Alpha").join(note_file_name(id));
        let own = dir.join("notes").join("Zeta").join(note_file_name(id));

        // Reopen, then pretend the disk listed the copy first.
        let (mut store, mut doc, _, _) = Store::open_at(dir.clone());
        let own_rel = note_rel_path("Zeta", id, false);
        let copy_rel = note_rel_path("Alpha", id, false);
        let text = store.written.remove(&id).unwrap().1;
        store.copies.remove(&copy_rel);
        store.written.insert(id, (copy_rel.clone(), text.clone()));
        store.copies.insert(own_rel.clone(), text);
        store.settle_copies(&doc);
        assert_eq!(store.written[&id].0, own_rel, "the note's own file counts");

        // Edit and save, as the app does.
        doc.note_mut(id).unwrap().body.push_str(" more");
        store.save(&doc).unwrap();
        store.save(&doc).unwrap();
        assert!(own.is_file(), "its own file is there");
        assert!(copy.is_file(), "the copy in Alpha is still there");
        assert!(fs::read_to_string(&copy).unwrap().contains("body more"), "and it was updated");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_copy_stays_when_the_notes_own_file_is_missing() {
        // The note's own file was deleted outside the app; only the copy in
        // the other notebook is left. Saving must not take the copy for a
        // stale location and delete it.
        let dir = temp_dir("missing-own");
        let (mut store, _, _, _) = Store::open_at(dir.clone());
        let mut doc = Document::from_parts(
            vec![
                Notebook { parent: None, id: 1, name: "Zeta".into(), color: 0 },
                Notebook { parent: None, id: 2, name: "Alpha".into(), color: 1 },
            ],
            Vec::new(),
        );
        let id = doc.add_note(1, "Both places", "body");
        doc.note_mut(id).unwrap().also_in = vec![2];
        store.save(&doc).unwrap();

        let own = dir.join("notes/Zeta").join(note_file_name(id));
        let copy = dir.join("notes/Alpha").join(note_file_name(id));
        fs::remove_file(&own).unwrap();
        assert!(copy.is_file());

        let (mut store, mut doc, _, _) = Store::open_at(dir.clone());
        doc.note_mut(id).unwrap().body.push_str(" more");
        store.save(&doc).unwrap();
        store.save(&doc).unwrap();

        assert!(own.is_file(), "its own file is written again");
        assert!(copy.is_file(), "the copy is not mistaken for a stale file");
        assert!(fs::read_to_string(&copy).unwrap().contains("body more"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn versions_are_listed_and_kept_apart() {
        let dir = temp_dir("versions-html");
        let (mut store, _, _, _) = Store::open_at(dir.clone());
        let mut doc = Document::from_parts(
            vec![Notebook { parent: None, id: 1, name: "Work".into(), color: 0 }],
            Vec::new(),
        );
        let id = doc.add_note(1, "Plan", "one");
        store.save(&doc).unwrap();
        for text in ["two", "three", "four"] {
            doc.note_mut(id).unwrap().body = text.into();
            store.save(&doc).unwrap();
        }
        let versions = store.versions("Work", id);
        assert!(!versions.is_empty(), "the history lists what was saved");
        // Saves a moment apart share one version instead of one each.
        assert_eq!(versions.len(), 1, "{} versions", versions.len());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn renaming_a_notebook_by_letter_case_keeps_its_notes() {
        let dir = temp_dir("case-rename");
        let (mut store, _, _, _) = Store::open_at(dir.clone());
        let mut doc = Document::from_parts(
            vec![Notebook { parent: None, id: 1, name: "Work".into(), color: 0 }],
            Vec::new(),
        );
        let id = doc.add_note(1, "Plan", "body");
        store.save(&doc).unwrap();
        doc.rename_notebook(1, "work");
        store.save(&doc).unwrap();
        let (_, again, _, _) = Store::open_at(dir.clone());
        assert!(again.note(id).is_some(), "the note is still there after the rename");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_deleted_notebook_does_not_come_back() {
        let dir = temp_dir("deleted-notebook");
        let (mut store, _, _, _) = Store::open_at(dir.clone());
        let mut doc = Document::from_parts(
            vec![
                Notebook { parent: None, id: 1, name: "Work".into(), color: 0 },
                Notebook { parent: None, id: 2, name: "Old".into(), color: 1 },
            ],
            Vec::new(),
        );
        let id = doc.add_note(2, "Leftover", "body");
        store.save(&doc).unwrap();
        doc.delete_notebook(2);
        store.save(&doc).unwrap();
        let (_, again, _, _) = Store::open_at(dir.clone());
        let names: Vec<&str> = again.notebooks.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["Work"], "no \"Recovered\" notebook");
        assert!(again.note(id).unwrap().trashed, "its note is in the trash");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_sealed_markdown_note_stays_openable() {
        let dir = temp_dir("sealed-md");
        let folder = dir.join("notes/Work");
        fs::create_dir_all(&folder).unwrap();
        let mut note = Note::new(5, 1, "Secret", "");
        note.body = crate::crypto::encrypt("pw", "<p>hidden</p>");
        note.locked = true;
        fs::write(folder.join("note-5.md"), render_note(&note)).unwrap();
        fs::write(dir.join("library.json"), r#"{"version":1,"notebooks":[{"id":1,"name":"Work","color":0}]}"#).unwrap();
        let (_, doc, _, _) = Store::open_at(dir.clone());
        let body = &doc.note(5).unwrap().body;
        assert_eq!(crate::crypto::decrypt("pw", body).as_deref(), Some("<p>hidden</p>"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_note_written_with_windows_line_endings_still_loads() {
        let note = Note::new(7, 2, "From another editor", "one\ntwo");
        let windows = render_note(&note).replace('\n', "\r\n");
        let parsed = parse_note(&windows).expect("a CRLF note should load, not vanish");
        assert_eq!(parsed.id, 7);
        assert_eq!(parsed.title, "From another editor");
        assert_eq!(parsed.body, "one\ntwo");
    }

    #[test]
    fn save_then_reopen_keeps_changes_and_removes_deleted_notes() {
        let dir = temp_dir("reopen");
        let (mut store, mut doc, _, problems) = Store::open_at(dir.clone());
        assert!(problems.is_empty(), "{problems:?}");
        let first = doc.notes[0].id;
        let gone = doc.notes[1].id;
        doc.note_mut(first).unwrap().body = "edited".into();
        doc.notes.retain(|n| n.id != gone);
        doc.rename_notebook(doc.notebooks[0].id, "Renamed");
        store.save(&doc).unwrap();
        assert!(!dir.join(format!("notes/note-{gone}.md")).exists());

        let (_, again, _, problems) = Store::open_at(dir.clone());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(again.notes.len(), doc.notes.len());
        assert_eq!(again.note(first).unwrap().body, "edited");
        assert!(again.note(gone).is_none());
        assert_eq!(again.notebooks[0].name, "Renamed");
        // New ids never collide with saved ones.
        let mut again = again;
        let new_id = again.add_note(again.notebooks[0].id, "x", "");
        assert!(doc.notes.iter().all(|n| n.id != new_id));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn settings_round_trip_without_the_password() {
        let dir = temp_dir("settings");
        let (store, _, _, _) = Store::open_at(dir.clone());
        let mut saved = SavedSettings::default();
        saved.prefs.accent = 3;
        saved.sync.webdav_user = "me".into();
        saved.sync.webdav_password = "hunter2-webdav-pass".into();
        store.save_settings(&saved).unwrap();
        let text = fs::read_to_string(dir.join("settings.json")).unwrap();
        assert!(!text.contains("hunter2-webdav-pass"));
        let (_, _, loaded, _) = Store::open_at(dir.clone());
        assert_eq!(loaded.prefs.accent, 3);
        assert_eq!(loaded.sync.webdav_user, "me");
        assert!(loaded.sync.webdav_password.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_trashed_note_moves_into_the_deleted_folder() {
        let dir = temp_dir("trash-folder");
        let (mut store, mut doc, _, _) = Store::open_at(dir.clone());
        doc.notebooks = vec![Notebook { parent: None, id: 7, name: "Work".into(), color: 0 }];
        let id = doc.add_note(7, "A note", "body");
        store.save(&doc).unwrap();
        assert!(store.notes_dir().join(note_rel_path("Work", id, false)).exists());

        doc.note_mut(id).unwrap().trashed = true;
        store.save(&doc).unwrap();
        assert!(!store.notes_dir().join(note_rel_path("Work", id, false)).exists());
        assert!(store.notes_dir().join(note_rel_path("Work", id, true)).exists());

        // Read back, it knows it is in the trash whatever the header says.
        // Deleting it for good takes the empty folders with it.
        let (mut store, mut reloaded, _, _) = Store::open_at(dir.clone());
        reloaded.notes.retain(|n| n.id != id);
        store.save(&reloaded).unwrap();
        assert!(!store.notes_dir().join("Work/.deleted").exists());
        assert!(!store.notes_dir().join("Work").exists());
    }

    #[test]
    fn a_notebook_called_deleted_is_not_the_trash() {
        let dir = temp_dir("notebook-named-deleted");
        let (mut store, mut doc, _, _) = Store::open_at(dir.clone());
        doc.notebooks = vec![Notebook { parent: None, id: 9, name: "Deleted".into(), color: 0 }];
        let id = doc.add_note(9, "Still a note", "body");
        store.save(&doc).unwrap();

        let (_, reloaded, _, _) = Store::open_at(dir);
        let note = reloaded.notes.iter().find(|n| n.id == id).unwrap();
        assert!(!note.trashed, "a notebook that happens to be called Deleted is not the trash");
        assert_eq!(reloaded.notebooks[0].name, "Deleted");
    }

    #[test]
    fn a_dead_temporary_file_is_swept_at_startup() {
        let dir = temp_dir("temp-sweep");
        let (mut store, mut doc, _, _) = Store::open_at(dir.clone());
        doc.notebooks = vec![Notebook { parent: None, id: 2, name: "Work".into(), color: 0 }];
        doc.add_note(2, "A note", "body");
        store.save(&doc).unwrap();

        // Left behind by a write that never reached its rename.
        let folder = store.notes_dir().join("Work");
        let stale = folder.join("note-1.html.tmp.4242");
        fs::write(&stale, "<p>half a note</p>").unwrap();
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        let handle = fs::File::options().write(true).open(&stale).unwrap();
        handle.set_modified(long_ago).unwrap();

        // A file that merely mentions ".tmp." is a note, not a leftover.
        let decoy = folder.join("my.tmp.backup.html");
        fs::write(&decoy, "<p>keep me</p>").unwrap();

        Store::open_at(dir);
        assert!(!stale.exists(), "an abandoned temporary file is cleared on open");
        assert!(decoy.exists(), "a file named like a note is left alone");
    }

    #[test]
    fn versions_survive_a_notebook_rename_or_move() {
        let dir = temp_dir("versions-move");
        let (mut store, _, _, _) = Store::open_at(dir.clone());
        let mut doc = Document::from_parts(
            vec![
                Notebook { parent: None, id: 1, name: "Work".into(), color: 0 },
                Notebook { parent: None, id: 2, name: "Home".into(), color: 1 },
            ],
            Vec::new(),
        );
        let id = doc.add_note(1, "Plan", "one");
        store.save(&doc).unwrap();
        doc.note_mut(id).unwrap().body = "two".into();
        store.save(&doc).unwrap();
        // The 2-minute gap means the second save made no version: force one.
        let note = doc.note(id).unwrap().clone();
        store.snapshot_now("Work", &note).unwrap();
        assert!(!store.versions("Work", id).is_empty());

        // An old-build `.md` version file in the same folder counts too.
        let old_md = dir
            .join("versions")
            .join(workspace_folder("Work"))
            .join(format!("note-{id}-1000.md"));
        fs::create_dir_all(old_md.parent().unwrap()).unwrap();
        fs::write(&old_md, render_note(&note)).unwrap();

        // Move the note: its history must still be found.
        doc.note_mut(id).unwrap().notebook_id = 2;
        store.save(&doc).unwrap();
        let found = store.versions("Home", id);
        assert!(!found.is_empty(), "versions are found in every workspace folder");
        assert!(found.iter().any(|v| v.path == old_md), "old .md versions are listed");

        // Removing by real path gets the .md file too.
        for version in &found {
            store.remove_version_path(&version.path);
        }
        assert!(!old_md.exists());
        assert!(store.versions("Home", id).is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn reseal_versions_rekeys_history_with_a_new_passcode() {
        let dir = temp_dir("reseal-versions");
        let (mut store, _, _, _) = Store::open_at(dir.clone());
        let mut doc = Document::from_parts(
            vec![Notebook { parent: None, id: 1, name: "Work".into(), color: 0 }],
            Vec::new(),
        );
        let id = doc.add_note(1, "Secret", "plain");
        {
            let note = doc.note_mut(id).unwrap();
            note.body = crate::crypto::encrypt("old-pw", "plain");
            note.locked = true;
        }
        store.save(&doc).unwrap();
        let note = doc.note(id).unwrap().clone();
        store.snapshot_now("Work", &note).unwrap();

        let done = store.reseal_versions("old-pw", "new-pw");
        assert!(done >= 1, "at least the forced version was re-sealed");
        for version in store.versions("Work", id) {
            assert!(
                crate::crypto::decrypt("new-pw", &version.body).is_some(),
                "the new passcode opens the history"
            );
            assert!(
                crate::crypto::decrypt("old-pw", &version.body).is_none(),
                "the old passcode no longer does"
            );
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn attachment_temp_files_are_swept_too() {
        let dir = temp_dir("temp-sweep-attachments");
        let _ = Store::open_at(dir.clone());
        let folder = dir.join(crate::attachments::LOCAL_FOLDER).join("42");
        fs::create_dir_all(&folder).unwrap();
        let stale = folder.join(".file.pdf.tmp.4242");
        fs::write(&stale, "half").unwrap();
        let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
        let handle = fs::File::options().write(true).open(&stale).unwrap();
        handle.set_modified(long_ago).unwrap();

        Store::open_at(dir.clone());
        assert!(!stale.exists(), "abandoned attachment temp files are cleared on open");
        let _ = fs::remove_dir_all(dir);
    }
}
