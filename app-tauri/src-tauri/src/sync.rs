//! WebDAV sync for the Tauri shell: the job runs off the UI thread, the
//! changes come back as a report and are applied under the lock.

use ownnotes_core::cloud::engine::{self, LocalFile, Resolution};
use ownnotes_core::cloud::{secrets, Remote, WebDav};
use ownnotes_core::model::Notebook;
use ownnotes_core::store::{self, SyncState, LIBRARY_FILE, LOCKS_FILE};
use std::collections::HashMap;
use serde::Serialize;

use crate::extras;
use crate::App;

#[derive(Serialize, Default)]
pub struct Report {
    pub changed: usize,
    pub error: Option<String>,
    pub needs_decision: Option<Decision>,
    /// Another sync was already running; this one didn't start.
    pub busy: bool,
    /// Something changed here while the sync ran: sync again soon.
    pub again: bool,
    /// Downloads held back because they landed in a locked container and no
    /// passcode was entered this session; sealed at the next unlock.
    pub held: Vec<String>,
}

#[derive(Serialize, Clone)]
pub struct Decision {
    pub folder_notes: usize,
    pub here_notes: usize,
    pub shared: usize,
    /// This computer already syncs with the folder; asked again from Settings.
    pub paired: bool,
}

/// Everything the sync needs, lifted out of the app so the UI thread can keep
/// running while the network call happens.
pub struct Prepared {
    pub settings: ownnotes_core::sync::SyncSettings,
    pub local: Vec<LocalFile>,
    pub state: SyncState,
    pub source: String,
    /// Show the Merge / Keep local / Keep server question, even for a folder
    /// that already syncs.
    pub ask: bool,
    /// The question was just answered: sync with that answer, don't ask again.
    pub answered: bool,
    /// The data folder, for the attachments that sync alongside the notes.
    pub data_dir: std::path::PathBuf,
    /// Each note's notebook folder, where its attachments go on the server.
    pub folders: std::collections::HashMap<u64, String>,
}

impl App {
    pub fn prepare_sync(&self) -> Prepared {
        Prepared {
            settings: self.sync.clone(),
            local: self.local_files(),
            state: self.store.load_sync_state(&self.sync_base()),
            source: self.sync_base(),
            ask: false,
            answered: false,
            data_dir: self.store.dir().to_path_buf(),
            folders: self
                .doc
                .notes
                .iter()
                .map(|note| {
                    let name = self.doc.notebook_name(note.notebook_id);
                    (note.id, store::workspace_folder(name))
                })
                .collect(),
        }
    }

    pub fn sync_base(&self) -> String {
        WebDav::new(&self.sync.webdav_url, &self.sync.webdav_user, "x")
            .map(|remote| remote.base().to_string())
            .unwrap_or_else(|_| self.sync.webdav_url.clone())
    }

    pub fn local_files(&self) -> Vec<LocalFile> {
        // A note filed in several notebooks has a file in each of their
        // folders, and every one of them syncs.
        let mut files: Vec<LocalFile> = Vec::new();
        for note in &self.doc.notes {
            let content = store::render_note(note);
            for notebook in self.doc.notebooks_of(note) {
                files.push(LocalFile {
                    name: store::note_rel_path(self.doc.notebook_name(notebook), note.id, note.trashed),
                    content: content.clone(),
                });
            }
        }
        files.push(LocalFile {
            name: LIBRARY_FILE.to_string(),
            content: store::render_library(&self.doc),
        });
        // Which notebooks and spaces are locked syncs too (never app.json,
        // which holds the passcode verifier).
        let lists = extras::lock_lists(self.store.dir());
        files.push(LocalFile {
            name: LOCKS_FILE.to_string(),
            content: serde_json::to_string_pretty(&lists).unwrap_or_default(),
        });
        files.extend(
            self.store
                .version_files()
                .into_iter()
                .map(|(name, content)| LocalFile { name, content }),
        );
        files
    }

    /// Write whatever the sync brought back into the library.
    pub fn apply_sync(&mut self, mut outcome: engine::Outcome, _source: &str, before: &SyncState) -> Report {
        let mut report = Report::default();
        let resolution = outcome.resolution;

        for file in &outcome.changes.downloaded {
            if engine::is_version(&file.name) {
                self.store.write_version_file(&file.name, &file.content);
                report.changed += 1;
            } else if file.name == LOCKS_FILE {
                // Merge the lock lists, so a lock set on another device
                // applies here too.
                if let Ok(synced) = serde_json::from_str::<extras::LockLists>(&file.content) {
                    let dir = self.store.dir().to_path_buf();
                    let mut all = extras::load(&dir);
                    for id in synced.locked_notebooks {
                        if !all.locked_notebooks.contains(&id) {
                            all.locked_notebooks.push(id);
                        }
                    }
                    for space in synced.locked_spaces {
                        if !all.locked_spaces.contains(&space) {
                            all.locked_spaces.push(space);
                        }
                    }
                    if extras::save(&dir, &all).is_ok() {
                        let lists = extras::LockLists {
                            locked_notebooks: all.locked_notebooks.clone(),
                            locked_spaces: all.locked_spaces.clone(),
                        };
                        let _ = extras::save_lock_lists(&dir, &lists);
                        report.changed += 1;
                    }
                }
            } else if file.name == LIBRARY_FILE {
                if let Some(notebooks) = store::parse_library(&file.content) {
                    if resolution == Resolution::Remote {
                        self.doc.notebooks = notebooks;
                    } else {
                        let local = std::mem::take(&mut self.doc.notebooks);
                        let mut merged = notebooks;
                        for notebook in local {
                            if !merged.iter().any(|m| m.id == notebook.id) {
                                merged.push(notebook);
                            }
                        }
                        self.doc.notebooks = merged;
                    }
                    report.changed += 1;
                }
            } else if let Some(mut note) = store::parse_note(&file.content) {
                // A note arriving in a locked notebook or space must be
                // sealed, never stored as plain text. Without the session
                // passcode it can't be: hold the download back (the next
                // sync brings it again, and the next unlock seals it).
                if !note.locked
                    && !ownnotes_core::crypto::is_encrypted(&note.body)
                    && crate::in_locked_container(self, &note, None)
                {
                    match self.session_passcode.clone() {
                        Some(code) => {
                            let sealed = ownnotes_core::crypto::encrypt(&code, &note.body);
                            if sealed.is_empty() {
                                report.held.push(file.name.clone());
                                hold_back(&mut outcome.state, before, &file.name);
                                continue;
                            }
                            note.body = sealed;
                            note.locked = true;
                            self.unlocked.insert(note.id, code);
                        }
                        None => {
                            report.held.push(file.name.clone());
                            hold_back(&mut outcome.state, before, &file.name);
                            continue;
                        }
                    }
                }
                if !self.doc.has_notebook(note.notebook_id) {
                    self.doc.notebooks.push(Notebook {
                        parent: None,
                        id: note.notebook_id,
                        name: "Synced".to_string(),
                        color: self.doc.notebooks.len(),
                    });
                }
                match self.doc.note_mut(note.id) {
                    Some(existing) => *existing = note,
                    None => self.doc.notes.push(note),
                }
                report.changed += 1;
            }
        }

        // Notes that came down in this sync: a note moved to another notebook
        // (or a renamed notebook) shows up as "new path downloaded, old path
        // deleted", and must not be deleted.
        let arrived: std::collections::HashSet<u64> = outcome
            .changes
            .downloaded
            .iter()
            .filter(|file| !engine::is_version(&file.name))
            .filter_map(|file| engine::note_id(&file.name))
            .collect();
        for name in &outcome.changes.deleted {
            if engine::is_version(name) {
                self.store.remove_version_file(name);
                report.changed += 1;
            } else if let Some(id) = engine::note_id(name) {
                // It came down under another path in this same sync: it moved
                // (or its notebook was renamed) — it wasn't deleted. A local
                // note is only ever listed under its own path, so any other
                // deleted name really means the note is gone.
                // The note's file may simply have moved — into the trash
                // folder, to another notebook, or because its notebook was
                // renamed. It is only gone when its current path is the one
                // that vanished.
                let moved = self
                    .doc
                    .note(id)
                    .is_some_and(|note| {
                        store::note_rel_path(
                            self.doc.notebook_name(note.notebook_id),
                            note.id,
                            note.trashed,
                        ) != name.as_str()
                    });
                if arrived.contains(&id) || moved {
                    continue;
                }
                self.doc.notes.retain(|note| note.id != id);
                report.changed += 1;
            }
        }

        for file in &outcome.changes.conflicts {
            if let Some(version) = store::parse_note(&file.content) {
                let notebook = if self.doc.has_notebook(version.notebook_id) {
                    version.notebook_id
                } else {
                    match self.doc.notebooks.first() {
                        Some(first) => first.id,
                        None => continue,
                    }
                };
                let title = format!("{} (conflict copy)", version.display_title());
                let id = self.doc.add_note(notebook, &title, &version.body);
                // A locked note's copy is sealed text: it stays locked too.
                if let Some(copy) = self.doc.note_mut(id) {
                    copy.locked = version.locked;
                    copy.space = version.space.clone();
                }
                report.changed += 1;
            }
        }

        if report.changed > 0 {
            if let Err(error) = self.store.save(&self.doc) {
                report.error = Some(error.to_string());
            }
        }
        let _ = self.store.save_sync_state(&outcome.state);

        if let Some(error) = outcome.error {
            report.error = Some(error.message);
        }
        report
    }
}

/// The hash of every file as it was when a sync started, to spot edits made
/// while it ran.
pub fn snapshot(files: &[LocalFile]) -> HashMap<String, String> {
    files.iter().map(|f| (f.name.clone(), engine::hash(&f.content))).collect()
}

/// Pretend a download never happened: restore the pre-sync state entry, so
/// the next sync brings the file again.
fn hold_back(state: &mut SyncState, before: &SyncState, name: &str) {
    match before.files.get(name) {
        Some(old) => {
            state.files.insert(name.to_string(), old.clone());
        }
        None => {
            state.files.remove(name);
        }
    }
}

/// Leave out whatever this sync brought for a file that was edited while it
/// ran, so the edit isn't overwritten. The saved state forgets this sync's
/// view of that file too, so the next sync sees both sides changed and keeps
/// both versions. Returns how many files were left for next time.
pub fn keep_edits_made_meanwhile(
    outcome: &mut engine::Outcome,
    started: &HashMap<String, String>,
    now: &HashMap<String, String>,
    before: &SyncState,
) -> usize {
    let edited = |name: &str| started.get(name) != now.get(name);
    let mut skipped: Vec<String> = Vec::new();
    let changes = &mut outcome.changes;
    changes.downloaded.retain(|f| {
        let keep = !edited(&f.name);
        if !keep {
            skipped.push(f.name.clone());
        }
        keep
    });
    changes.deleted.retain(|name| {
        let keep = !edited(name);
        if !keep {
            skipped.push(name.clone());
        }
        keep
    });
    changes.conflicts.retain(|f| !skipped.contains(&f.name));
    for name in &skipped {
        match before.files.get(name) {
            Some(old) => {
                outcome.state.files.insert(name.clone(), old.clone());
            }
            None => {
                outcome.state.files.remove(name);
            }
        }
    }
    skipped.len()
}

/// What a run produced: either a sync to apply, or a question to ask first.
pub struct SyncRun {
    pub outcome: Option<engine::Outcome>,
    pub decision: Option<Decision>,
    pub error: Option<String>,
}

/// The blocking half: connect, compare, upload and download.
pub fn run(prepared: Prepared, resolution: Resolution) -> Result<SyncRun, String> {
    let password = secrets::load().unwrap_or_default();
    let mut remote = WebDav::new(
        &prepared.settings.webdav_url,
        &prepared.settings.webdav_user,
        &password,
    )
    .map_err(|e| e.message)?;

    // First contact with a folder that already holds notes: look, don't touch.
    // An answer given for a different server doesn't count for this one.
    let settings = &prepared.settings;
    let unanswered = settings.first_sync == ownnotes_core::sync::FirstSync::Ask
        || settings.first_sync_for != prepared.source;
    let first_contact =
        unanswered && prepared.state.files.is_empty() && !prepared.local.is_empty();
    if !prepared.answered && (prepared.ask || first_contact) {
        match remote.list() {
            Ok(files) => {
                let folder_notes = files
                    .iter()
                    .filter(|file| engine::note_id(&file.name).is_some())
                    .count();
                if folder_notes > 0 || prepared.ask {
                    let here_notes = prepared
                        .local
                        .iter()
                        .filter(|file| engine::note_id(&file.name).is_some())
                        .count();
                    let shared = files
                        .iter()
                        .filter(|file| {
                            engine::note_id(&file.name).is_some()
                                && prepared.local.iter().any(|local| local.name == file.name)
                        })
                        .count();
                    return Ok(SyncRun {
                        outcome: None,
                        decision: Some(Decision {
                            folder_notes,
                            here_notes,
                            shared,
                            paired: !prepared.state.files.is_empty(),
                        }),
                        error: None,
                    });
                }
            }
            Err(error) => {
                return Ok(SyncRun {
                    outcome: None,
                    decision: None,
                    error: Some(error.message),
                })
            }
        }
    }

    let before = prepared.state.clone();
    let outcome = engine::run(engine::Job {
        remote,
        state: prepared.state,
        local: prepared.local,
        resolution,
    });

    // Attachments go both ways after the notes, whatever was chosen: a file
    // is only ever added, never changed, so there is nothing to overwrite.
    //
    // The note list is brought up to date with what this sync did first:
    // notes that just arrived get their files now rather than next time, and
    // notes deleted on either side lose theirs.
    let note_ids = |state: &SyncState| -> std::collections::HashSet<u64> {
        state
            .files
            .keys()
            .filter(|name| !engine::is_version(name))
            .filter_map(|name| engine::note_id(name))
            .collect()
    };
    let mut deleted: std::collections::HashSet<u64> = note_ids(&before)
        .difference(&note_ids(&outcome.state))
        .copied()
        .collect();
    deleted.extend(
        outcome
            .changes
            .deleted
            .iter()
            .filter(|name| !engine::is_version(name))
            .filter_map(|name| engine::note_id(name)),
    );
    // A note that moved (new path downloaded or still known under another
    // path) wasn't deleted: its attachments stay.
    let still_there = note_ids(&outcome.state);
    let arrived: std::collections::HashSet<u64> = outcome
        .changes
        .downloaded
        .iter()
        .filter(|file| !engine::is_version(&file.name))
        .filter_map(|file| engine::note_id(&file.name))
        .collect();
    deleted.retain(|id| !still_there.contains(id) && !arrived.contains(id));
    let mut folders = prepared.folders.clone();
    for file in &outcome.changes.downloaded {
        if engine::is_version(&file.name) {
            continue;
        }
        if let (Some(id), Some((workspace, _))) = (engine::note_id(&file.name), file.name.split_once('/')) {
            folders.insert(id, workspace.to_string());
        }
    }
    for name in &outcome.changes.deleted {
        if let Some(id) = engine::note_id(name) {
            folders.remove(&id);
        }
    }

    let mut error = None;
    if outcome.error.is_none() {
        let attachments = WebDav::new(
            &prepared.settings.webdav_url,
            &prepared.settings.webdav_user,
            &password,
        )
        .and_then(|mut remote| {
            ownnotes_core::attachments::sync(&mut remote, &prepared.data_dir, &folders, &deleted)
        });
        if let Err(e) = attachments {
            error = Some(format!("Notes synced, but attachments didn't: {}", e.message));
        }
    }
    Ok(SyncRun {
        outcome: Some(outcome),
        decision: None,
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ownnotes_core::store::SyncedFile;

    fn file(name: &str, content: &str) -> LocalFile {
        LocalFile { name: name.into(), content: content.into() }
    }

    #[test]
    fn an_edit_made_during_a_sync_is_not_overwritten() {
        let started = snapshot(&[file("Work/note-1.html", "old"), file("Work/note-2.html", "two")]);
        // Note 1 was edited while the sync ran; note 2 wasn't.
        let now = snapshot(&[file("Work/note-1.html", "edited meanwhile"), file("Work/note-2.html", "two")]);
        let mut before = SyncState::default();
        before.files.insert(
            "Work/note-1.html".into(),
            SyncedFile { remote_id: "a".into(), rev: "r1".into(), hash: engine::hash("old") },
        );
        let mut outcome = engine::Outcome {
            state: SyncState::default(),
            changes: engine::Changes::default(),
            error: None,
            resolution: Resolution::Merge,
        };
        outcome.changes.downloaded = vec![file("Work/note-1.html", "from server"), file("Work/note-2.html", "server two")];
        outcome.changes.conflicts = vec![file("Work/note-1.html", "old")];
        outcome.state.files.insert(
            "Work/note-1.html".into(),
            SyncedFile { remote_id: "a".into(), rev: "r2".into(), hash: engine::hash("from server") },
        );

        let held = keep_edits_made_meanwhile(&mut outcome, &started, &now, &before);

        assert_eq!(held, 1);
        let names: Vec<_> = outcome.changes.downloaded.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["Work/note-2.html"], "only the untouched note is applied");
        assert!(outcome.changes.conflicts.is_empty());
        // The state goes back to what it was, so next time both sides look changed.
        assert_eq!(outcome.state.files["Work/note-1.html"].rev, "r1");
    }
}
