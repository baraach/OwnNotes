// The sync engine: compares this computer's files with the WebDAV folder and
// the state saved after the last sync, then uploads, downloads or deletes so
// both sides match.
//
// For each file name:
//   changed here only      → upload
//   changed there only     → download
//   changed on both sides  → the newer note wins; the other version is kept
//                            as a "conflict copy" so nothing is lost
//   deleted here           → delete there (unless it changed there: restore)
//   deleted there          → delete here (unless it changed here: re-upload)
//
// The engine never touches the app's notes directly. It gets a snapshot of
// the files and returns what the app should apply, so editing can carry on
// while a sync runs.

use std::collections::{BTreeSet, HashMap};

use sha2::{Digest, Sha256};

use super::{CloudError, CloudResult, Remote, RemoteFile, WebDav};
use crate::store::{parse_note, SyncState, SyncedFile, LIBRARY_FILE};

#[derive(Clone, Debug)]
pub struct LocalFile {
    pub name: String,
    pub content: String,
}

/// Which side a first sync should believe when the two disagree.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Resolution {
    /// Keep both: the newer note wins, the other becomes a conflict copy.
    Merge,
    /// This computer wins: upload it, drop the folder's extra notes.
    Local,
    /// The folder wins: download it, drop this computer's extra notes.
    Remote,
}

pub struct Job {
    pub remote: WebDav,
    pub state: SyncState,
    pub local: Vec<LocalFile>,
    pub resolution: Resolution,
}

/// What a sync did, for the app to apply.
#[derive(Default, Debug, Clone)]
pub struct Changes {
    /// Files to write here (new or newer in the cloud).
    pub downloaded: Vec<LocalFile>,
    /// Files to delete here (deleted in the cloud).
    pub deleted: Vec<String>,
    /// Versions that lost a conflict, to keep as separate notes.
    pub conflicts: Vec<LocalFile>,
    pub uploaded: usize,
    pub removed_remote: usize,
}

pub struct Outcome {
    pub state: SyncState,
    pub changes: Changes,
    pub error: Option<CloudError>,
    pub resolution: Resolution,
}

pub fn hash(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Only our own files take part; anything else in the folder is left alone.
/// A note is `note-<id>.md`, either in the folder's root (older versions) or
/// inside a notebook's folder. `locks.json` (which containers are locked)
/// syncs too; `app.json` (the passcode verifier) deliberately does not.
pub fn syncable(name: &str) -> bool {
    name == LIBRARY_FILE
        || name == crate::store::LOCKS_FILE
        || note_id(name).is_some()
        || is_version(name)
}

/// An old version: `Versions/<workspace>/note-<id>-<millis>.md`.
pub fn is_version(name: &str) -> bool {
    let mut parts = name.split('/');
    if parts.next() != Some(crate::store::VERSIONS_FOLDER) {
        return false;
    }
    let (Some(_workspace), Some(file), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let Some(rest) = file
        .strip_prefix("note-")
        .and_then(|r| r.strip_suffix(".html").or_else(|| r.strip_suffix(".md")))
    else {
        return false;
    };
    let Some((id, millis)) = rest.split_once('-') else {
        return false;
    };
    !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_digit())
        && !millis.is_empty()
        && millis.bytes().all(|b| b.is_ascii_digit())
}

/// The note id in a file name, wherever it sits.
pub fn note_id(name: &str) -> Option<u64> {
    let file = name.rsplit('/').next().unwrap_or(name);
    let id = file
        .strip_prefix("note-")?
        .strip_suffix(".html")
        .or_else(|| file.strip_prefix("note-")?.strip_suffix(".md"))?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    id.parse().ok()
}

/// Run one sync. Even on error, the state reflects the files that did move,
/// so the next run carries on from there.
pub fn run(job: Job) -> Outcome {
    let Job {
        mut remote,
        mut state,
        local,
        resolution,
    } = job;
    let mut changes = Changes::default();
    let error = sync_files(&mut remote, &mut state, &local, &mut changes, resolution).err();
    if error.is_none() {
        // A renamed workspace deletes its notes under the old name; take the
        // now-empty folder with them.
        remote.prune_empty_folders();
    }
    Outcome {
        state,
        changes,
        error,
        resolution,
    }
}

fn newer_in_cloud(local: &str, remote: &str) -> bool {
    match (parse_note(local), parse_note(remote)) {
        (Some(l), Some(r)) => r.updated > l.updated,
        (None, Some(_)) => true,
        _ => false,
    }
}

pub fn sync_files(
    remote: &mut dyn Remote,
    state: &mut SyncState,
    local: &[LocalFile],
    changes: &mut Changes,
    resolution: Resolution,
) -> CloudResult<()> {
    // Group the cloud files by name. A well-behaved WebDAV server has one
    // file per name, but a badly merged backup can leave duplicates; keep one
    // and fold the rest in so nothing is lost.
    let mut by_name: HashMap<String, Vec<RemoteFile>> = HashMap::new();
    for f in remote.list()? {
        if syncable(&f.name) {
            by_name.entry(f.name.clone()).or_default().push(f);
        }
    }
    let mut remote_files: HashMap<String, RemoteFile> = HashMap::new();
    for (name, mut files) in by_name {
        let keep = state
            .files
            .get(&name)
            .and_then(|s| files.iter().position(|f| f.id == s.remote_id))
            .unwrap_or(0);
        let primary = files.swap_remove(keep);
        for extra in files {
            let content = remote.download(&extra)?;
            if name == LIBRARY_FILE {
                changes.downloaded.push(LocalFile { name: name.clone(), content });
            } else {
                changes.conflicts.push(LocalFile { name: name.clone(), content });
            }
            remote.delete(&extra.id)?;
        }
        remote_files.insert(name, primary);
    }

    let local_files: HashMap<&str, (&str, String)> = local
        .iter()
        .filter(|f| syncable(&f.name))
        .map(|f| (f.name.as_str(), (f.content.as_str(), hash(&f.content))))
        .collect();

    let names: BTreeSet<String> = local_files
        .keys()
        .map(|n| n.to_string())
        .chain(remote_files.keys().cloned())
        .chain(state.files.keys().cloned())
        .collect();

    for name in names {
        let here = local_files.get(name.as_str());
        let there = remote_files.get(&name);
        let before = state.files.get(&name).cloned();

        match (here, there) {
            (Some((content, h)), None) => {
                let unchanged_here = before.as_ref().is_some_and(|b| &b.hash == h);
                if unchanged_here || resolution == Resolution::Remote {
                    // Deleted elsewhere, or this computer is not the source of truth.
                    changes.deleted.push(name.clone());
                    state.files.remove(&name);
                } else {
                    upload(remote, state, changes, &name, None, content, h)?;
                }
            }
            (None, Some(r)) => {
                let unchanged_there = before.as_ref().is_some_and(|b| b.rev == r.rev);
                if unchanged_there || resolution == Resolution::Local {
                    // Deleted here, or the folder is not the source of truth.
                    remote.delete(&r.id)?;
                    changes.removed_remote += 1;
                    state.files.remove(&name);
                } else {
                    download(remote, state, changes, &name, r)?;
                }
            }
            (None, None) => {
                state.files.remove(&name);
            }
            (Some((content, h)), Some(r)) => {
                let changed_here = before.as_ref().is_none_or(|b| &b.hash != h);
                let changed_there = before.as_ref().is_none_or(|b| b.rev != r.rev);
                match (changed_here, changed_there) {
                    (false, false) => {}
                    (true, false) => upload(remote, state, changes, &name, Some(&r.id), content, h)?,
                    (false, true) => download(remote, state, changes, &name, r)?,
                    (true, true) if name == LIBRARY_FILE => {
                        // Notebooks follow the same rule as the notes: an
                        // overwrite takes one side's list, a merge unions them.
                        if resolution == Resolution::Local {
                            upload(remote, state, changes, &name, Some(&r.id), content, h)?;
                        } else {
                            let theirs = remote.download(r)?;
                            let their_hash = hash(&theirs);
                            changes.downloaded.push(LocalFile {
                                name: name.clone(),
                                content: theirs,
                            });
                            state.files.insert(
                                name.clone(),
                                SyncedFile { remote_id: r.id.clone(), rev: r.rev.clone(), hash: their_hash },
                            );
                        }
                    }
                    (true, true) => match resolution {
                        Resolution::Local => {
                            upload(remote, state, changes, &name, Some(&r.id), content, h)?;
                        }
                        Resolution::Remote => {
                            download(remote, state, changes, &name, r)?;
                        }
                        Resolution::Merge => {
                            let theirs = remote.download(r)?;
                            let their_hash = hash(&theirs);
                            if &their_hash == h {
                                // Same content on both sides; just remember it.
                                state.files.insert(
                                    name.clone(),
                                    SyncedFile { remote_id: r.id.clone(), rev: r.rev.clone(), hash: their_hash },
                                );
                            } else if newer_in_cloud(content, &theirs) {
                                changes.conflicts.push(LocalFile { name: name.clone(), content: content.to_string() });
                                changes.downloaded.push(LocalFile { name: name.clone(), content: theirs });
                                state.files.insert(
                                    name.clone(),
                                    SyncedFile { remote_id: r.id.clone(), rev: r.rev.clone(), hash: their_hash },
                                );
                            } else {
                                changes.conflicts.push(LocalFile { name: name.clone(), content: theirs });
                                upload(remote, state, changes, &name, Some(&r.id), content, h)?;
                            }
                        }
                    },
                }
            }
        }
    }
    Ok(())
}

fn upload(
    remote: &mut dyn Remote,
    state: &mut SyncState,
    changes: &mut Changes,
    name: &str,
    existing: Option<&str>,
    content: &str,
    h: &str,
) -> CloudResult<()> {
    let f = remote.upload(name, existing, content)?;
    state.files.insert(
        name.to_string(),
        SyncedFile { remote_id: f.id, rev: f.rev, hash: h.to_string() },
    );
    changes.uploaded += 1;
    Ok(())
}

fn download(
    remote: &mut dyn Remote,
    state: &mut SyncState,
    changes: &mut Changes,
    name: &str,
    r: &RemoteFile,
) -> CloudResult<()> {
    let content = remote.download(r)?;
    state.files.insert(
        name.to_string(),
        SyncedFile { remote_id: r.id.clone(), rev: r.rev.clone(), hash: hash(&content) },
    );
    changes.downloaded.push(LocalFile { name: name.to_string(), content });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Note;
    use crate::store::render_note;

    /// An in-memory cloud folder.
    #[derive(Default)]
    struct FakeRemote {
        files: HashMap<String, (String, String)>, // id -> (name, content)
        next: u32,
    }

    impl FakeRemote {
        fn content_of(&self, name: &str) -> Option<String> {
            self.files.values().find(|(n, _)| n == name).map(|(_, c)| c.clone())
        }
        fn put(&mut self, name: &str, content: &str) {
            self.next += 1;
            self.files.insert(format!("id{}", self.next), (name.into(), content.into()));
        }
        fn replace(&mut self, name: &str, content: &str) {
            for (n, c) in self.files.values_mut() {
                if n == name {
                    *c = content.into();
                }
            }
        }
    }

    impl Remote for FakeRemote {
        fn list(&mut self) -> CloudResult<Vec<RemoteFile>> {
            Ok(self
                .files
                .iter()
                .map(|(id, (name, c))| RemoteFile { id: id.clone(), name: name.clone(), rev: hash(c) })
                .collect())
        }
        fn download(&mut self, f: &RemoteFile) -> CloudResult<String> {
            Ok(self.files[&f.id].1.clone())
        }
        fn upload(&mut self, name: &str, existing: Option<&str>, content: &str) -> CloudResult<RemoteFile> {
            let id = match existing {
                Some(id) => id.to_string(),
                None => {
                    self.next += 1;
                    format!("id{}", self.next)
                }
            };
            self.files.insert(id.clone(), (name.into(), content.into()));
            Ok(RemoteFile { id, name: name.into(), rev: hash(content) })
        }
        fn delete(&mut self, id: &str) -> CloudResult<()> {
            self.files.remove(id);
            Ok(())
        }
    }

    fn note(id: u64, body: &str, minutes_ago: i64) -> LocalFile {
        let mut n = Note::new(id, 1, "T", body);
        n.updated = chrono::Local::now() - chrono::Duration::minutes(minutes_ago);
        LocalFile { name: format!("note-{id}.md"), content: render_note(&n) }
    }

    fn sync(remote: &mut FakeRemote, state: &mut SyncState, local: &[LocalFile]) -> Changes {
        sync_with(remote, state, local, Resolution::Merge)
    }

    fn sync_with(
        remote: &mut FakeRemote,
        state: &mut SyncState,
        local: &[LocalFile],
        resolution: Resolution,
    ) -> Changes {
        let mut changes = Changes::default();
        sync_files(remote, state, local, &mut changes, resolution).unwrap();
        changes
    }

    #[test]
    fn first_sync_uploads_then_nothing_to_do() {
        let (mut remote, mut state) = (FakeRemote::default(), SyncState::default());
        let local = vec![note(1, "a", 0), note(2, "b", 0)];
        assert_eq!(sync(&mut remote, &mut state, &local).uploaded, 2);
        let again = sync(&mut remote, &mut state, &local);
        assert_eq!(again.uploaded + again.downloaded.len() + again.deleted.len(), 0);
    }

    #[test]
    fn edits_and_deletes_flow_both_ways() {
        let (mut remote, mut state) = (FakeRemote::default(), SyncState::default());
        let mut local = vec![note(1, "a", 5), note(2, "b", 5)];
        sync(&mut remote, &mut state, &local);

        // Edited here → uploaded.
        local[0] = note(1, "a edited", 0);
        assert_eq!(sync(&mut remote, &mut state, &local).uploaded, 1);
        assert!(remote.content_of("note-1.md").unwrap().ends_with("a edited"));

        // Edited in the cloud → downloaded.
        remote.replace("note-2.md", &note(2, "b from phone", 0).content);
        let c = sync(&mut remote, &mut state, &local);
        assert_eq!(c.downloaded.len(), 1);
        local[1] = c.downloaded[0].clone();

        // Created in the cloud → downloaded.
        remote.put("note-3.md", &note(3, "new elsewhere", 0).content);
        let c = sync(&mut remote, &mut state, &local);
        assert_eq!(c.downloaded[0].name, "note-3.md");
        local.push(c.downloaded[0].clone());

        // Deleted here → deleted there.
        local.retain(|f| f.name != "note-1.md");
        assert_eq!(sync(&mut remote, &mut state, &local).removed_remote, 1);
        assert!(remote.content_of("note-1.md").is_none());

        // Deleted there → deleted here.
        let id = remote.files.iter().find(|(_, (n, _))| n == "note-3.md").unwrap().0.clone();
        remote.files.remove(&id);
        assert_eq!(sync(&mut remote, &mut state, &local).deleted, vec!["note-3.md".to_string()]);
    }

    #[test]
    fn conflicts_keep_both_versions() {
        let (mut remote, mut state) = (FakeRemote::default(), SyncState::default());
        let local = vec![note(1, "original", 10)];
        sync(&mut remote, &mut state, &local);

        // Both sides edit; the cloud edit is newer.
        remote.replace("note-1.md", &note(1, "phone edit", 0).content);
        let mine = vec![note(1, "laptop edit", 5)];
        let c = sync(&mut remote, &mut state, &mine);
        assert!(c.downloaded[0].content.ends_with("phone edit"));
        assert!(c.conflicts[0].content.ends_with("laptop edit"));

        // Both sides edit again; this time the local edit is newer.
        remote.replace("note-1.md", &note(1, "phone again", 8).content);
        let mine = vec![note(1, "laptop again", 0)];
        let c = sync(&mut remote, &mut state, &mine);
        assert_eq!(c.uploaded, 1);
        assert!(c.conflicts[0].content.ends_with("phone again"));
        assert!(remote.content_of("note-1.md").unwrap().ends_with("laptop again"));
    }

    #[test]
    fn choosing_this_computer_overwrites_the_folder() {
        let (mut remote, mut state) = (FakeRemote::default(), SyncState::default());
        // The folder holds a note this computer doesn't have, and a newer
        // version of one it does.
        remote.put("note-9.md", &note(9, "only in the folder", 5).content);
        remote.put("note-1.md", &note(1, "folder's newer version", 0).content);
        let local = vec![note(1, "my version", 300), note(2, "only here", 5)];

        let changes = sync_with(&mut remote, &mut state, &local, Resolution::Local);

        // This computer wins even though the folder's copy is newer.
        assert!(remote.content_of("note-1.md").unwrap().ends_with("my version"));
        assert!(remote.content_of("note-2.md").is_some());
        assert!(remote.content_of("note-9.md").is_none(), "the folder's extra note is deleted");
        assert!(changes.conflicts.is_empty(), "overwriting keeps no second copy");
        assert_eq!(changes.removed_remote, 1);
    }

    #[test]
    fn choosing_the_folder_overwrites_this_computer() {
        let (mut remote, mut state) = (FakeRemote::default(), SyncState::default());
        // The folder has an older version of note 1, and a note 2 that this
        // computer doesn't have.
        remote.put("note-1.md", &note(1, "folder's older version", 300).content);
        remote.put("note-9.md", &note(9, "only in the folder", 5).content);
        let local = vec![note(1, "my newer version", 0), note(2, "only here", 5)];

        let changes = sync_with(&mut remote, &mut state, &local, Resolution::Remote);

        assert_eq!(changes.downloaded.len(), 2, "both folder notes come down");
        assert!(changes.downloaded.iter().any(|f| f.name == "note-1.md"
            && f.content.ends_with("folder's older version")));
        assert_eq!(changes.deleted, vec!["note-2.md".to_string()], "this computer's extra note goes");
        assert!(changes.conflicts.is_empty());
        // The folder was not touched beyond what we downloaded.
        assert!(remote.content_of("note-9.md").is_some());
    }

    #[test]
    fn the_notebook_list_follows_the_chosen_side() {
        let mine = r#"{"version":1,"notebooks":[{"id":2,"name":"Mine","color":1}]}"#;
        let theirs = r#"{"version":1,"notebooks":[{"id":1,"name":"Theirs","color":0}]}"#;
        let local = vec![LocalFile {
            name: LIBRARY_FILE.to_string(),
            content: mine.to_string(),
        }];

        // Keep local: our list replaces the folder's.
        let (mut remote, mut state) = (FakeRemote::default(), SyncState::default());
        remote.put(LIBRARY_FILE, theirs);
        let changes = sync_with(&mut remote, &mut state, &local, Resolution::Local);
        assert!(changes.downloaded.is_empty(), "nothing comes down over our list");
        assert!(remote.content_of(LIBRARY_FILE).unwrap().contains("Mine"));

        // Keep server: the folder's list comes down on its own.
        let (mut remote, mut state) = (FakeRemote::default(), SyncState::default());
        remote.put(LIBRARY_FILE, theirs);
        let changes = sync_with(&mut remote, &mut state, &local, Resolution::Remote);
        assert_eq!(changes.downloaded.len(), 1);
        assert!(changes.downloaded[0].content.contains("Theirs"));
        assert!(!changes.downloaded[0].content.contains("Mine"));

        // Merge: the app gets the folder's list and unions the two.
        let (mut remote, mut state) = (FakeRemote::default(), SyncState::default());
        remote.put(LIBRARY_FILE, theirs);
        let changes = sync_with(&mut remote, &mut state, &local, Resolution::Merge);
        assert_eq!(changes.downloaded.len(), 1);
        assert!(changes.downloaded[0].content.contains("Theirs"));
    }

    #[test]
    fn other_files_in_the_folder_are_left_alone() {
        let (mut remote, mut state) = (FakeRemote::default(), SyncState::default());
        remote.put("holiday.jpg", "binary");
        let c = sync(&mut remote, &mut state, &[]);
        assert!(c.downloaded.is_empty());
        assert!(remote.content_of("holiday.jpg").is_some());
    }
}
