//! The Tauri shell: the Rust core, exposed to the web UI.

mod extras;
mod importer;
mod sync;

use std::sync::Mutex;

use ownnotes_core::attachments;
use ownnotes_core::model::Document;
use ownnotes_core::cloud::engine::Resolution;
use ownnotes_core::cloud::secrets;
use ownnotes_core::prefs::Prefs;
use ownnotes_core::store::{SavedSettings, Store};
use ownnotes_core::sync::SyncSettings;
use serde::Serialize;
use tauri::State;

struct App {
    store: Store,
    doc: Document,
    prefs: Prefs,
    sync: SyncSettings,
    /// Notes unlocked for this session, and the passcode that opened them, so
    /// what is written back can be sealed again.
    unlocked: std::collections::HashMap<u64, String>,
    /// The passcode, once it has been entered this session: notes added to a
    /// locked notebook or space are sealed with it.
    session_passcode: Option<String>,
}

#[derive(Serialize)]
struct NoteDto {
    id: u64,
    notebook_id: u64,
    notebooks: Vec<u64>,
    space: String,
    title: String,
    body: String,
    preview: String,
    pinned: bool,
    starred: bool,
    trashed: bool,
    locked: bool,
    sealed: bool,
    /// Milliseconds since 1970, for sorting by date.
    updated_ms: i64,
    created_ms: i64,
    updated: String,
    created: String,
    todos: usize,
}

#[derive(Serialize)]
struct NotebookDto {
    id: u64,
    name: String,
    color: usize,
    count: usize,
    locked: bool,
    depth: usize,
    parent: Option<u64>,
}

#[derive(Serialize)]
struct Snapshot {
    notes: Vec<NoteDto>,
    spaces: Vec<String>,
    notebooks: Vec<NotebookDto>,
    prefs: Prefs,
    sync_url: String,
    sync_user: String,
    sync_connected: bool,
    problems: Vec<String>,
}

/// A note's body for the interface: plain when it is open in this session,
/// empty when it is sealed.
fn open_body(app: &App, note: &ownnotes_core::model::Note) -> String {
    if !note.locked {
        return note.body.clone();
    }
    match app.unlocked.get(&note.id) {
        Some(passcode) if ownnotes_core::crypto::is_encrypted(&note.body) => {
            ownnotes_core::crypto::decrypt(passcode, &note.body).unwrap_or_default()
        }
        Some(_) => note.body.clone(),
        None => String::new(),
    }
}

/// Every note inside a notebook (with its children) or a space.
fn notes_in_container(app: &App, kind: &str, key: &str) -> Vec<u64> {
    if kind == "space" {
        return app
            .doc
            .notes
            .iter()
            .filter(|note| note.space == key)
            .map(|note| note.id)
            .collect();
    }
    let Ok(root) = key.parse::<u64>() else { return Vec::new() };
    let mut family = vec![root];
    let mut index = 0;
    while index < family.len() {
        let current = family[index];
        for book in &app.doc.notebooks {
            if book.parent == Some(current) && !family.contains(&book.id) {
                family.push(book.id);
            }
        }
        index += 1;
    }
    app.doc
        .notes
        .iter()
        .filter(|note| {
            family.contains(&note.notebook_id) || note.also_in.iter().any(|id| family.contains(id))
        })
        .map(|note| note.id)
        .collect()
}

/// Whether a note sits in a locked notebook (or inside one) or a locked space.
fn in_locked_container(app: &App, note: &ownnotes_core::model::Note, skip: Option<(&str, &str)>) -> bool {
    let all = extras::load(app.store.dir());
    if all.locked_spaces.iter().any(|space| *space == note.space && skip != Some(("space", space.as_str()))) {
        return true;
    }
    for book in app.doc.notebooks_of(note) {
        // The notebook itself and every notebook above it.
        let mut current = Some(book);
        let mut seen = 0;
        while let Some(id) = current {
            let key = id.to_string();
            if all.locked_notebooks.contains(&id) && skip != Some(("notebook", key.as_str())) {
                return true;
            }
            current = app.doc.notebooks.iter().find(|n| n.id == id).and_then(|n| n.parent);
            seen += 1;
            if seen > 64 {
                break;
            }
        }
    }
    false
}

/// A note that ended up in a locked notebook or space (made there, moved,
/// added or imported) is sealed like the rest, with the passcode entered this
/// session. Without one it can't be, so the caller refuses the change.
fn seal_if_in_locked_container(app: &mut App, id: u64) -> Result<(), String> {
    let Some(note) = app.doc.note(id) else { return Ok(()) };
    if note.locked || !in_locked_container(app, note, None) {
        return Ok(());
    }
    let code = app
        .session_passcode
        .clone()
        .ok_or("That notebook or space is locked — unlock it first")?;
    let sealed = ownnotes_core::crypto::encrypt(&code, &note.body);
    if sealed.is_empty() {
        return Err("The note couldn't be encrypted".into());
    }
    if let Some(note) = app.doc.note_mut(id) {
        note.body = sealed;
        note.locked = true;
    }
    app.unlocked.insert(id, code);
    Ok(())
}

/// A file name in a folder that isn't taken yet: "name.pdf", "name (2).pdf"…
fn unique_path(dir: &std::path::Path, stem: &str, extension: &str) -> std::path::PathBuf {
    let mut path = dir.join(format!("{stem}.{extension}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).{extension}"));
        n += 1;
    }
    path
}

/// Exported and printed pages are opened in a browser: no scripts there.
const NO_SCRIPTS: &str = "<meta http-equiv=\"Content-Security-Policy\" content=\"script-src 'none'; object-src 'none'\">";

fn doc_notes(app: &App) -> Vec<String> {
    app.doc.notes.iter().map(|note| note.space.clone()).collect()
}

fn snapshot(app: &App, problems: Vec<String>) -> Snapshot {
    let locked_books = extras::load(app.store.dir()).locked_notebooks;
    let locked_books = &locked_books;
    let notes = app
        .doc
        .notes
        .iter()
        .map(|note| NoteDto {
            id: note.id,
            notebook_id: note.notebook_id,
            notebooks: app.doc.notebooks_of(note),
            space: note.space.clone(),
            title: note.title.clone(),
            body: open_body(app, note),
            preview: if note.locked {
                String::new()
            } else {
                note.preview()
            },
            pinned: note.pinned,
            starred: note.starred,
            trashed: note.trashed,
            // Locked for the session means locked for the interface too.
            locked: note.locked && !app.unlocked.contains_key(&note.id),
            // Stored encrypted, whether or not it is open right now.
            sealed: note.locked,
            updated_ms: note.updated.timestamp_millis(),
            created_ms: note.created.timestamp_millis(),
            updated: note.updated_label(),
            created: note.created.format("%b %-d, %Y").to_string(),
            todos: note.open_todos(),
        })
        .collect();

    let notebooks = app
        .doc
        .notebooks
        .iter()
        .map(|book| NotebookDto {
            id: book.id,
            name: book.name.clone(),
            color: book.color,
            count: app.doc.count_in(book.id),
            locked: locked_books.contains(&book.id),
            depth: app.doc.notebook_depth(book.id),
            parent: book.parent,
        })
        .collect();

    let mut spaces: Vec<String> = Vec::new();
    for name in doc_notes(app) {
        if !spaces.contains(&name) {
            spaces.push(name);
        }
    }
    spaces.sort();

    Snapshot {
        spaces,
        notes,
        notebooks,
        prefs: app.prefs.clone(),
        sync_url: app.sync.webdav_url.clone(),
        sync_user: app.sync.webdav_user.clone(),
        sync_connected: app.sync.active(),
        problems,
    }
}

#[tauri::command]
fn load(state: State<'_, Mutex<App>>) -> Result<Snapshot, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    Ok(snapshot(&app, Vec::new()))
}

#[tauri::command]
fn save_note(
    state: State<'_, Mutex<App>>,
    id: u64,
    title: String,
    body: String,
) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let app = &mut *guard;
    let passcode = app.unlocked.get(&id).cloned();
    if let Some(note) = app.doc.note_mut(id) {
        let body = match (&passcode, note.locked) {
            // Never write plain text into a locked note.
            (Some(code), true) if !ownnotes_core::crypto::is_encrypted(&body) => {
                // Sealed elsewhere with another passcode (another device):
                // what the editor shows isn't this text, so don't replace it.
                if ownnotes_core::crypto::is_encrypted(&note.body)
                    && ownnotes_core::crypto::decrypt(code, &note.body).is_none()
                {
                    return Err("This note was locked with another passcode; it can't be edited here".into());
                }
                let sealed = ownnotes_core::crypto::encrypt(code, &body);
                if sealed.is_empty() {
                    return Err("The note couldn't be encrypted; nothing was saved".into());
                }
                sealed
            }
            // Locked and not opened: whatever the interface sent is not the
            // note's text (it never saw it), so the sealed text is kept.
            (None, true) if !ownnotes_core::crypto::is_encrypted(&body) => {
                return Err("This note is locked — unlock it before editing".into());
            }
            _ => body,
        };
        note.title = title;
        note.body = body;
        note.updated = chrono::Local::now();
    }
    let App { store, doc, .. } = app;
    store.save(doc).map_err(|e| e.to_string())
}

#[tauri::command]
fn create_note(
    state: State<'_, Mutex<App>>,
    notebook_id: u64,
    space: Option<String>,
) -> Result<u64, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let app = &mut *guard;
    let notebook = if app.doc.has_notebook(notebook_id) {
        notebook_id
    } else {
        app.doc.notebooks.first().map(|n| n.id).unwrap_or_default()
    };
    let id = app.doc.add_note(notebook, "", "");
    // New notes go into the space that is open, not always the default one.
    if let Some(space) = space.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        if let Some(note) = app.doc.note_mut(id) {
            note.space = space;
        }
    }
    if let Err(error) = seal_if_in_locked_container(app, id) {
        app.doc.notes.retain(|note| note.id != id);
        return Err(error);
    }
    let App { store, doc, .. } = app;
    store.save(doc).map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
fn create_notebook(
    state: State<'_, Mutex<App>>,
    name: String,
    parent: Option<u64>,
) -> Result<u64, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let app = &mut *guard;
    let id = match parent.filter(|id| app.doc.has_notebook(*id)) {
        Some(parent) => app.doc.add_sub_notebook(&name, parent),
        None => app.doc.add_notebook(&name),
    };
    let App { store, doc, .. } = app;
    store.save(doc).map_err(|e| e.to_string())?;
    Ok(id)
}

#[tauri::command]
fn set_flags(
    state: State<'_, Mutex<App>>,
    id: u64,
    pinned: Option<bool>,
    starred: Option<bool>,
    trashed: Option<bool>,
) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let app = &mut *guard;
    if let Some(note) = app.doc.note_mut(id) {
        if let Some(value) = pinned {
            note.pinned = value;
        }
        if let Some(value) = starred {
            note.starred = value;
        }
        if let Some(value) = trashed {
            note.trashed = value;
        }
        note.updated = chrono::Local::now();
    }
    let App { store, doc, .. } = app;
    store.save(doc).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_note(state: State<'_, Mutex<App>>, id: u64) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let app = &mut *guard;
    app.doc.notes.retain(|note| note.id != id);
    let App { store, doc, .. } = app;
    store.save(doc).map_err(|e| e.to_string())?;
    // Its files go too; the server copy is removed on the next sync.
    attachments::remove_local(store.dir(), id);
    Ok(())
}

/// Delete everything in the trash for good: notes, their files and versions.
#[tauri::command]
fn empty_trash(state: State<'_, Mutex<App>>, ids: Vec<u64>) -> Result<usize, String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let app = &mut *guard;
    // Only notes that are in the trash can be deleted for good, whatever
    // ids are asked for.
    let doomed: Vec<u64> = app
        .doc
        .notes
        .iter()
        .filter(|note| note.trashed && (ids.is_empty() || ids.contains(&note.id)))
        .map(|note| note.id)
        .collect();
    if doomed.is_empty() {
        return Ok(0);
    }
    for id in &doomed {
        attachments::remove_local(app.store.dir(), *id);
    }
    app.doc.notes.retain(|note| !doomed.contains(&note.id));
    let App { store, doc, .. } = app;
    store.save(doc).map_err(|e| e.to_string())?;
    Ok(doomed.len())
}

#[tauri::command]
fn save_prefs(state: State<'_, Mutex<App>>, prefs: Prefs) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let app = &mut *guard;
    app.prefs = prefs;
    let settings = SavedSettings {
        prefs: app.prefs.clone(),
        sync: app.sync.clone(),
    };
    let App { store, .. } = app;
    store.save_settings(&settings).map_err(|e| e.to_string())
}

#[tauri::command]
async fn sync_now(
    state: State<'_, Mutex<App>>,
    resolution: Option<String>,
    ask: Option<bool>,
) -> Result<sync::Report, String> {
    let (resolution, answered) = match resolution.as_deref() {
        Some("remote") => (Resolution::Remote, Some(ownnotes_core::sync::FirstSync::KeepServer)),
        Some("local") => (Resolution::Local, Some(ownnotes_core::sync::FirstSync::KeepLocal)),
        Some("merge") => (Resolution::Merge, Some(ownnotes_core::sync::FirstSync::Merge)),
        _ => (Resolution::Merge, None),
    };

    let mut prepared = {
        let app = state.lock().map_err(|e| e.to_string())?;
        if !app.sync.active() {
            return Err("No WebDAV folder is set up yet".into());
        }
        app.prepare_sync()
    };
    prepared.ask = ask.unwrap_or(false);
    if answered.is_some() {
        // An answer means starting over with this folder: forget what was
        // synced before, so Keep local / Keep server replace everything and
        // not just the notes that changed.
        prepared.answered = true;
        prepared.state = ownnotes_core::store::SyncState {
            source: prepared.source.clone(),
            ..Default::default()
        };
    }

    // One sync at a time: a timer, an edit and a button press can all ask.
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Ok(sync::Report {
            changed: 0,
            error: None,
            needs_decision: None,
            busy: true,
            again: false,
        });
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let _done = Done;

    let started = sync::snapshot(&prepared.local);
    let before = prepared.state.clone();
    let run = tauri::async_runtime::spawn_blocking(move || sync::run(prepared, resolution))
        .await
        .map_err(|e| e.to_string())??;

    if let Some(decision) = run.decision {
        return Ok(sync::Report {
            changed: 0,
            error: None,
            needs_decision: Some(decision),
            busy: false,
            again: false,
        });
    }
    let mut outcome = match (run.outcome, run.error.clone()) {
        (Some(outcome), _) => outcome,
        (None, Some(error)) => {
            return Ok(sync::Report {
                changed: 0,
                error: Some(error),
                needs_decision: None,
                busy: false,
                again: false,
            })
        }
        (None, None) => return Err("Nothing to apply".into()),
    };

    let mut app = state.lock().map_err(|e| e.to_string())?;
    let source = app.sync_base();
    let now = sync::snapshot(&app.local_files());
    let held_back = sync::keep_edits_made_meanwhile(&mut outcome, &started, &now, &before);

    // Remember the answer once, so the question is only asked on first contact.
    if let Some(choice) = answered {
        app.sync.first_sync = choice;
        app.sync.first_sync_for = source.clone();
    }
    let clean = outcome.error.is_none();
    let mut report = app.apply_sync(outcome, &source);
    // A problem after the notes synced (attachments) is reported, but the
    // notes themselves were applied above.
    if report.error.is_none() {
        report.error = run.error;
    }
    report.again = held_back > 0;

    if clean && app.sync.first_sync != ownnotes_core::sync::FirstSync::Merge {
        app.sync.first_sync = ownnotes_core::sync::FirstSync::Merge;
    }
    if answered.is_some() || clean {
        let settings = SavedSettings {
            prefs: app.prefs.clone(),
            sync: app.sync.clone(),
        };
        let App { store, .. } = &mut *app;
        let _ = store.save_settings(&settings);
    }
    Ok(report)
}

#[derive(serde::Deserialize)]
struct SyncForm {
    url: String,
    user: String,
    password: String,
    backend_on: bool,
}

#[tauri::command]
fn save_sync(state: State<'_, Mutex<App>>, form: SyncForm) -> Result<(), String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    app.sync.webdav_url = form.url.trim().to_string();
    app.sync.webdav_user = form.user.trim().to_string();
    app.sync.backend = if form.backend_on {
        ownnotes_core::sync::SyncBackend::WebDav
    } else {
        ownnotes_core::sync::SyncBackend::Off
    };
    if !form.backend_on {
        // Turning sync back on asks Merge / Keep local / Keep server again
        // instead of quietly reusing the old answer, and anything edited in
        // the folder meanwhile isn't mistaken for a deletion.
        app.sync.first_sync = ownnotes_core::sync::FirstSync::Ask;
        app.sync.first_sync_for.clear();
        app.store.forget_sync_state();
    }
    if !form.password.is_empty() {
        secrets::save(&form.password)?;
    }
    let settings = SavedSettings {
        prefs: app.prefs.clone(),
        sync: app.sync.clone(),
    };
    let App { store, .. } = &mut *app;
    store.save_settings(&settings).map_err(|e| e.to_string())
}

/// Check an address and login without saving them or moving any notes. An
/// empty password means the one already in the keychain.
#[tauri::command]
async fn test_webdav(form: SyncForm) -> Result<(), String> {
    let password = if form.password.is_empty() {
        secrets::load().unwrap_or_default()
    } else {
        form.password.clone()
    };
    if form.url.trim().is_empty() || form.user.trim().is_empty() {
        return Err("Fill in the server address and username first.".into());
    }
    if password.is_empty() {
        return Err("Enter the password for this server.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut remote = ownnotes_core::cloud::WebDav::new(form.url.trim(), form.user.trim(), &password)
            .map_err(|e| e.message)?;
        remote.check().map_err(|e| e.message)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize, serde::Deserialize)]
struct SyncOptions {
    /// Sync a few seconds after a note changes.
    sync_on_change: bool,
    /// Check for changes from other devices every this many minutes; 0 = no.
    auto_sync_minutes: u32,
}

#[tauri::command]
fn sync_options(state: State<'_, Mutex<App>>) -> Result<SyncOptions, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    Ok(SyncOptions {
        sync_on_change: app.sync.sync_on_change,
        auto_sync_minutes: app.sync.auto_sync_minutes,
    })
}

#[tauri::command]
fn save_sync_options(state: State<'_, Mutex<App>>, options: SyncOptions) -> Result<(), String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    app.sync.sync_on_change = options.sync_on_change;
    app.sync.auto_sync_minutes = options.auto_sync_minutes.min(24 * 60);
    let settings = SavedSettings {
        prefs: app.prefs.clone(),
        sync: app.sync.clone(),
    };
    let App { store, .. } = &mut *app;
    store.save_settings(&settings).map_err(|e| e.to_string())
}

#[tauri::command]
fn sync_status(state: State<'_, Mutex<App>>) -> Result<(String, String, bool), String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    Ok((
        app.sync.webdav_url.clone(),
        app.sync.webdav_user.clone(),
        app.sync.active(),
    ))
}

#[tauri::command]
fn versions(
    state: State<'_, Mutex<App>>,
    id: u64,
) -> Result<Vec<VersionDto>, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let notebook = app
        .doc
        .note(id)
        .map(|note| app.doc.notebook_name(note.notebook_id).to_string())
        .unwrap_or_default();
    Ok(app
        .store
        .versions(&notebook, id)
        .into_iter()
        .map(|version| VersionDto {
            label: version.at.format("%b %-d, %-I:%M %p").to_string(),
            at: version.at.to_rfc3339(),
            body: version.body,
        })
        .collect())
}

#[derive(Serialize)]
struct VersionDto {
    label: String,
    at: String,
    body: String,
}

#[tauri::command]
fn restore_version(
    state: State<'_, Mutex<App>>,
    id: u64,
    at: String,
) -> Result<(), String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let notebook = app
        .doc
        .note(id)
        .map(|note| app.doc.notebook_name(note.notebook_id).to_string())
        .unwrap_or_default();
    let Some(version) = app
        .store
        .versions(&notebook, id)
        .into_iter()
        .find(|version| version.at.to_rfc3339() == at)
    else {
        return Err("That version is gone".into());
    };
    let passcode = app.unlocked.get(&id).cloned();
    if let Some(note) = app.doc.note_mut(id) {
        note.body = if note.locked {
            // The old version may be readable (from before the lock) or
            // sealed; either way the note stays sealed with the passcode.
            let code = passcode.ok_or("Unlock the note before restoring a version")?;
            let plain = if ownnotes_core::crypto::is_encrypted(&version.body) {
                ownnotes_core::crypto::decrypt(&code, &version.body)
                    .ok_or("That version was sealed with another passcode")?
            } else {
                version.body
            };
            ownnotes_core::crypto::encrypt(&code, &plain)
        } else {
            version.body
        };
        note.updated = chrono::Local::now();
    }
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())
}

#[tauri::command]
fn rename_notebook(state: State<'_, Mutex<App>>, id: u64, name: String) -> Result<(), String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    app.doc.rename_notebook(id, &name);
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())
}

#[tauri::command]
fn colour_notebook(state: State<'_, Mutex<App>>, id: u64, colour: usize) -> Result<(), String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    app.doc.set_notebook_color(id, colour);
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())
}

#[tauri::command]
fn remove_notebook(state: State<'_, Mutex<App>>, id: u64) -> Result<(), String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    if app.doc.notebooks.len() <= 1 {
        return Err("Keep at least one notebook".into());
    }
    app.doc.delete_notebook(id);
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())
}

/// What is locked: notebook ids and space names.
#[tauri::command]
fn locks(state: State<'_, Mutex<App>>) -> Result<serde_json::Value, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let all = extras::load(app.store.dir());
    let notebooks = all.locked_notebooks;
    let spaces = all.locked_spaces;
    Ok(serde_json::json!({ "notebooks": notebooks, "spaces": spaces }))
}

/// Lock or unlock a notebook (`kind: "notebook"`, key = id) or a space
/// (`kind: "space"`, key = its name).
#[tauri::command]
fn set_container_lock(
    state: State<'_, Mutex<App>>,
    kind: String,
    key: String,
    locked: bool,
    passcode: Option<String>,
) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let app = &mut *guard;
    let dir = app.store.dir().to_path_buf();
    if kind != "notebook" && kind != "space" {
        return Err("Unknown kind of lock".into());
    }

    let stored = extras::load(&dir).lock;
    let code = passcode.unwrap_or_default();

    // Every note inside is sealed with the passcode, so a passcode must exist
    // and the one given must be it.
    if locked {
        let Some(hash) = stored else {
            return Err("Set an app passcode in Settings first — notes are sealed with it".into());
        };
        if extras::hash_passcode(&code) != hash {
            return Err("That passcode doesn't match".into());
        }
    } else if let Some(hash) = stored {
        if !code.is_empty() && extras::hash_passcode(&code) != hash {
            return Err("That passcode doesn't match".into());
        }
    }

    let ids = notes_in_container(app, &kind, &key);
    if locked {
        // Seal every note first; only if all of them worked is anything
        // changed, so a failure leaves no half-locked notes behind.
        let mut sealed = Vec::new();
        for id in &ids {
            let Some(note) = app.doc.note(*id) else { continue };
            let body = if ownnotes_core::crypto::is_encrypted(&note.body) {
                note.body.clone()
            } else {
                ownnotes_core::crypto::encrypt(&code, &note.body)
            };
            if body.is_empty() {
                return Err("Some notes could not be sealed; nothing was locked".into());
            }
            sealed.push((*id, body));
        }
        for (id, body) in sealed {
            if let Some(note) = app.doc.note_mut(id) {
                note.body = body;
                note.locked = true;
            }
            app.unlocked.remove(&id);
        }
        app.session_passcode = Some(code.clone());
        let App { store, doc, .. } = app;
        store.save(doc).map_err(|e| e.to_string())?;
        for id in &ids {
            store.forget_versions(*id);
        }
    } else {
        // Removing the lock: every note in it is opened for good. All of
        // them are opened first, so a wrong passcode changes nothing.
        let Some(hash) = extras::load(&dir).lock else {
            return Err("There is no app passcode to open these notes with".into());
        };
        if extras::hash_passcode(&code) != hash {
            return Err("That passcode doesn't match".into());
        }
        let mut opened = Vec::new();
        for id in &ids {
            let Some(note) = app.doc.note(*id) else { continue };
            // Still inside another locked notebook or space: it stays sealed
            // (open for this session only).
            if in_locked_container(app, note, Some((kind.as_str(), key.as_str()))) {
                continue;
            }
            if !ownnotes_core::crypto::is_encrypted(&note.body) {
                opened.push((*id, note.body.clone()));
                continue;
            }
            match ownnotes_core::crypto::decrypt(&code, &note.body) {
                Some(plain) => opened.push((*id, plain)),
                None => return Err("That passcode doesn't open one of the notes; nothing changed".into()),
            }
        }
        for (id, plain) in opened {
            if let Some(note) = app.doc.note_mut(id) {
                note.body = plain;
                note.locked = false;
            }
            app.unlocked.remove(&id);
        }
        for id in &ids {
            if app.doc.note(*id).is_some_and(|note| note.locked) {
                app.unlocked.insert(*id, code.clone());
            }
        }
        app.session_passcode = Some(code.clone());
        let App { store, doc, .. } = app;
        store.save(doc).map_err(|e| e.to_string())?;
    }

    let mut all = extras::load(&dir);
    match kind.as_str() {
        "notebook" => {
            let id: u64 = key.parse().map_err(|_| "Unknown notebook")?;
            all.locked_notebooks.retain(|book| *book != id);
            if locked {
                all.locked_notebooks.push(id);
            }
        }
        _ => {
            all.locked_spaces.retain(|space| *space != key);
            if locked {
                all.locked_spaces.push(key);
            }
        }
    }
    extras::save(&dir, &all)
}

/// Lock a note: its body is sealed with the passcode before it is saved.
#[tauri::command]
fn lock_note(state: State<'_, Mutex<App>>, id: u64, passcode: String) -> Result<(), String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let dir = app.store.dir().to_path_buf();
    if extras::load(&dir).lock.is_none() {
        return Err("Set an app passcode in Settings first — the note is sealed with it.".into());
    }
    if extras::hash_passcode(&passcode) != extras::load(&dir).lock.unwrap_or_default() {
        return Err("That passcode doesn't match".into());
    }

    if let Some(note) = app.doc.note_mut(id) {
        if !ownnotes_core::crypto::is_encrypted(&note.body) {
            let sealed = ownnotes_core::crypto::encrypt(&passcode, &note.body);
            if sealed.is_empty() {
                return Err("The note couldn't be encrypted; it was not locked".into());
            }
            note.body = sealed;
        }
        note.locked = true;
        note.updated = chrono::Local::now();
    }
    app.unlocked.remove(&id);
    app.session_passcode = Some(passcode);
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())?;
    // Its older versions are readable text: they go, here and (on the next
    // sync) on the server. Versions from now on are sealed.
    store.forget_versions(id);
    Ok(())
}

/// Unlock a note: the body is opened in memory for this session.
#[tauri::command]
fn unlock_note(
    state: State<'_, Mutex<App>>,
    id: u64,
    passcode: String,
) -> Result<String, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let body = app
        .doc
        .note(id)
        .map(|note| note.body.clone())
        .ok_or("No such note")?;

    if ownnotes_core::crypto::is_encrypted(&body) {
        match ownnotes_core::crypto::decrypt(&passcode, &body) {
            Some(plain) => {
                app.unlocked.insert(id, passcode.clone());
                app.session_passcode = Some(passcode);
                Ok(plain)
            }
            None => Err("That passcode doesn't open this note".into()),
        }
    } else {
        // Written before notes were sealed: nothing to open.
        app.unlocked.insert(id, passcode);
        Ok(body)
    }
}

/// Open every note in a locked notebook or space for this session. The lock
/// and the sealed files stay; quitting locks them again.
#[tauri::command]
fn open_container(
    state: State<'_, Mutex<App>>,
    kind: String,
    key: String,
    passcode: String,
) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let app = &mut *guard;
    let Some(hash) = extras::load(app.store.dir()).lock else {
        return Err("There is no app passcode".into());
    };
    if extras::hash_passcode(&passcode) != hash {
        return Err("That passcode doesn't match".into());
    }
    let ids = notes_in_container(app, &kind, &key);
    for id in &ids {
        if let Some(note) = app.doc.note(*id) {
            if ownnotes_core::crypto::is_encrypted(&note.body)
                && ownnotes_core::crypto::decrypt(&passcode, &note.body).is_none()
            {
                return Err("That passcode doesn't open one of the notes".into());
            }
        }
    }
    for id in ids {
        app.unlocked.insert(id, passcode.clone());
    }
    app.session_passcode = Some(passcode);
    Ok(())
}

/// Take the lock off a note for good: its text is stored readable again.
#[tauri::command]
fn remove_note_lock(state: State<'_, Mutex<App>>, id: u64, passcode: String) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let app = &mut *guard;
    let Some(hash) = extras::load(app.store.dir()).lock else {
        return Err("There is no app passcode".into());
    };
    if extras::hash_passcode(&passcode) != hash {
        return Err("That passcode doesn't match".into());
    }
    let Some(note) = app.doc.note_mut(id) else { return Err("No such note".into()) };
    if ownnotes_core::crypto::is_encrypted(&note.body) {
        note.body = ownnotes_core::crypto::decrypt(&passcode, &note.body)
            .ok_or("That passcode doesn't open this note")?;
    }
    note.locked = false;
    note.updated = chrono::Local::now();
    app.unlocked.remove(&id);
    let App { store, doc, .. } = app;
    store.save(doc).map_err(|e| e.to_string())
}

/// Put the same note in another notebook: a copy of its file appears there.
#[tauri::command]
fn add_to_notebook(state: State<'_, Mutex<App>>, id: u64, notebook: u64) -> Result<(), String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    if !app.doc.has_notebook(notebook) {
        return Err("That notebook is gone".into());
    }
    if let Some(note) = app.doc.note_mut(id) {
        if note.notebook_id != notebook && !note.also_in.contains(&notebook) {
            note.also_in.push(notebook);
        }
    }
    if let Err(error) = seal_if_in_locked_container(&mut app, id) {
        if let Some(note) = app.doc.note_mut(id) {
            note.also_in.retain(|other| *other != notebook);
        }
        return Err(error);
    }
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())
}

/// Take the note out of one notebook (its own notebook can't be removed).
#[tauri::command]
fn remove_from_notebook(
    state: State<'_, Mutex<App>>,
    id: u64,
    notebook: u64,
) -> Result<(), String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    if let Some(note) = app.doc.note_mut(id) {
        note.also_in.retain(|other| *other != notebook);
    }
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())
}

/// Move a note to another space.
#[tauri::command]
fn move_note_space(state: State<'_, Mutex<App>>, id: u64, space: String) -> Result<(), String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let space = space.trim().to_string();
    if space.is_empty() {
        return Err("A space needs a name".into());
    }
    let before = app.doc.note(id).map(|note| note.space.clone());
    if let Some(note) = app.doc.note_mut(id) {
        note.space = space;
        note.updated = chrono::Local::now();
    }
    if let Err(error) = seal_if_in_locked_container(&mut app, id) {
        if let (Some(old), Some(note)) = (before, app.doc.note_mut(id)) {
            note.space = old;
        }
        return Err(error);
    }
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())
}

#[tauri::command]
fn move_note(state: State<'_, Mutex<App>>, id: u64, notebook: u64) -> Result<(), String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    if notebook != 0 && !app.doc.has_notebook(notebook) {
        return Err("That notebook is gone".into());
    }
    if !app.doc.has_notebook(notebook) {
        return Err("That notebook is gone".into());
    }
    let before = app.doc.note(id).map(|note| note.notebook_id);
    if let Some(note) = app.doc.note_mut(id) {
        note.notebook_id = notebook;
        // A note outside every notebook has no other homes either.
        if notebook == 0 {
            note.also_in.clear();
        }
        note.updated = chrono::Local::now();
    }
    if let Err(error) = seal_if_in_locked_container(&mut app, id) {
        if let (Some(old), Some(note)) = (before, app.doc.note_mut(id)) {
            note.notebook_id = old;
        }
        return Err(error);
    }
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_everything(state: State<'_, Mutex<App>>) -> Result<(), String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let dir = app.store.dir().to_path_buf();
    let _ = secrets::forget();
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.to_string()),
    }
    // The notes still in memory would otherwise be written straight back on
    // the next save. Start over as on a first launch.
    let (store, doc, saved, _) = Store::open_at(dir);
    app.store = store;
    app.doc = doc;
    app.prefs = saved.prefs;
    app.sync = saved.sync;
    app.unlocked.clear();
    app.session_passcode = None;
    Ok(())
}

#[tauri::command]
fn note_details(state: State<'_, Mutex<App>>, id: u64) -> Result<serde_json::Value, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let note = app.doc.note(id).ok_or("No such note")?;
    let text = ownnotes_core::html::html_to_text(&note.body);
    let words = text.split_whitespace().count();
    let headings: Vec<String> = note
        .body
        .split('<')
        .filter(|chunk| {
            let tag = chunk.split('>').next().unwrap_or("");
            tag.starts_with("h1") || tag.starts_with("h2") || tag.starts_with("h3")
        })
        .map(|chunk| {
            let body = chunk.split_once('>').map(|(_, rest)| rest).unwrap_or("");
            ownnotes_core::html::html_to_text(body)
        })
        .filter(|line| !line.trim().is_empty())
        .collect();
    Ok(serde_json::json!({
        "id": note.id,
        "path": format!("note-{}.html", note.id),
        "notebook": app.doc.notebook_name(note.notebook_id),
        "created": note.created.format("%b %-d, %Y  %-I:%M %p").to_string(),
        "updated": note.updated.format("%b %-d, %Y  %-I:%M %p").to_string(),
        "words": words,
        "characters": text.chars().count(),
        "paragraphs": note.body.matches("<p>").count(),
        "headings": headings,
        "todos": note.open_todos(),
    }))
}

#[tauri::command]
fn open_backups(state: State<'_, Mutex<App>>) -> Result<(), String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let dir = app.store.dir().join("backups");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    reveal(&dir)
}

#[tauri::command]
fn backups_dir(state: State<'_, Mutex<App>>) -> Result<String, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    Ok(app.store.dir().join("backups").display().to_string())
}

#[tauri::command]
fn make_backup(state: State<'_, Mutex<App>>) -> Result<String, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let dir = app.store.dir();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let target = dir.join("backups").join(&stamp);
    std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == "backups" || name == "target" {
            continue;
        }
        if path.is_dir() {
            copy_dir(&path, &target.join(&name)).map_err(|e| e.to_string())?;
        } else if name.to_string_lossy().ends_with(".json") {
            std::fs::copy(&path, target.join(&name)).map_err(|e| e.to_string())?;
        }
    }
    Ok(target.display().to_string())
}

fn copy_dir(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

// ----- attachments and maths -----

/// Keep a file dropped, pasted or picked into a note. The bytes arrive as the
/// raw request body; the note id and original file name in the `x-note` and
/// `x-name` headers. Returns the `<note id>/<file name>` reference.
#[tauri::command]
fn save_attachment(
    state: State<'_, Mutex<App>>,
    request: tauri::ipc::Request<'_>,
) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("No file was sent".into());
    };
    let name = request
        .headers()
        .get("x-name")
        .and_then(|v| v.to_str().ok())
        .map(|v| percent_decode(v))
        .unwrap_or_default();
    let note_id = request
        .headers()
        .get("x-note")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .ok_or("Which note is this file for?")?;
    let dir = state.lock().map_err(|e| e.to_string())?.store.dir().to_path_buf();
    attachments::save(&dir, note_id, &name, bytes).map_err(|e| e.to_string())
}

/// `%20`-style decoding for the file name header.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Open an attachment in the app the system uses for that kind of file.
#[tauri::command]
fn open_attachment(state: State<'_, Mutex<App>>, reference: String) -> Result<(), String> {
    let dir = state.lock().map_err(|e| e.to_string())?.store.dir().to_path_buf();
    let path = attachments::path(&dir, &reference).ok_or("That isn't an attachment")?;
    if !path.exists() {
        return Err("This file hasn't been downloaded yet — sync, then try again.".into());
    }
    // Files that would run something when opened are shown in the file
    // manager instead, so a note synced from elsewhere can't start a program.
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
        return reveal(&path);
    }
    system_open(&path)
}

/// Show a file in Finder / Explorer / the file manager without opening it.
fn reveal(path: &std::path::Path) -> Result<(), String> {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg("-R").arg(path).spawn()
    } else if cfg!(windows) {
        std::process::Command::new("explorer").arg(format!("/select,{}", path.display())).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn()
    };
    result.map(drop).map_err(|e| e.to_string())
}

/// Open a web link in the default browser. Only http(s) links, so a note
/// can't make this start a program.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    let parsed = url::Url::parse(&url).map_err(|_| "That isn't a web address".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only web links can be opened".into());
    }
    system_open(parsed.as_str())
}

fn system_open(target: impl AsRef<std::ffi::OsStr>) -> Result<(), String> {
    let mut command = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else {
        std::process::Command::new("xdg-open")
    };
    command.arg(target).spawn().map(drop).map_err(|e| e.to_string())
}

/// Give a note its own copies of the attachments its text points at in
/// other notes (after merging, duplicating or using a template), and return
/// the text with the links pointing at the copies. Otherwise emptying the
/// trash would delete the originals' files and break the pictures here.
#[tauri::command]
fn adopt_attachments(state: State<'_, Mutex<App>>, id: u64, body: String) -> Result<String, String> {
    let dir = state.lock().map_err(|e| e.to_string())?.store.dir().to_path_buf();
    Ok(attachments::adopt(&dir, id, &body))
}

#[tauri::command]
fn render_math(tex: String, block: bool) -> Result<String, String> {
    ownnotes_core::math::to_mathml(&tex, block)
}

/// Serve `attachment://localhost/<note id>/<file name>` from the attachments
/// folder.
fn serve_attachment(dir: &std::path::Path, uri_path: &str) -> tauri::http::Response<Vec<u8>> {
    let reference = percent_decode(uri_path.trim_start_matches('/'));
    let name = reference.as_str();
    let found = attachments::path(dir, name).and_then(|path| std::fs::read(path).ok());
    match found {
        Some(bytes) => tauri::http::Response::builder()
            .header("Content-Type", attachments::mime(name))
            .header("Access-Control-Allow-Origin", "*")
            .body(bytes)
            .unwrap_or_default(),
        None => tauri::http::Response::builder()
            .status(404)
            .header("Content-Type", "text/plain")
            .body(b"Not downloaded yet".to_vec())
            .unwrap_or_default(),
    }
}

/// Open one note in a window of its own.
#[tauri::command]
fn open_note_window(app: tauri::AppHandle, id: u64) -> Result<(), String> {
    let label = format!("note-{id}");
    use tauri::Manager;
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


/// A PDF made of the page as it looks: the capture is a JPEG, one page per
/// slice of it. The text is drawn as part of the picture, so it is not
/// selectable — the price of keeping colours, images and layout exact.
fn pdf_from_jpeg(jpeg: &[u8], width: u32, height: u32) -> Vec<u8> {
    let page_w = 595.0f32;
    let page_h = 842.0f32;
    let margin = 28.0f32;
    let usable_w = page_w - margin * 2.0;
    let usable_h = page_h - margin * 2.0;
    let scale = usable_w / width as f32;
    let slice_px = (usable_h / scale).max(1.0);
    let pages = ((height as f32 / slice_px).ceil() as usize).max(1);

    let mut objects: Vec<Vec<u8>> = Vec::new();
    let image_obj = 3 + pages * 2;

    // 1: catalog, 2: pages
    objects.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 3 + i * 2)).collect();
    objects.push(
        format!(
            "<< /Type /Pages /Count {pages} /Kids [{}] >>",
            kids.join(" ")
        )
        .into_bytes(),
    );

    for page in 0..pages {
        let content_obj = 4 + page * 2;
        objects.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_w} {page_h}] \
                 /Resources << /XObject << /Im0 {image_obj} 0 R >> >> /Contents {content_obj} 0 R >>"
            )
            .into_bytes(),
        );

        let top = page as f32 * slice_px * scale;
        let stream = format!(
            "q\n{} {} {} {} re W n\n{} 0 0 -{} {} {} cm\n/Im0 Do\nQ\n",
            margin,
            margin,
            usable_w,
            usable_h,
            scale,
            scale,
            margin,
            page_h - margin + top,
        );
        objects.push(
            format!(
                "<< /Length {} >>\nstream\n{}endstream",
                stream.len(),
                stream
            )
            .into_bytes(),
        );
    }

    // the picture itself
    let mut image = format!(
        "<< /Type /XObject /Subtype /Image /Width {width} /Height {height} \
         /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n",
        jpeg.len()
    )
    .into_bytes();
    image.extend_from_slice(jpeg);
    image.extend_from_slice(b"\nendstream");
    objects.push(image);

    // assemble with an xref table
    let mut out = String::from("%PDF-1.4\n").into_bytes();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// Save the page as a PDF in Downloads. The picture comes from the app, which
/// draws the note exactly as it looks.
#[tauri::command]
fn save_pdf(
    state: State<'_, Mutex<App>>,
    request: tauri::ipc::Request<'_>,
) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("No picture was sent".into());
    };
    let header = |name: &str| -> Option<String> {
        request
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(percent_decode)
    };
    let name = header("x-name").unwrap_or_else(|| "note".to_string());
    let width: u32 = header("x-width")
        .and_then(|value| value.parse().ok())
        .ok_or("The picture had no width")?;
    let height: u32 = header("x-height")
        .and_then(|value| value.parse().ok())
        .ok_or("The picture had no height")?;

    let pdf = pdf_from_jpeg(bytes, width, height);
    let app = state.lock().map_err(|e| e.to_string())?;
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let dir = home
        .map(|home| home.join("Downloads"))
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(|| app.store.dir().to_path_buf());
    let mut safe: String = name
        .chars()
        .map(|c| if matches!(c, '/' | ':' | '\\') { '-' } else { c })
        .collect();
    safe = safe.trim().chars().take(80).collect();
    if safe.is_empty() {
        safe = "note".to_string();
    }
    let path = unique_path(&dir, &safe, "pdf");
    std::fs::write(&path, pdf).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// The system's own print panel, on the page as it is drawn — "Save as PDF"
/// in that panel is a real PDF file.
#[tauri::command]
fn print_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.print().map_err(|e| e.to_string())
}

/// Write a PNG the page captured of itself.
#[tauri::command]
fn save_png(
    state: State<'_, Mutex<App>>,
    request: tauri::ipc::Request<'_>,
) -> Result<String, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("No picture was sent".into());
    };
    let name = request
        .headers()
        .get("x-name")
        .and_then(|v| v.to_str().ok())
        .map(percent_decode)
        .unwrap_or_else(|| "note".to_string());

    let app = state.lock().map_err(|e| e.to_string())?;
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let dir = home
        .map(|home| home.join("Downloads"))
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(|| app.store.dir().to_path_buf());
    let mut safe: String = name
        .chars()
        .map(|c| if matches!(c, '/' | ':' | '\\') { '-' } else { c })
        .collect();
    safe = safe.trim().chars().take(80).collect();
    if safe.is_empty() {
        safe = "note".to_string();
    }
    let path = unique_path(&dir, &safe, "png");
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Put text on the clipboard. The web clipboard API is not always available
/// in a web view, so the system's own tool is used instead.
/// Forget the older versions of one note.
#[tauri::command]
fn clear_versions(state: State<'_, Mutex<App>>, id: u64) -> Result<usize, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let notebook = app
        .doc
        .note(id)
        .map(|note| app.doc.notebook_name(note.notebook_id).to_string())
        .unwrap_or_default();
    let versions = app.store.versions(&notebook, id);
    for version in &versions {
        let name = format!(
            "{}/{}/note-{}-{}.html",
            ownnotes_core::store::VERSIONS_FOLDER,
            ownnotes_core::store::workspace_folder(&notebook),
            id,
            version.at.timestamp_millis()
        );
        app.store.remove_version_file(&name);
    }
    Ok(versions.len())
}

/// Throw away every stored version, everywhere.
#[tauri::command]
fn clear_all_versions(state: State<'_, Mutex<App>>) -> Result<(), String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let dir = app.store.dir().join("versions");
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// Read or write one interface preference. `value: None` reads it.
#[tauri::command]
fn ui_pref(
    state: State<'_, Mutex<App>>,
    key: String,
    value: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let dir = app.store.dir();
    let mut all = extras::load(dir);
    match value {
        Some(value) => {
            all.ui.insert(key, value.clone());
            extras::save(dir, &all)?;
            Ok(value)
        }
        None => Ok(all.ui.get(&key).cloned().unwrap_or(serde_json::Value::Null)),
    }
}

/// Notes whose file is not what the last sync left on the server: edited
/// since, or never synced.
#[tauri::command]
fn unsynced(state: State<'_, Mutex<App>>) -> Result<Vec<u64>, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    // Nothing can be "not yet synced" while sync is off.
    if !app.sync.active() {
        return Ok(Vec::new());
    }
    let source = app.sync_base();
    let state_file = app.store.load_sync_state(&source);
    let mut out = Vec::new();
    for note in &app.doc.notes {
        let rel = ownnotes_core::store::note_rel_path(app.doc.notebook_name(note.notebook_id), note.id);
        let text = ownnotes_core::store::render_note(note);
        let hash = ownnotes_core::cloud::engine::hash(&text);
        match state_file.files.get(&rel) {
            Some(known) if known.hash == hash => {}
            _ => out.push(note.id),
        }
    }
    Ok(out)
}

/// Rename a tag everywhere it is used, or remove it. Returns how many notes
/// changed.
#[tauri::command]
fn rename_tag(
    state: State<'_, Mutex<App>>,
    from: String,
    to: Option<String>,
) -> Result<usize, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let target = to
        .map(|name| name.trim().trim_start_matches('#').to_string())
        .filter(|name| !name.is_empty());
    if target.as_deref().is_some_and(|name| name.contains(char::is_whitespace)) {
        return Err("A tag can't contain spaces".into());
    }

    let mut changed = 0;
    for note in app.doc.notes.iter_mut() {
        let (title, a) = retag(&note.title, &from, target.as_deref());
        let (body, b) = retag(&note.body, &from, target.as_deref());
        if a + b > 0 {
            note.title = title;
            note.body = body;
            note.updated = chrono::Local::now();
            changed += 1;
        }
    }
    if changed > 0 {
        let App { store, doc, .. } = &mut *app;
        store.save(doc).map_err(|e| e.to_string())?;
    }
    Ok(changed)
}

/// Swap `#from` for `#to` (or delete it) outside of HTML tags, so a link's
/// `href="#from"` is left alone.
fn retag(text: &str, from: &str, to: Option<&str>) -> (String, usize) {
    let needle = format!("#{from}");
    if from.is_empty() {
        return (text.to_string(), 0);
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut in_tag = false;
    let mut count = 0;

    while let Some(at) = rest.find(&needle) {
        // Anything before the match that opens an unclosed element counts.
        for ch in rest[..at].chars() {
            match ch {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ => {}
            }
        }
        out.push_str(&rest[..at]);

        let before = out.chars().last();
        let after = rest[at + needle.len()..].chars().next();
        let free_before = before.is_none_or(|c| !c.is_alphanumeric() && c != '#' && c != '_' && c != '&');
        let free_after = after.is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '-');

        if !in_tag && free_before && free_after {
            count += 1;
            if let Some(to) = to {
                out.push('#');
                out.push_str(to);
            }
        } else {
            out.push_str(&needle);
        }
        rest = &rest[at + needle.len()..];
    }
    out.push_str(rest);
    (out, count)
}

/// Every note, out in one go: a single Markdown file, or a folder of HTML
/// files with one per note.
#[tauri::command]
fn export_all(state: State<'_, Mutex<App>>, format: Option<String>) -> Result<String, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let stamp = chrono::Local::now().format("%Y-%m-%d %H%M").to_string();
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let base = home
        .map(|home| home.join("Downloads"))
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(|| app.store.dir().to_path_buf());

    // Locked notes that aren't open this session are left out: their text
    // is sealed.
    let live: Vec<_> = app
        .doc
        .notes
        .iter()
        .filter(|note| !note.trashed)
        .filter(|note| !note.locked || app.unlocked.contains_key(&note.id))
        .collect();
    if live.is_empty() {
        return Err("There are no notes to export".into());
    }

    // Pictures and files of every note, collected once for the export.
    let mut used = std::collections::HashMap::new();

    if format.as_deref() == Some("html") {
        let folder = base.join(format!("OwnNotes {stamp}"));
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let mut taken = std::collections::HashSet::new();
        for note in &live {
            let title = note.display_title();
            let mut name: String = title
                .chars()
                .map(|c| if matches!(c, '/' | ':' | '\\') { '-' } else { c })
                .collect();
            name = name.trim().chars().take(80).collect();
            if name.is_empty() {
                name = format!("note-{}", note.id);
            }
            // Two notes with the same title get their own files.
            let base_name = name.clone();
            let mut n = 2;
            while !taken.insert(name.to_lowercase()) {
                name = format!("{base_name} ({n})");
                n += 1;
            }
            let (body, _) =
                attachments::export_copies(app.store.dir(), &open_body(&app, note), &folder, "attachments", &mut used);
            let page = format!(
                "<!doctype html><meta charset=\"utf-8\">{NO_SCRIPTS}<title>{}</title>\
                 <style>body{{font:16px/1.6 -apple-system,sans-serif;margin:40px auto;max-width:720px;color:#1f2328}}\
                 img{{max-width:100%}}</style><h1>{}</h1>{}",
                escape_html(title),
                escape_html(title),
                body
            );
            std::fs::write(folder.join(format!("{name}.html")), page).map_err(|e| e.to_string())?;
        }
        return Ok(folder.display().to_string());
    }

    let mut text = String::new();
    let files_folder = format!("OwnNotes {stamp} files");
    for note in &live {
        text.push_str(&format!("\n\n# {}\n\n", note.display_title()));
        text.push_str(&ownnotes_core::html::html_to_text(&open_body(&app, note)));
        let (_, files) = attachments::export_copies(app.store.dir(), &open_body(&app, note), &base, &files_folder, &mut used);
        if !files.is_empty() {
            text.push_str("\n\nAttachments:\n");
            for rel in files {
                let shown = percent_encoding::percent_decode_str(rel.rsplit('/').next().unwrap_or(&rel))
                    .decode_utf8_lossy()
                    .into_owned();
                text.push_str(&format!("- [{shown}]({rel})\n"));
            }
        }
    }
    let path = base.join(format!("OwnNotes {stamp}.md"));
    std::fs::write(&path, text.trim_start()).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// Evernote export → notes. `text` is the whole `.enex` file.
#[tauri::command]
fn import_enex(
    state: State<'_, Mutex<App>>,
    text: String,
    notebook: Option<u64>,
    space: Option<String>,
) -> Result<usize, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let notes = importer::parse_enex(&text);
    if notes.is_empty() {
        return Err("That file didn't look like an Evernote export".into());
    }
    let target = notebook
        .filter(|id| app.doc.has_notebook(*id))
        .or_else(|| app.doc.notebooks.first().map(|n| n.id))
        .ok_or("There is no notebook to import into")?;

    let space = space.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    for imported in &notes {
        let id = app.doc.add_note(target, &imported.title, &imported.body);
        if let Some(note) = app.doc.note_mut(id) {
            if let Some(created) = imported.created {
                note.created = created;
                note.updated = created;
            }
            if let Some(space) = &space {
                note.space = space.clone();
            }
        }
    }
    // Into a locked notebook or space: sealed like everything there.
    let fresh: Vec<u64> = app.doc.notes.iter().rev().take(notes.len()).map(|n| n.id).collect();
    for id in fresh {
        if let Err(error) = seal_if_in_locked_container(&mut app, id) {
            app.doc.notes.retain(|note| note.id != id);
            return Err(error);
        }
    }
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())?;
    Ok(notes.len())
}

/// A Bear archive (`.bear`, a zip of `.textbundle` folders) → notes.
#[tauri::command]
fn import_bear(
    state: State<'_, Mutex<App>>,
    request: tauri::ipc::Request<'_>,
    notebook: Option<u64>,
) -> Result<usize, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("No archive was sent".into());
    };
    let notes = importer::parse_bear(bytes)?;
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let target = notebook
        .filter(|id| app.doc.has_notebook(*id))
        .or_else(|| app.doc.notebooks.first().map(|n| n.id))
        .ok_or("There is no notebook to import into")?;
    for imported in &notes {
        app.doc.add_note(target, &imported.title, &imported.body);
    }
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())?;
    Ok(notes.len())
}

/// A Simplenote JSON backup → notes.
#[tauri::command]
fn import_simplenote(
    state: State<'_, Mutex<App>>,
    text: String,
    notebook: Option<u64>,
    space: Option<String>,
) -> Result<usize, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let notes = importer::parse_simplenote(&text);
    if notes.is_empty() {
        return Err("That file didn't look like a Simplenote backup (notes.json)".into());
    }
    let target = notebook
        .filter(|id| app.doc.has_notebook(*id))
        .or_else(|| app.doc.notebooks.first().map(|n| n.id))
        .ok_or("There is no notebook to import into")?;

    let space = space.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    for imported in &notes {
        let id = app.doc.add_note(target, &imported.title, &imported.body);
        if let Some(note) = app.doc.note_mut(id) {
            if let Some(created) = imported.created {
                note.created = created;
                note.updated = created;
            }
            if let Some(space) = &space {
                note.space = space.clone();
            }
        }
    }
    // Into a locked notebook or space: sealed like everything there.
    let fresh: Vec<u64> = app.doc.notes.iter().rev().take(notes.len()).map(|n| n.id).collect();
    for id in fresh {
        if let Err(error) = seal_if_in_locked_container(&mut app, id) {
            app.doc.notes.retain(|note| note.id != id);
            return Err(error);
        }
    }
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())?;
    Ok(notes.len())
}

#[derive(Serialize)]
struct AppleImport {
    imported: usize,
    locked: usize,
}

/// Read every note from the Notes app (macOS). The first time, macOS asks
/// whether OwnNotes may control Notes.
#[tauri::command]
async fn import_apple_notes(
    state: State<'_, Mutex<App>>,
    notebook: Option<u64>,
    space: Option<String>,
) -> Result<AppleImport, String> {
    if !cfg!(target_os = "macos") {
        return Err("Apple Notes can only be imported on a Mac.".into());
    }
    let output = tauri::async_runtime::spawn_blocking(|| {
        std::process::Command::new("osascript")
            .args(["-l", "JavaScript", "-e", importer::APPLE_NOTES_SCRIPT])
            .output()
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let why = String::from_utf8_lossy(&output.stderr);
        return Err(if why.contains("-1743") || why.contains("Not authorized") {
            "OwnNotes isn't allowed to read Notes. Allow it in System Settings → Privacy & Security → Automation, then try again.".into()
        } else {
            format!("Notes couldn't be read: {}", why.trim())
        });
    }
    let (notes, locked) = importer::parse_apple_notes(&String::from_utf8_lossy(&output.stdout));
    if notes.is_empty() {
        return Err(if locked > 0 {
            format!("All {locked} notes are locked in Notes; unlock them there first.")
        } else {
            "No notes were found in the Notes app.".into()
        });
    }

    let mut app = state.lock().map_err(|e| e.to_string())?;
    let target = notebook
        .filter(|id| app.doc.has_notebook(*id))
        .or_else(|| app.doc.notebooks.first().map(|n| n.id))
        .ok_or("There is no notebook to import into")?;
    let space = space.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    for imported in &notes {
        let id = app.doc.add_note(target, &imported.title, &imported.body);
        if let Some(note) = app.doc.note_mut(id) {
            if let Some(created) = imported.created {
                note.created = created;
                note.updated = created;
            }
            if let Some(space) = &space {
                note.space = space.clone();
            }
        }
    }
    // Into a locked notebook or space: sealed like everything there.
    let fresh: Vec<u64> = app.doc.notes.iter().rev().take(notes.len()).map(|n| n.id).collect();
    for id in fresh {
        if let Err(error) = seal_if_in_locked_container(&mut app, id) {
            app.doc.notes.retain(|note| note.id != id);
            return Err(error);
        }
    }
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())?;
    Ok(AppleImport { imported: notes.len(), locked })
}

/// A folder of Markdown files → notes.
#[tauri::command]
fn import_markdown(
    state: State<'_, Mutex<App>>,
    files: Vec<(String, String)>,
    notebook: Option<u64>,
    space: Option<String>,
) -> Result<usize, String> {
    let mut app = state.lock().map_err(|e| e.to_string())?;
    let sources = files
        .into_iter()
        .map(|(name, text)| importer::SourceFile { name, text })
        .collect::<Vec<_>>();
    let notes = importer::parse_markdown_folder(&sources);
    if notes.is_empty() {
        return Err("No Markdown files were found in that folder".into());
    }
    let target = notebook
        .filter(|id| app.doc.has_notebook(*id))
        .or_else(|| app.doc.notebooks.first().map(|n| n.id))
        .ok_or("There is no notebook to import into")?;

    let space = space.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    for imported in &notes {
        let id = app.doc.add_note(target, &imported.title, &imported.body);
        if let (Some(space), Some(note)) = (&space, app.doc.note_mut(id)) {
            note.space = space.clone();
        }
    }
    // Into a locked notebook or space: sealed like everything there.
    let fresh: Vec<u64> = app.doc.notes.iter().rev().take(notes.len()).map(|n| n.id).collect();
    for id in fresh {
        if let Err(error) = seal_if_in_locked_container(&mut app, id) {
            app.doc.notes.retain(|note| note.id != id);
            return Err(error);
        }
    }
    let App { store, doc, .. } = &mut *app;
    store.save(doc).map_err(|e| e.to_string())?;
    Ok(notes.len())
}

#[tauri::command]
fn copy_text(text: String) -> Result<(), String> {
    use std::io::Write;

    let mut command = if cfg!(target_os = "macos") {
        std::process::Command::new("pbcopy")
    } else if cfg!(windows) {
        std::process::Command::new("clip")
    } else if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        std::process::Command::new("wl-copy")
    } else {
        std::process::Command::new("xclip")
    };
    let mut child = command
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|_| "No clipboard tool was found".to_string())?;
    child
        .stdin
        .as_mut()
        .ok_or("No clipboard tool was found")?
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;
    child.wait().map_err(|e| e.to_string())?;
    Ok(())
}

/// Print a note (or save it as PDF) by handing a clean copy to the browser,
/// which has the print panel the web view doesn't.
#[tauri::command]
fn print_note(state: State<'_, Mutex<App>>, id: u64) -> Result<(), String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let note = app.doc.note(id).ok_or("No such note")?;
    if note.locked && !app.unlocked.contains_key(&id) {
        return Err("Unlock the note before printing it".into());
    }
    let text = open_body(&app, note);
    let title = note.display_title().to_string();
    let page = format!(
        "<!doctype html><meta charset=\"utf-8\">{NO_SCRIPTS}<title>{}</title>\
         <style>body{{font:16px/1.6 -apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;\
         margin:48px auto;max-width:720px;color:#1f2328}}img{{max-width:100%}}\
         blockquote{{border-left:3px solid #e6e8eb;margin:0;padding-left:12px;color:#6b727d}}\
         pre{{background:#f4f5f7;padding:10px 12px;border-radius:8px;overflow:auto}}\
         h1,h2,h3{{line-height:1.3}}</style>\
         <h1>{}</h1>{}",
        escape_html(&title),
        escape_html(&title),
        text
    );
    let path = std::env::temp_dir().join(format!("ownnotes-print-{id}.html"));
    std::fs::write(&path, page).map_err(|e| e.to_string())?;
    system_open(&path)
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[tauri::command]
fn log_ui_error(message: String) {
    eprintln!("[ui error] {message}");
}

#[tauri::command]
fn always_on_top(window: tauri::WebviewWindow, on: bool) -> Result<(), String> {
    window.set_always_on_top(on).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn export_note(
    state: State<'_, Mutex<App>>,
    id: u64,
    format: String,
    extras: Option<bool>,
) -> Result<String, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let note = app.doc.note(id).ok_or("No such note")?;
    // A locked note is exported as its text, and only when it is open.
    if note.locked && !app.unlocked.contains_key(&id) {
        return Err("Unlock the note before exporting it".into());
    }
    let open_text = open_body(&app, note);
    let title = note.display_title().to_string();
    let mut name: String = title
        .chars()
        .map(|c| if matches!(c, '/' | ':' | '\\') { '-' } else { c })
        .collect();
    name = name.trim().chars().take(80).collect();
    if name.is_empty() {
        name = format!("note-{id}");
    }

    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let dir = home
        .map(|home| home.join("Downloads"))
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(|| app.store.dir().to_path_buf());
    // Pictures and files go into a folder next to the export.
    let files_folder = format!("{name} files");
    let (linked_body, files) = attachments::export_copies(
        app.store.dir(),
        &open_text,
        &dir,
        &files_folder,
        &mut std::collections::HashMap::new(),
    );
    let file_list = if files.is_empty() {
        String::new()
    } else {
        let lines: Vec<String> = files
            .iter()
            .map(|rel| {
                let shown = percent_encoding::percent_decode_str(rel.rsplit('/').next().unwrap_or(rel))
                    .decode_utf8_lossy()
                    .into_owned();
                format!("- [{shown}]({rel})")
            })
            .collect();
        format!("\n\nAttachments:\n{}", lines.join("\n"))
    };

    let (extension, body) = if format == "html" {
        (
            "html",
            format!(
                "<!doctype html><meta charset=\"utf-8\">{NO_SCRIPTS}<title>{}</title>\n{}",
                escape_html(&title),
                linked_body
            ),
        )
    } else if format == "md" {
        let mut text = format!(
            "# {}\n\n{}{file_list}",
            title,
            ownnotes_core::html::html_to_text(&open_text)
        );
        if extras.unwrap_or(false) {
            text = format!(
                "---\nid: {}\nnotebook: {}\ncreated: {}\nupdated: {}\n---\n\n{}",
                note.id,
                app.doc.notebook_name(note.notebook_id),
                note.created.to_rfc3339(),
                note.updated.to_rfc3339(),
                text
            );
        }
        ("md", text)
    } else {
        (
            "txt",
            format!("{title}\n\n{}{file_list}", ownnotes_core::html::html_to_text(&open_text)),
        )
    };

    let path = unique_path(&dir, &name, extension);
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

#[tauri::command]
fn templates(state: State<'_, Mutex<App>>) -> Result<Vec<extras::Template>, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    Ok(extras::load(app.store.dir()).templates)
}

#[tauri::command]
fn save_template(
    state: State<'_, Mutex<App>>,
    name: String,
    body: String,
) -> Result<(), String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let dir = app.store.dir();
    let mut all = extras::load(dir);
    all.templates.retain(|t| t.name != name);
    all.templates.push(extras::Template { name, body });
    extras::save(dir, &all)
}

#[tauri::command]
fn delete_template(state: State<'_, Mutex<App>>, name: String) -> Result<(), String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let dir = app.store.dir();
    let mut all = extras::load(dir);
    all.templates.retain(|t| t.name != name);
    extras::save(dir, &all)
}

#[tauri::command]
fn lock_set(
    state: State<'_, Mutex<App>>,
    passcode: Option<String>,
    current: Option<String>,
) -> Result<(), String> {
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let app = &mut *guard;
    let dir = app.store.dir().to_path_buf();
    let mut all = extras::load(&dir);
    let current = current.unwrap_or_default();
    // Changing or removing a passcode needs the one in use, or anyone could
    // clear it and then open every locked note, notebook and space.
    if let Some(hash) = &all.lock {
        if extras::hash_passcode(&current) != *hash {
            return Err("Enter the current passcode to change it".into());
        }
    }
    let new = passcode.filter(|code| !code.is_empty());

    // Locked notes are sealed with the passcode itself: with a new one they
    // must be sealed again, or they could never be opened.
    let sealed: Vec<u64> = app
        .doc
        .notes
        .iter()
        .filter(|note| ownnotes_core::crypto::is_encrypted(&note.body))
        .map(|note| note.id)
        .collect();
    if !sealed.is_empty() {
        let Some(new_code) = &new else {
            return Err(format!(
                "{} locked note{} would become unreadable. Remove their locks first.",
                sealed.len(),
                if sealed.len() == 1 { "" } else { "s" }
            ));
        };
        let mut resealed = Vec::new();
        for id in &sealed {
            let Some(note) = app.doc.note(*id) else { continue };
            let plain = ownnotes_core::crypto::decrypt(&current, &note.body).ok_or(
                "A locked note doesn't open with the current passcode (it may come from another device); nothing changed",
            )?;
            let again = ownnotes_core::crypto::encrypt(new_code, &plain);
            if again.is_empty() {
                return Err("A note couldn't be encrypted; nothing changed".into());
            }
            resealed.push((*id, again));
        }
        for (id, body) in resealed {
            if let Some(note) = app.doc.note_mut(id) {
                note.body = body;
            }
        }
        for code in app.unlocked.values_mut() {
            *code = new_code.clone();
        }
        let App { store, doc, .. } = &mut *app;
        store.save(doc).map_err(|e| e.to_string())?;
    }

    app.session_passcode = new.clone();
    all.lock = new.map(|code| extras::hash_passcode(&code));
    extras::save(&dir, &all)
}

#[tauri::command]
fn lock_check(state: State<'_, Mutex<App>>, passcode: String) -> Result<bool, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    let all = extras::load(app.store.dir());
    Ok(all
        .lock
        .map(|stored| stored == extras::hash_passcode(&passcode))
        .unwrap_or(true))
}

#[tauri::command]
fn lock_state(state: State<'_, Mutex<App>>) -> Result<bool, String> {
    let app = state.lock().map_err(|e| e.to_string())?;
    Ok(extras::load(app.store.dir()).lock.is_some())
}

/// Keep a copy quietly in `backups/`: as often as the settings say, keeping
/// as many as they say.
fn daily_backup(dir: &std::path::Path) {
    let extras = extras::load(dir);
    let hours = extras
        .ui
        .get("backup_hours")
        .and_then(|value| value.as_u64())
        .unwrap_or(24)
        .max(1);
    let keep = extras
        .ui
        .get("backup_keep")
        .and_then(|value| value.as_u64())
        .unwrap_or(7)
        .clamp(1, 60) as usize;
    let backups = dir.join("backups");
    let fresh = std::fs::read_dir(&backups)
        .ok()
        .and_then(|entries| {
            let mut newest: Option<std::time::SystemTime> = None;
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        newest = Some(newest.map_or(modified, |best| best.max(modified)));
                    }
                }
            }
            newest
        })
        .and_then(|when| when.elapsed().ok())
        .is_some_and(|age| age.as_secs() < hours * 60 * 60);
    if fresh {
        return;
    }

    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let target = backups.join(stamp);
    if std::fs::create_dir_all(&target).is_err() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        // Attachments never change once added and can be large; copying
        // them every day would fill the disk. They stay in their own folder
        // (and on the server) instead.
        if name == "backups" || name == attachments::LOCAL_FOLDER {
            continue;
        }
        if path.is_dir() {
            let _ = copy_dir(&path, &target.join(&name));
        } else if name.to_string_lossy().ends_with(".json") {
            let _ = std::fs::copy(&path, target.join(&name));
        }
    }
    prune_backups(&backups, keep);
}

/// Keep only the newest `keep` backups. Only folders named like the ones made
/// here (`20240131-141500`) are ever removed.
fn prune_backups(backups: &std::path::Path, keep: usize) {
    let is_stamp = |name: &str| {
        let bytes = name.as_bytes();
        bytes.len() == 15
            && bytes[8] == b'-'
            && bytes.iter().enumerate().all(|(i, b)| i == 8 || b.is_ascii_digit())
    };
    let Ok(entries) = std::fs::read_dir(backups) else { return };
    let mut stamps: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| is_stamp(name))
        .collect();
    // The stamp sorts by time, oldest first.
    stamps.sort();
    let extra = stamps.len().saturating_sub(keep.max(1));
    for old in &stamps[..extra] {
        let _ = std::fs::remove_dir_all(backups.join(old));
    }
}

#[cfg(test)]
mod backup_tests {
    use super::prune_backups;

    #[test]
    fn only_the_newest_backups_are_kept() {
        let dir = std::env::temp_dir().join(format!("ownnotes-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for stamp in ["20260101-090000", "20260102-090000", "20260103-090000", "20260104-090000"] {
            std::fs::create_dir_all(dir.join(stamp)).unwrap();
        }
        std::fs::create_dir_all(dir.join("my own folder")).unwrap();
        prune_backups(&dir, 2);
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["20260103-090000", "20260104-090000", "my own folder"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod container_tests {
    use ownnotes_core::model::{Document, Notebook};

    fn doc() -> Document {
        let mut doc = Document::from_parts(
            vec![
                Notebook { parent: None, id: 1, name: "Work".into(), color: 0 },
                Notebook { parent: Some(1), id: 2, name: "Work / 2026".into(), color: 1 },
                Notebook { parent: None, id: 3, name: "Home".into(), color: 2 },
            ],
            Vec::new(),
        );
        let a = doc.add_note(1, "In work", "one");
        let b = doc.add_note(2, "In a sub-notebook", "two");
        let c = doc.add_note(3, "At home", "three");
        doc.note_mut(c).unwrap().space = "Personal".into();
        doc.note_mut(a).unwrap().also_in = vec![3];
        doc
    }

    #[test]
    fn a_notebook_lock_covers_its_children_and_copies() {
        let doc = doc();
        let app = super::App {
            store: ownnotes_core::store::Store::open_at(std::env::temp_dir()).0,
            doc,
            prefs: Default::default(),
            sync: Default::default(),
            unlocked: Default::default(),
            session_passcode: None,
        };
        let mut ids = super::notes_in_container(&app, "notebook", "1");
        ids.sort();
        // Work itself, its sub-notebook, and the note filed in Work *and*
        // Home — which counts once, not twice.
        assert_eq!(ids.len(), 2);
        assert!(!ids.contains(&3), "a note only in Home is not in Work");
    }

    #[test]
    fn a_space_lock_covers_only_its_notes() {
        use ownnotes_core::model::DEFAULT_SPACE;
        let doc = doc();
        let app = super::App {
            store: ownnotes_core::store::Store::open_at(std::env::temp_dir()).0,
            doc,
            prefs: Default::default(),
            sync: Default::default(),
            unlocked: Default::default(),
            session_passcode: None,
        };
        assert_eq!(super::notes_in_container(&app, "space", "Personal").len(), 1);
        assert_eq!(super::notes_in_container(&app, "space", DEFAULT_SPACE).len(), 2);
    }
}

#[cfg(test)]
mod tag_tests {
    use super::retag;

    #[test]
    fn tags_are_renamed_only_where_they_are_tags() {
        let (text, count) = retag("<p>#work and #workshop</p><a href=\"#work\">x</a>", "work", Some("job"));
        assert_eq!(count, 1);
        assert!(text.contains("#job and #workshop"));
        assert!(text.contains("href=\"#work\""));
    }

    #[test]
    fn a_tag_can_be_removed() {
        let (text, count) = retag("<p>one #temp two</p>", "temp", None);
        assert_eq!(count, 1);
        assert!(!text.contains("#temp"));
    }
}

fn main() {
    let (store, doc, saved, mut problems) = Store::open();
    {
        let dir = store.dir().to_path_buf();
        std::thread::spawn(move || daily_backup(&dir));
    }
    if let Some(problem) = problems.pop() {
        eprintln!("{problem}");
    }

    let app = App {
        store,
        doc,
        prefs: saved.prefs,
        sync: saved.sync,
        unlocked: std::collections::HashMap::new(),
        session_passcode: None,
    };

    let attachment_dir = app.store.dir().to_path_buf();
    let window_state = extras::load(&attachment_dir).window;

    tauri::Builder::default()
        // Put the window back where it was left, if that still fits a screen.
        .setup(move |app| {
            use tauri::Manager;
            if let (Some((x, y, w, h)), Some(window)) = (window_state, app.get_webview_window("main"))
            {
                let _ = window.set_size(tauri::PhysicalSize::new(w.max(700), h.max(480)));
                // A screen that was unplugged would put the window out of
                // sight: only go back to a spot some screen still shows.
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
            Ok(())
        })
        .on_window_event(move |window, event| {
            // Only the main window's place is remembered, not a note's own
            // window.
            if let (tauri::WindowEvent::CloseRequested { .. }, "main") = (event, window.label()) {
                use tauri::Manager;
                let Ok(size) = window.inner_size() else { return };
                let Ok(pos) = window.outer_position() else { return };
                if let Some(app) = window.app_handle().try_state::<Mutex<App>>() {
                    if let Ok(app) = app.lock() {
                        let dir = app.store.dir().to_path_buf();
                        let mut all = extras::load(&dir);
                        all.window = Some((pos.x, pos.y, size.width, size.height));
                        let _ = extras::save(&dir, &all);
                    }
                }
            }
        })
        .register_uri_scheme_protocol("attachment", move |_ctx, request| {
            serve_attachment(&attachment_dir, request.uri().path())
        })
        .manage(Mutex::new(app))
        .invoke_handler(tauri::generate_handler![
            load,
            save_note,
            create_note,
            create_notebook,
            set_flags,
            delete_note,
            empty_trash,
            save_prefs,
            sync_now,
            save_sync,
            sync_status,
            test_webdav,
            sync_options,
            save_sync_options,
            versions,
            restore_version,
            always_on_top,
            export_note,
            templates,
            save_template,
            delete_template,
            lock_set,
            lock_check,
            lock_state,
            rename_notebook,
            colour_notebook,
            remove_notebook,
            move_note,
            delete_everything,
            note_details,
            backups_dir,
            make_backup,
            save_attachment,
            open_attachment,
            render_math,
            adopt_attachments,
            locks,
            set_container_lock,
            import_apple_notes,
            open_url,
            log_ui_error,
            copy_text,
            print_window,
            open_note_window,
            save_png,
            save_pdf,
            print_note,
            import_enex,
            import_markdown,
            import_simplenote,
            import_bear,
            export_all,
            rename_tag,
            ui_pref,
            clear_versions,
            clear_all_versions,
            open_backups,
            move_note_space,
            add_to_notebook,
            remove_from_notebook,
            lock_note,
            unlock_note,
            open_container,
            remove_note_lock,
            unsynced
        ])
        .run(tauri::generate_context!())
        .expect("OwnNotes failed to start");
}
