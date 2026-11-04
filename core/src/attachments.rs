// Images, PDFs and other files added to notes.
//
// Each note keeps its files in a folder of its own, under their original
// names:
//
//   on this computer   attachments/<note id>/pic1.jpg
//   on the server      OwnNotes/<Notebook>/note-<id>_attachments/pic1.jpg
//
// so the server folder reads like the notes it belongs to. The note's HTML
// refers to a file as `attachment://localhost/<note id>/<name>`.
//
// A file never changes once it is added (adding another file with the same
// name gives it a new name), so syncing is simple: whatever one side is
// missing is copied over, and two versions of one file can't conflict.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::cloud::{CloudError, CloudResult, WebDav};

/// What the editor's `encodeURIComponent` leaves alone in a file name.
const LINK_NAME: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'!')
    .remove(b'~')
    .remove(b'*')
    .remove(b'\'')
    .remove(b'(')
    .remove(b')');

/// The local folder, inside the data folder.
pub const LOCAL_FOLDER: &str = "attachments";
/// Refuse anything larger rather than filling memory.
pub const MAX_BYTES: usize = 200 * 1024 * 1024;

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join(LOCAL_FOLDER)
}

/// The server folder for one note's files, next to the note itself.
pub fn remote_folder(workspace: &str, note_id: u64) -> String {
    format!("{workspace}/note-{note_id}_attachments")
}

/// The note id in a server folder name like `note-123_attachments`.
pub fn note_of_folder(folder: &str) -> Option<u64> {
    let id = folder.strip_prefix("note-")?.strip_suffix("_attachments")?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    id.parse().ok()
}

/// A file name that is safe to use as is: no folders, nothing hidden, no
/// control characters.
pub fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 150
        && !name.starts_with('.')
        && !name.contains(['/', '\\', ':'])
        && !name.chars().any(|c| c.is_control())
}

/// Turn whatever the file was called into a safe name, keeping it readable.
pub fn clean_file_name(original: &str) -> String {
    let base = original.rsplit(['/', '\\']).next().unwrap_or(original);
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_control() || matches!(c, ':' | '/' | '\\') { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').trim();
    let cleaned: String = if cleaned.chars().count() > 120 {
        // Keep the extension when shortening.
        match cleaned.rsplit_once('.') {
            Some((stem, ext)) if ext.len() <= 10 => {
                format!("{}.{ext}", stem.chars().take(100).collect::<String>())
            }
            _ => cleaned.chars().take(120).collect(),
        }
    } else {
        cleaned.to_string()
    };
    if cleaned.is_empty() {
        "file".to_string()
    } else {
        cleaned
    }
}

/// `<note id>/<file name>`, the part after `attachment://localhost/`.
pub fn split_ref(reference: &str) -> Option<(u64, &str)> {
    let (id, name) = reference.split_once('/')?;
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) || !valid_file_name(name) {
        return None;
    }
    Some((id.parse().ok()?, name))
}

/// The file on disk for a reference, if the reference is a valid one.
pub fn path(data_dir: &Path, reference: &str) -> Option<PathBuf> {
    let (id, name) = split_ref(reference)?;
    Some(dir(data_dir).join(id.to_string()).join(name))
}

/// Keep a file for a note and return the reference the note uses for it.
/// Adding the same file twice reuses it; a different file with a name that
/// is already taken becomes "name (2).ext".
pub fn save(data_dir: &Path, note_id: u64, original: &str, bytes: &[u8]) -> io::Result<String> {
    if bytes.len() > MAX_BYTES {
        return Err(io::Error::other(format!(
            "Files larger than {} MB can't be added to a note.",
            MAX_BYTES / 1024 / 1024
        )));
    }
    let folder = dir(data_dir).join(note_id.to_string());
    fs::create_dir_all(&folder)?;
    let wanted = clean_file_name(original);
    let (stem, ext) = match wanted.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (wanted.clone(), String::new()),
    };
    let mut n = 1;
    loop {
        let name = if n == 1 { wanted.clone() } else { format!("{stem} ({n}){ext}") };
        let path = folder.join(&name);
        match fs::read(&path) {
            Ok(existing) if existing == bytes => return Ok(format!("{note_id}/{name}")),
            Ok(_) => n += 1,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                write_atomic(&folder, &name, bytes)?;
                return Ok(format!("{note_id}/{name}"));
            }
            Err(e) => return Err(e),
        }
    }
}

fn write_atomic(folder: &Path, name: &str, bytes: &[u8]) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    let tmp = folder.join(format!(".{name}.tmp.{}", std::process::id()));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, folder.join(name))
}

pub fn mime(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "heic" => "image/heic",
        "bmp" => "image/bmp",
        "pdf" => "application/pdf",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "txt" | "md" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "json" => "application/json",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

/// Every file kept on this computer, as (note id, file name).
fn local_files(data_dir: &Path) -> HashSet<(u64, String)> {
    let mut out = HashSet::new();
    let Ok(notes) = fs::read_dir(dir(data_dir)) else { return out };
    for note in notes.flatten() {
        let Some(id) = note.file_name().to_str().and_then(|s| s.parse::<u64>().ok()) else {
            continue;
        };
        let Ok(files) = fs::read_dir(note.path()) else { continue };
        for file in files.flatten() {
            if let Some(name) = file.file_name().to_str() {
                if valid_file_name(name) && file.path().is_file() {
                    out.insert((id, name.to_string()));
                }
            }
        }
    }
    out
}

#[derive(Debug, Default, PartialEq)]
pub struct Moved {
    pub uploaded: usize,
    pub downloaded: usize,
}

/// Rewrite `attachment://localhost/<other id>/<name>` links in `body` to point
/// at copies kept for `note_id`, copying each file once. Links to files that
/// aren't on this computer are left as they are.
pub fn adopt(data_dir: &Path, note_id: u64, body: &str) -> String {
    const PREFIX: &str = "attachment://localhost/";
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(at) = rest.find(PREFIX) {
        out.push_str(&rest[..at + PREFIX.len()]);
        rest = &rest[at + PREFIX.len()..];
        // The reference runs to the closing quote of the attribute (the
        // editor writes them double-quoted; names may contain brackets).
        let end = rest.find(['"', '<', ' ', '\n']).unwrap_or(rest.len());
        let encoded = &rest[..end];
        let decoded = percent_encoding::percent_decode_str(encoded).decode_utf8_lossy().into_owned();
        let replaced = split_ref(&decoded)
            .filter(|(from, _)| *from != note_id)
            .and_then(|(from, name)| {
                let bytes = fs::read(dir(data_dir).join(from.to_string()).join(name)).ok()?;
                let new_ref = save(data_dir, note_id, name, &bytes).ok()?;
                let (_, new_name) = split_ref(&new_ref)?;
                Some(format!(
                    "{note_id}/{}",
                    percent_encoding::utf8_percent_encode(new_name, LINK_NAME)
                ))
            });
        match replaced {
            Some(new) => out.push_str(&new),
            None => out.push_str(encoded),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// For an export: copy the files `body` links to into `dest/<folder>/` and
/// point the links there (relative, so the export folder can be moved).
/// `used` keeps names unique across several notes of one export. Returns the
/// new body and the relative paths of the files copied for it.
pub fn export_copies(
    data_dir: &Path,
    body: &str,
    dest: &Path,
    folder: &str,
    used: &mut HashMap<String, String>,
) -> (String, Vec<String>) {
    const PREFIX: &str = "attachment://localhost/";
    let mut out = String::with_capacity(body.len());
    let mut files = Vec::new();
    let mut rest = body;
    while let Some(at) = rest.find(PREFIX) {
        out.push_str(&rest[..at]);
        let after = &rest[at + PREFIX.len()..];
        let end = after.find(['"', '<', ' ', '\n']).unwrap_or(after.len());
        let encoded = &after[..end];
        let decoded = percent_encoding::percent_decode_str(encoded).decode_utf8_lossy().into_owned();
        let copied = split_ref(&decoded).and_then(|(id, name)| {
            let key = format!("{id}/{name}");
            if let Some(rel) = used.get(&key) {
                return Some(rel.clone());
            }
            let bytes = fs::read(dir(data_dir).join(id.to_string()).join(name)).ok()?;
            // Two notes may each have a "photo.jpg": the second gets a number.
            let (stem, ext) = match name.rsplit_once('.') {
                Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
                _ => (name.to_string(), String::new()),
            };
            let mut n = 1;
            let file = loop {
                let candidate = if n == 1 { name.to_string() } else { format!("{stem} ({n}){ext}") };
                if !used.values().any(|rel| rel.ends_with(&format!("/{}", encode_name(&candidate)))) {
                    break candidate;
                }
                n += 1;
            };
            write_atomic(&dest.join(folder), &file, &bytes).ok()?;
            let rel = format!("{}/{}", encode_name(folder), encode_name(&file));
            used.insert(key, rel.clone());
            Some(rel)
        });
        match copied {
            Some(rel) => {
                if !files.contains(&rel) {
                    files.push(rel.clone());
                }
                out.push_str(&rel);
            }
            // A file that isn't on this computer keeps its old link.
            None => {
                out.push_str(PREFIX);
                out.push_str(encoded);
            }
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    (out, files)
}

fn encode_name(name: &str) -> String {
    percent_encoding::utf8_percent_encode(name, LINK_NAME).to_string()
}

/// Remove a note's files from this computer (the note was deleted for good).
pub fn remove_local(data_dir: &Path, note_id: u64) {
    let _ = fs::remove_dir_all(dir(data_dir).join(note_id.to_string()));
}

/// Copy whatever each side is missing, then tidy up after notes that moved
/// to another notebook or were deleted.
///
/// `folders` gives the notebook folder of every note on this computer. Files
/// of a note that isn't here and wasn't deleted (not synced yet) are left
/// alone on the server. `deleted` lists notes that this sync saw go away, on
/// either side: their files are removed here and on the server.
pub fn sync(
    remote: &mut WebDav,
    data_dir: &Path,
    folders: &HashMap<u64, String>,
    deleted: &HashSet<u64>,
) -> CloudResult<Moved> {
    // (workspace, note id, file name) for every attachment on the server.
    let mut there = remote.list_attachments()?;
    let mut moved = Moved::default();

    // Deleted notes: their files go, on both sides.
    for id in deleted {
        if folders.contains_key(id) {
            continue;
        }
        remove_local(data_dir, *id);
        let gone: HashSet<String> = there
            .iter()
            .filter(|(_, note, _)| note == id)
            .map(|(workspace, _, name)| {
                format!("{}/{name}", remote_folder(workspace, *id))
            })
            .collect();
        for path in &gone {
            remote.delete_path(path)?;
        }
        let workspaces: HashSet<String> = there
            .iter()
            .filter(|(_, note, _)| note == id)
            .map(|(w, _, _)| w.clone())
            .collect();
        for workspace in workspaces {
            remote.delete_folder_if_empty(&remote_folder(&workspace, *id));
        }
        there.retain(|(_, note, _)| note != id);
    }
    let here = local_files(data_dir);

    // Download: anything on the server this computer doesn't have, wherever
    // the server keeps it.
    for (workspace, id, name) in &there {
        if here.contains(&(*id, name.clone())) || !valid_file_name(name) {
            continue;
        }
        let path = format!("{}/{name}", remote_folder(workspace, *id));
        let bytes = remote.get_bytes(&path, MAX_BYTES)?;
        write_atomic(&dir(data_dir).join(id.to_string()), name, &bytes)
            .map_err(|e| CloudError::new(format!("Couldn't save the attachment {name}: {e}")))?;
        moved.downloaded += 1;
    }

    // Upload: anything here that isn't in the note's current folder there.
    let at: HashSet<(&str, u64, &str)> = there
        .iter()
        .map(|(w, id, name)| (w.as_str(), *id, name.as_str()))
        .collect();
    let mut placed: HashSet<u64> = HashSet::new();
    for (id, name) in &here {
        let Some(workspace) = folders.get(id) else { continue };
        placed.insert(*id);
        if at.contains(&(workspace.as_str(), *id, name.as_str())) {
            continue;
        }
        let bytes = fs::read(dir(data_dir).join(id.to_string()).join(name))
            .map_err(|e| CloudError::new(format!("Couldn't read the attachment {name}: {e}")))?;
        let path = format!("{}/{name}", remote_folder(workspace, *id));
        remote.put_bytes(&path, mime(name), &bytes)?;
        moved.uploaded += 1;
    }

    // A note that moved notebook: its files now sit in the new folder too,
    // so the copies in the old folder can go.
    for (workspace, id, name) in &there {
        let Some(current) = folders.get(id) else { continue };
        if current != workspace && placed.contains(id) {
            remote.delete_path(&format!("{}/{name}", remote_folder(workspace, *id)))?;
        }
    }
    let stale: HashSet<(String, u64)> = there
        .iter()
        .filter(|(w, id, _)| folders.get(id).is_some_and(|c| c != w) && placed.contains(id))
        .map(|(w, id, _)| (w.clone(), *id))
        .collect();
    for (workspace, id) in stale {
        remote.delete_folder_if_empty(&remote_folder(&workspace, id));
    }
    Ok(moved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ownnotes-attach-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn files_keep_their_names_per_note() {
        let data = temp_dir("names");
        let a = save(&data, 7, "pic1.jpg", b"one").unwrap();
        assert_eq!(a, "7/pic1.jpg");
        // The same file again is reused…
        assert_eq!(save(&data, 7, "pic1.jpg", b"one").unwrap(), "7/pic1.jpg");
        // …a different file with the same name gets a new one.
        assert_eq!(save(&data, 7, "pic1.jpg", b"two").unwrap(), "7/pic1 (2).jpg");
        // Another note has its own folder.
        assert_eq!(save(&data, 8, "pic1.jpg", b"two").unwrap(), "8/pic1.jpg");
        assert_eq!(fs::read(path(&data, "7/pic1 (2).jpg").unwrap()).unwrap(), b"two");
        assert_eq!(local_files(&data).len(), 3);
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn a_merged_note_gets_its_own_copies() {
        let data = temp_dir("adopt");
        save(&data, 7, "pic 1.jpg", b"jpeg").unwrap();
        save(&data, 7, "pic (2).jpg", b"second").unwrap();
        let body = r#"<p>x</p><img src="attachment://localhost/7/pic%201.jpg"><img src="attachment://localhost/7/pic%20(2).jpg"><img src="attachment://localhost/9/missing.png">"#;
        let out = adopt(&data, 20, body);
        assert!(out.contains("attachment://localhost/20/pic%201.jpg"), "{out}");
        assert!(out.contains("attachment://localhost/20/pic%20(2).jpg"), "brackets survive: {out}");
        assert!(out.contains("attachment://localhost/9/missing.png"), "missing files stay as they were");
        assert_eq!(fs::read(path(&data, "20/pic 1.jpg").unwrap()).unwrap(), b"jpeg");
        // Its own links are left alone.
        assert_eq!(adopt(&data, 20, &out), out);
        let _ = fs::remove_dir_all(&data);
    }

    #[test]
    fn exports_carry_their_files_along() {
        let data = temp_dir("export");
        let out_dir = temp_dir("export-out");
        save(&data, 1, "photo.jpg", b"one").unwrap();
        save(&data, 2, "photo.jpg", b"two").unwrap();
        let mut used = HashMap::new();
        let (a, files_a) = export_copies(
            &data,
            r#"<img src="attachment://localhost/1/photo.jpg">"#,
            &out_dir,
            "Notes files",
            &mut used,
        );
        let (b, _) = export_copies(
            &data,
            r#"<img src="attachment://localhost/2/photo.jpg"><img src="attachment://localhost/1/photo.jpg">"#,
            &out_dir,
            "Notes files",
            &mut used,
        );
        assert_eq!(a, r#"<img src="Notes%20files/photo.jpg">"#);
        assert_eq!(files_a, ["Notes%20files/photo.jpg"]);
        assert!(b.contains("Notes%20files/photo%20(2).jpg"), "{b}");
        assert!(b.contains("Notes%20files/photo.jpg"), "the same file is reused");
        assert_eq!(fs::read(out_dir.join("Notes files/photo (2).jpg")).unwrap(), b"two");
        let _ = fs::remove_dir_all(&data);
        let _ = fs::remove_dir_all(&out_dir);
    }

    #[test]
    fn references_cannot_escape_the_folder() {
        assert!(split_ref("12/pic.png").is_some());
        assert!(split_ref("../etc/passwd").is_none());
        assert!(split_ref("12/../../etc").is_none());
        assert!(split_ref("12/.hidden").is_none());
        assert!(split_ref("12/a/b.png").is_none());
        assert!(path(Path::new("/data"), "x/secret.txt").is_none());
    }

    #[test]
    fn odd_names_are_cleaned_up() {
        assert_eq!(clean_file_name("C:\\Users\\me\\Report.pdf"), "Report.pdf");
        assert_eq!(clean_file_name(".bashrc"), "bashrc");
        assert_eq!(clean_file_name(""), "file");
        assert_eq!(clean_file_name("a:b.txt"), "a_b.txt");
        assert!(clean_file_name(&format!("{}.png", "x".repeat(300))).ends_with(".png"));
    }

    #[test]
    fn server_folders_are_named_after_the_note() {
        assert_eq!(remote_folder("Work", 42), "Work/note-42_attachments");
        assert_eq!(note_of_folder("note-42_attachments"), Some(42));
        assert_eq!(note_of_folder("note-x_attachments"), None);
        assert_eq!(note_of_folder("Work"), None);
    }
}
