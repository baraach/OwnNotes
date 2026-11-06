// WebDAV: the one cloud OwnNotes talks to. Plain HTTP verbs, no OAuth, no
// SDK — just PROPFIND to list the folder, GET to read, PUT to write and
// DELETE to remove. Works with Nextcloud, ownCloud, a NAS, Apache mod_dav or
// anything else that speaks RFC 4918.
//
// The configured address is a *collection*; every note is a file directly
// inside it, so the folder stays readable in the server's own web UI.

use base64::Engine as _;
use std::collections::HashSet;
use std::io::Read as _;
use quick_xml::events::Event;
use quick_xml::Reader;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};

use super::{agent, describe, CloudError, CloudResult, Remote, RemoteFile};

/// Characters that are not safe inside a single URL path segment.
const UNSAFE_IN_SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'%')
    .add(b'/')
    .add(b'\\');

/// The folder OwnNotes keeps its notes in. It is created inside whatever
/// address the person typed, so they only have to give the server.
pub const NOTES_FOLDER: &str = "OwnNotes";

/// A note is text; refuse anything absurd rather than filling memory.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

const PROPFIND_BODY: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:">
  <d:prop>
    <d:resourcetype/>
    <d:getetag/>
    <d:getlastmodified/>
  </d:prop>
</d:propfind>"#;

pub struct WebDav {
    agent: ureq::Agent,
    /// The collection URL, always ending in `/`.
    base: String,
    auth: String,
    /// Notebook folders already created this session.
    made_folders: HashSet<String>,
    /// Notebook folders that lost a file this sync, checked for emptiness at
    /// the end so a renamed workspace doesn't leave an empty folder behind.
    emptied: HashSet<String>,
}

impl WebDav {
    /// Build a client from what the user typed in Settings.
    pub fn new(url: &str, user: &str, password: &str) -> CloudResult<Self> {
        let base = normalise_url(url)?;
        let auth = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
        );
        Ok(WebDav {
            agent: agent(),
            base,
            auth,
            made_folders: HashSet::new(),
            emptied: HashSet::new(),
        })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// Delete notebook folders that this sync emptied. Best effort: a folder
    /// that still has anything in it is left alone, and when the server gives
    /// us an ETag for the folder we only delete it if it hasn't changed since
    /// we looked — so a note added meanwhile can't be swept away.
    pub fn prune_empty_folders(&mut self) {
        let folders: Vec<String> = self.emptied.drain().collect();
        for folder in folders {
            // A workspace may sit inside `Versions/`; after removing it, the
            // parent can be empty too. Walk up until something is left.
            let mut current = Some(folder);
            while let Some(folder) = current {
                let url = format!("{}/", self.file_url(&folder));
                let Ok(entries) = self.propfind(&url, 1) else {
                    break;
                };
                let mut children = 0;
                let mut etag = None;
                for entry in &entries {
                    match self.relative(&entry.href) {
                        Some(name) if name == folder => etag = entry.etag.clone(),
                        Some(_) => children += 1,
                        None => {}
                    }
                }
                if children > 0 {
                    break;
                }
                let mut request = self
                    .agent
                    .request("DELETE", &url)
                    .set("Authorization", &self.auth);
                if let Some(etag) = etag {
                    request = request.set("If-Match", &etag);
                }
                if request.call().is_err() {
                    break;
                }
                self.made_folders.remove(&folder);
                current = folder.rsplit_once('/').map(|(parent, _)| parent.to_string());
            }
        }
    }

    /// Check the address, credentials and folder without moving any notes.
    pub fn check(&mut self) -> CloudResult<()> {
        self.ensure_collection()
    }

    fn file_url(&self, name: &str) -> String {
        let mut url = self.base.clone();
        let mut first = true;
        for segment in name.split('/').filter(|s| !s.is_empty()) {
            if !first {
                url.push('/');
            }
            url.push_str(&utf8_percent_encode(segment, UNSAFE_IN_SEGMENT).to_string());
            first = false;
        }
        url
    }

    /// The path of `href` relative to the collection, decoded.
    fn relative(&self, href: &str) -> Option<String> {
        let base = url::Url::parse(&self.base).ok()?;
        relative_path(href, base.path())
    }

    /// Create a notebook's folder on the server the first time a note needs it.
    fn ensure_folder(&mut self, folder: &str) -> CloudResult<()> {
        let mut path = String::new();
        for segment in folder.split('/').filter(|s| !s.is_empty()) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(segment);
            if self.made_folders.contains(&path) {
                continue;
            }
            let url = format!("{}/", self.file_url(&path));
            match self.call("MKCOL", &url, None, None) {
                // 405 = it was already there.
                Ok(_) => {}
                Err(e) if e.status == Some(405) => {}
                Err(e) => {
                    return Err(CloudError {
                        message: format!("Couldn't create the “{path}” folder: {}", e.message),
                        status: e.status,
                    })
                }
            }
            self.made_folders.insert(path.clone());
        }
        Ok(())
    }

    fn call(
        &self,
        method: &str,
        url: &str,
        depth: Option<u8>,
        body: Option<(&str, &str)>,
    ) -> CloudResult<ureq::Response> {
        let mut req = self
            .agent
            .request(method, url)
            .set("Authorization", &self.auth);
        if let Some(depth) = depth {
            req = req.set("Depth", &depth.to_string());
        }
        let result = match body {
            Some((mime, text)) => req.set("Content-Type", mime).send_string(text),
            None => req.call(),
        };
        result.map_err(describe)
    }

    fn propfind(&mut self, url: &str, depth: u8) -> CloudResult<Vec<Entry>> {
        let resp = self.call(
            "PROPFIND",
            url,
            Some(depth),
            Some(("application/xml; charset=utf-8", PROPFIND_BODY)),
        )?;
        let text = read_text(resp)?;
        parse_propfind(&text)
    }

    /// Create the collection if it isn't there yet.
    fn ensure_collection(&mut self) -> CloudResult<()> {
        let base = self.base.clone();
        match self.propfind(&base, 0) {
            Ok(_) => Ok(()),
            Err(e) if e.status == Some(404) => {
                match self.call("MKCOL", &self.base, None, None) {
                    Ok(_) => Ok(()),
                    // 405 usually means it exists after all (or the parent is
                    // missing on servers that answer 409).
                    Err(e) if e.status == Some(405) => Ok(()),
                    Err(e) => Err(CloudError {
                        message: format!(
                            "The folder couldn't be created: {}",
                            e.message
                        ),
                        status: e.status,
                    }),
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Every attachment on the server, as (notebook folder, note id, file
    /// name). They live in `<Notebook>/note-<id>_attachments/`.
    pub fn list_attachments(&mut self) -> CloudResult<Vec<(String, u64, String)>> {
        let base = self.base.clone();
        let top = match self.propfind(&base, 1) {
            Ok(entries) => entries,
            Err(e) if e.status == Some(404) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut out = Vec::new();
        for notebook in top.iter().filter(|e| e.collection) {
            let Some(workspace) = self.relative(&notebook.href) else { continue };
            if workspace.contains('/') {
                continue;
            }
            let url = format!("{}/", self.file_url(&workspace));
            for folder in self.propfind(&url, 1)?.into_iter().filter(|e| e.collection) {
                let Some(rel) = self.relative(&folder.href) else { continue };
                let Some((_, last)) = rel.rsplit_once('/') else { continue };
                let Some(id) = crate::attachments::note_of_folder(last) else { continue };
                let url = format!("{}/", self.file_url(&rel));
                let prefix = format!("{rel}/");
                for file in self.propfind(&url, 1)?.into_iter().filter(|e| !e.collection) {
                    if let Some(name) = self
                        .relative(&file.href)
                        .and_then(|n| n.strip_prefix(&prefix).map(str::to_string))
                    {
                        if !name.contains('/') {
                            out.push((workspace.clone(), id, name));
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// Delete one file by its path in the collection; already gone is fine.
    pub fn delete_path(&mut self, name: &str) -> CloudResult<()> {
        match self.call("DELETE", &self.file_url(name), None, None) {
            Ok(_) => Ok(()),
            Err(e) if e.status == Some(404) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Remove a folder if nothing is left in it. Best effort.
    pub fn delete_folder_if_empty(&mut self, folder: &str) {
        let url = format!("{}/", self.file_url(folder));
        let Ok(entries) = self.propfind(&url, 1) else { return };
        let children = entries
            .iter()
            .filter(|e| self.relative(&e.href).is_some_and(|n| n != folder))
            .count();
        if children == 0 {
            let _ = self.call("DELETE", &url, None, None);
        }
    }

    /// Write a binary file (an attachment), creating its folder if needed.
    pub fn put_bytes(&mut self, name: &str, mime: &str, bytes: &[u8]) -> CloudResult<()> {
        if let Some((folder, _)) = name.rsplit_once('/') {
            self.ensure_folder(folder)?;
        }
        self.agent
            .request("PUT", &self.file_url(name))
            .set("Authorization", &self.auth)
            .set("Content-Type", mime)
            .send_bytes(bytes)
            .map(drop)
            .map_err(describe)
    }

    /// Read a binary file (an attachment), refusing anything over `limit`.
    pub fn get_bytes(&mut self, name: &str, limit: usize) -> CloudResult<Vec<u8>> {
        let resp = self.call("GET", &self.file_url(name), None, None)?;
        let mut bytes = Vec::new();
        resp.into_reader()
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| CloudError::new(format!("Couldn't download {name}: {e}")))?;
        if bytes.len() > limit {
            return Err(CloudError::new(format!("{name} is too large to download")));
        }
        Ok(bytes)
    }

    /// The ETag of one file, for after an upload that didn't return one.
    fn stat(&mut self, name: &str) -> CloudResult<String> {
        let url = self.file_url(name);
        let entries = self.propfind(&url, 0)?;
        Ok(entries
            .into_iter()
            .next()
            .map(|entry| entry.revision())
            .unwrap_or_default())
    }

    /// List the syncable files inside a top-level `folder` on the server.
    ///
    /// Only two kinds of subfolder are walked into, one level deep: a
    /// workspace's trash (`<Workspace>/.deleted/`) and the per-workspace
    /// folders under `Versions/`. Anything else — above all the
    /// `note-<id>_attachments/` folders — is left alone, because the sync
    /// only looks at a file's last path segment to decide whether it is a
    /// note, and an attachment called `note-5.md` must never pass for one.
    fn collect_files(
        &mut self,
        folder: &str,
        files: &mut Vec<RemoteFile>,
        walk_into: bool,
    ) -> CloudResult<()> {
        let url = format!("{}/", self.file_url(folder));
        let in_versions = folder.eq_ignore_ascii_case(crate::store::VERSIONS_FOLDER);
        for entry in self.propfind(&url, 1)? {
            let Some(name) = self.relative(&entry.href) else { continue };
            // PROPFIND lists the collection itself first; walking into that
            // would come back to this same folder for ever.
            if name == folder {
                continue;
            }
            if entry.collection {
                let last = name.rsplit('/').next().unwrap_or(&name);
                if walk_into && (in_versions || last == crate::store::TRASH_FOLDER) {
                    self.collect_files(&name, files, false)?;
                }
            } else if crate::cloud::engine::syncable(&name) {
                files.push(RemoteFile {
                    id: name.clone(),
                    name,
                    rev: entry.revision(),
                });
            }
        }
        Ok(())
    }
}

impl Remote for WebDav {
    fn list(&mut self) -> CloudResult<Vec<RemoteFile>> {
        let base = self.base.clone();
        let entries = match self.propfind(&base, 1) {
            Ok(entries) => entries,
            Err(e) if e.status == Some(404) => {
                self.ensure_collection()?;
                self.propfind(&base, 1)?
            }
            Err(e) => return Err(e),
        };

        let mut files = Vec::new();
        for entry in entries {
            if entry.collection {
                // A workspace folder (with its `.deleted` trash) or
                // `Versions` (one folder per workspace).
                let Some(folder) = self.relative(&entry.href) else {
                    continue;
                };
                self.collect_files(&folder, &mut files, true)?;
            } else if let Some(name) = self.relative(&entry.href) {
                // Notes at the top level (older versions kept them there).
                if crate::cloud::engine::syncable(&name) {
                    files.push(RemoteFile {
                        id: name.clone(),
                        name,
                        rev: entry.revision(),
                    });
                }
            }
        }
        Ok(files)
    }

    fn download(&mut self, file: &RemoteFile) -> CloudResult<String> {
        let resp = self.call("GET", &self.file_url(&file.name), None, None)?;
        read_text(resp)
    }

    fn upload(&mut self, name: &str, _existing: Option<&str>, content: &str) -> CloudResult<RemoteFile> {
        if let Some((folder, _)) = name.rsplit_once('/') {
            self.ensure_folder(folder)?;
        }
        // WebDAV writes in place: PUT to the same path replaces the file.
        let resp = self.call(
            "PUT",
            &self.file_url(name),
            None,
            Some(("text/markdown; charset=utf-8", content)),
        )?;
        let etag = resp.header("ETag").map(str::to_string);
        drop(resp);
        // Not every server returns the new ETag; ask for it rather than
        // recording a guess, or the next sync would re-download our own file.
        let rev = match etag {
            Some(etag) => etag,
            None => self.stat(name).unwrap_or_default(),
        };
        Ok(RemoteFile {
            id: name.to_string(),
            name: name.to_string(),
            rev,
        })
    }

    fn delete(&mut self, id: &str) -> CloudResult<()> {
        let result = match self.call("DELETE", &self.file_url(id), None, None) {
            Ok(_) => Ok(()),
            // Already gone is the outcome we wanted.
            Err(e) if e.status == Some(404) => Ok(()),
            Err(e) => Err(e),
        };
        if result.is_ok() {
            if let Some((folder, _)) = id.rsplit_once('/') {
                self.emptied.insert(folder.to_string());
            }
        }
        result
    }
}

fn push_text(entry: &mut Entry, field: Field, text: &str) {
    match field {
        Field::Href => entry.href.push_str(text),
        Field::Etag => entry.etag.get_or_insert_with(String::new).push_str(text),
        Field::Modified => entry
            .last_modified
            .get_or_insert_with(String::new)
            .push_str(text),
        Field::None => {}
    }
}

/// Text content of an XML element, with entities like `&quot;` resolved.
fn unescape(text: &str) -> String {
    match quick_xml::escape::unescape(text) {
        Ok(text) => text.into_owned(),
        Err(_) => text.to_string(),
    }
}

fn read_text(resp: ureq::Response) -> CloudResult<String> {
    let mut out = String::new();
    resp.into_reader()
        .take(MAX_FILE_BYTES + 1)
        .read_to_string(&mut out)
        .map_err(|e| super::CloudError::new(format!("The download failed: {e}")))?;
    // Cutting it off would save a shortened note and later upload it over
    // the full one.
    if out.len() as u64 > MAX_FILE_BYTES {
        return Err(super::CloudError::new(format!(
            "A file on the server is larger than {} MB and was skipped",
            MAX_FILE_BYTES / 1024 / 1024
        )));
    }
    Ok(out)
}

/// Accept `host/notes`, `https://host/notes` and `https://host/notes/`, and
/// produce a collection URL that always ends in `/`.
pub fn normalise_url(url: &str) -> CloudResult<String> {
    let url = url.trim();
    if url.is_empty() {
        return Err(CloudError::new("Add the address of your WebDAV server"));
    }
    let with_scheme = if url.contains("://") {
        url.to_string()
    } else {
        format!("https://{url}")
    };
    let parsed = url::Url::parse(&with_scheme)
        .map_err(|_| CloudError::new("That doesn't look like a web address"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(CloudError::new("The address must start with http:// or https://"));
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(CloudError::new("The address is missing a server name"));
    }
    if !parsed.username().is_empty() {
        return Err(CloudError::new(
            "The address should not contain a username. Enter it separately in the username field.",
        ));
    }
    let mut base = parsed.to_string();
    // `Url` drops a trailing slash when the path is empty but keeps it
    // otherwise; make sure we always address a collection, not a file.
    if !base.ends_with('/') {
        base.push('/');
    }
    // The person gives the server (or their own notes folder); the notes live
    // in an `OwnNotes` folder inside it, which is created on first sync.
    let last = base
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or_default();
    if !last.eq_ignore_ascii_case(NOTES_FOLDER) {
        base.push_str(NOTES_FOLDER);
        base.push('/');
    }
    Ok(base)
}

/// The path of a PROPFIND `href` relative to the collection, percent-decoded.
/// Servers answer with absolute paths, full URLs, or both.
fn relative_path(href: &str, base_path: &str) -> Option<String> {
    let href = href.split(['?', '#']).next().unwrap_or(href).trim();
    let path = if href.starts_with("http://") || href.starts_with("https://") {
        url::Url::parse(href).ok()?.path().to_string()
    } else {
        href.to_string()
    };
    let decoded = percent_decode_str(&path).decode_utf8_lossy().into_owned();
    let base = percent_decode_str(base_path).decode_utf8_lossy().into_owned();
    let rel = decoded.strip_prefix(&base)?;
    let rel = rel.trim_end_matches('/');
    if rel.is_empty() {
        None
    } else {
        Some(rel.to_string())
    }
}

/// One `<d:response>` from a PROPFIND answer.
#[derive(Debug, Default, Clone)]
pub struct Entry {
    pub href: String,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub collection: bool,
}

impl Entry {
    pub fn revision(&self) -> String {
        self.etag
            .clone()
            .or_else(|| self.last_modified.clone())
            .unwrap_or_default()
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Field {
    None,
    Href,
    Etag,
    Modified,
}

/// Parse a multistatus body. Namespace prefixes vary by server (`d:`, `D:`,
/// `lp1:`, none at all), so elements are matched on their local name.
pub fn parse_propfind(xml: &str) -> CloudResult<Vec<Entry>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut entries: Vec<Entry> = Vec::new();
    let mut current: Option<Entry> = None;
    let mut field = Field::None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => match e.local_name().as_ref() {
                b"response" => current = Some(Entry::default()),
                b"href" => field = Field::Href,
                b"getetag" => field = Field::Etag,
                b"getlastmodified" => field = Field::Modified,
                b"collection" => {
                    if let Some(entry) = current.as_mut() {
                        entry.collection = true;
                    }
                }
                _ => {}
            },
            Ok(Event::Empty(e)) => {
                if e.local_name().as_ref() == b"collection" {
                    if let Some(entry) = current.as_mut() {
                        entry.collection = true;
                    }
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(entry) = current.as_mut() {
                    push_text(entry, field, &t.decode().unwrap_or_default());
                }
            }
            Ok(Event::CData(t)) => {
                if let Some(entry) = current.as_mut() {
                    push_text(entry, field, &t.decode().unwrap_or_default());
                }
            }
            // `&quot;` and friends arrive as their own events; keep the raw
            // form and unescape the finished value below.
            Ok(Event::GeneralRef(r)) => {
                if let Some(entry) = current.as_mut() {
                    let name = r.decode().unwrap_or_default();
                    push_text(entry, field, &format!("&{name};"));
                }
            }
            Ok(Event::End(e)) => {
                match e.local_name().as_ref() {
                    b"href" | b"getetag" | b"getlastmodified" => {
                        if let Some(entry) = current.as_mut() {
                            match field {
                                Field::Href => entry.href = unescape(&entry.href),
                                Field::Etag => {
                                    entry.etag = entry.etag.as_deref().map(unescape);
                                }
                                Field::Modified => {
                                    entry.last_modified =
                                        entry.last_modified.as_deref().map(unescape);
                                }
                                Field::None => {}
                            }
                        }
                        field = Field::None;
                    }
                    b"response" => {
                        if let Some(entry) = current.take() {
                            entries.push(entry);
                        }
                        field = Field::None;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                return Err(CloudError::new(format!(
                    "The server's file list couldn't be read ({e})"
                )))
            }
            _ => {}
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    #[test]
    fn accepts_addresses_the_way_people_type_them() {
        // Only the server has to be given; the notes folder is added.
        assert_eq!(
            normalise_url("cloud.example.com").unwrap(),
            "https://cloud.example.com/OwnNotes/"
        );
        assert_eq!(
            normalise_url("https://cloud.example.com/remote.php/dav/files/me/").unwrap(),
            "https://cloud.example.com/remote.php/dav/files/me/OwnNotes/"
        );
        assert_eq!(
            normalise_url("  http://192.168.1.9:8080/dav  ").unwrap(),
            "http://192.168.1.9:8080/dav/OwnNotes/"
        );
        // A full path to the notes folder is used as is, not nested twice.
        assert_eq!(
            normalise_url("https://cloud.example.com/remote.php/dav/files/me/OwnNotes/").unwrap(),
            "https://cloud.example.com/remote.php/dav/files/me/OwnNotes/"
        );
        assert_eq!(
            normalise_url("https://cloud.example.com/dav/ownnotes").unwrap(),
            "https://cloud.example.com/dav/ownnotes/"
        );
        assert!(normalise_url("").is_err());
        assert!(normalise_url("   ").is_err());
        assert!(normalise_url("ftp://host/notes").is_err());
        assert!(normalise_url("not a url").is_err());
    }

    #[test]
    fn reads_nextcloud_style_multistatus() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:s="http://sabredav.org/ns">
 <d:response>
  <d:href>/remote.php/dav/files/me/OwnNotes/</d:href>
  <d:propstat>
   <d:prop><d:resourcetype><d:collection/></d:resourcetype></d:prop>
   <d:status>HTTP/1.1 200 OK</d:status>
  </d:propstat>
 </d:response>
 <d:response>
  <d:href>/remote.php/dav/files/me/OwnNotes/note-1.md</d:href>
  <d:propstat>
   <d:prop>
    <d:resourcetype/>
    <d:getetag>&quot;abc123&quot;</d:getetag>
    <d:getlastmodified>Mon, 03 Nov 2026 10:00:00 GMT</d:getlastmodified>
   </d:prop>
   <d:status>HTTP/1.1 200 OK</d:status>
  </d:propstat>
 </d:response>
 <d:response>
  <d:href>/remote.php/dav/files/me/OwnNotes/my%20note.md</d:href>
  <d:propstat><d:prop><d:getetag>"def"</d:getetag></d:prop></d:propstat>
 </d:response>
</d:multistatus>"#;
        let entries = parse_propfind(xml).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(entries[0].collection);
        assert_eq!(relative_path(&entries[1].href, "/remote.php/dav/files/me/OwnNotes/"), Some("note-1.md".to_string()));
        assert_eq!(entries[1].etag.as_deref(), Some("\"abc123\""));
        assert_eq!(entries[1].revision(), "\"abc123\"");
        assert_eq!(relative_path(&entries[2].href, "/remote.php/dav/files/me/OwnNotes/"), Some("my note.md".to_string()));
    }

    #[test]
    fn reads_a_server_that_omits_prefixes_and_etags() {
        let xml = r#"<multistatus xmlns="DAV:">
 <response>
  <href>/dav/notes/note-9.md</href>
  <propstat><prop><getlastmodified>Tue, 04 Nov 2026 09:30:00 GMT</getlastmodified></prop></propstat>
 </response>
</multistatus>"#;
        let entries = parse_propfind(xml).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(relative_path(&entries[0].href, "/dav/notes/"), Some("note-9.md".to_string()));
        assert!(!entries[0].collection);
        // Falls back to the modification date when there is no ETag.
        assert_eq!(entries[0].revision(), "Tue, 04 Nov 2026 09:30:00 GMT");
    }

    #[test]
    fn a_broken_multistatus_is_an_error_not_a_panic() {
        assert!(parse_propfind("<d:multistatus><d:response></d:wrong></d:multistatus>").is_err());
    }

    // ---- a tiny WebDAV server, to exercise the real HTTP layer ----

    #[derive(Default, Debug, Clone)]
    struct Seen {
        method: String,
        path: String,
        depth: Option<String>,
        auth: Option<String>,
        body: String,
    }

    /// Serve exactly `rounds` requests, answering by method, and record them.
    /// `nested` makes the folder list a notebook folder with a note inside.
    fn serve(rounds: usize, nested: bool) -> (u16, Arc<Mutex<Vec<Seen>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for _ in 0..rounds {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let request = read_request(&mut stream);
                let (status, headers, body) = answer(&request, nested);
                let _ = write_response(&mut stream, status, &headers, &body);
                log.lock().unwrap().push(request);
            }
        });
        (port, seen)
    }

    fn read_request(stream: &mut std::net::TcpStream) -> Seen {
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        let header_end = loop {
            if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break i + 4;
            }
            let n = stream.read(&mut chunk).unwrap();
            if n == 0 {
                break buf.len();
            }
            buf.extend_from_slice(&chunk[..n]);
        };
        let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
        let mut body = buf[header_end..].to_vec();
        let mut lines = head.lines();
        let request_line = lines.next().unwrap_or_default();
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        let mut depth = None;
        let mut auth = None;
        let mut length = 0usize;
        for line in lines {
            let Some((k, v)) = line.split_once(':') else {
                continue;
            };
            match k.to_ascii_lowercase().as_str() {
                "depth" => depth = Some(v.trim().to_string()),
                "authorization" => auth = Some(v.trim().to_string()),
                "content-length" => length = v.trim().parse().unwrap_or(0),
                _ => {}
            }
        }
        while body.len() < length {
            let n = stream.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        body.truncate(length);
        Seen {
            method,
            path,
            depth,
            auth,
            body: String::from_utf8_lossy(&body).into_owned(),
        }
    }

    fn answer(req: &Seen, nested: bool) -> (u16, Vec<(&'static str, String)>, String) {
        let entry = |href: &str, collection: bool, etag: &str| {
            let kind = if collection {
                "<d:resourcetype><d:collection/></d:resourcetype>".to_string()
            } else {
                format!("<d:resourcetype/><d:getetag>&quot;{etag}&quot;</d:getetag>")
            };
            format!(
                "<d:response><d:href>{href}</d:href><d:propstat><d:prop>{kind}</d:prop>\
                 </d:propstat></d:response>"
            )
        };
        match req.method.as_str() {
            "PROPFIND" => {
                let dir = req.path.clone();
                let mut entries = Vec::new();
                if dir.ends_with(".md") {
                    entries.push(entry(&dir, false, "e-1"));
                } else if nested && dir.ends_with("/Personal/") {
                    entries.push(entry(&format!("{dir}note-2.md"), false, "e-2"));
                } else {
                    if nested {
                        entries.push(entry(&format!("{dir}Personal/"), true, ""));
                    }
                    entries.push(entry(&format!("{dir}note-1.md"), false, "e-1"));
                    entries.push(entry(&format!("{dir}other.jpg"), false, "x"));
                }
                let payload = format!(
                    "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\">{}</d:multistatus>",
                    entries.join("")
                );
                (207, vec![("Content-Type", "application/xml".into())], payload)
            }
            "GET" => (200, vec![("Content-Type", "text/markdown".into())], "hello from the server".into()),
            "MKCOL" => (201, vec![], String::new()),
            "PUT" => (201, vec![("ETag", "\"e-2\"".into())], String::new()),
            "DELETE" => (204, vec![], String::new()),
            _ => (405, vec![], String::new()),
        }
    }

    fn write_response(
        stream: &mut std::net::TcpStream,
        status: u16,
        headers: &[(&str, String)],
        body: &str,
    ) -> std::io::Result<()> {
        let reason = match status {
            200 => "OK",
            201 => "Created",
            204 => "No Content",
            207 => "Multi-Status",
            _ => "Error",
        };
        let mut head = format!("HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str("\r\n");
        stream.write_all(head.as_bytes())?;
        stream.write_all(body.as_bytes())?;
        stream.flush()
    }

    #[test]
    fn reads_a_folder_per_notebook() {
        let (port, _seen) = serve(2, true);
        let mut dav = WebDav::new(&format!("http://127.0.0.1:{port}/dav/notes"), "me", "pw").unwrap();
        let mut files = dav.list().unwrap();
        files.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "Personal/note-2.md", "the note inside the folder");
        assert_eq!(files[1].name, "note-1.md", "a note still at the top level");
    }

    #[test]
    fn a_folder_that_still_has_notes_is_left_alone() {
        let (port, seen) = serve(2, true);
        let mut dav = WebDav::new(&format!("http://127.0.0.1:{port}/dav/notes"), "me", "pw").unwrap();
        dav.delete("Personal/note-1.md").unwrap();
        dav.prune_empty_folders();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "no folder DELETE when a note is still inside");
        assert_eq!(seen[1].method, "PROPFIND");
    }

    #[test]
    fn creates_the_notebook_folder_before_uploading_into_it() {
        let (port, seen) = serve(2, false);
        let mut dav = WebDav::new(&format!("http://127.0.0.1:{port}/dav/notes"), "me", "pw").unwrap();
        dav.upload("Personal/note-3.md", None, "text").unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0].method, "MKCOL");
        assert_eq!(seen[0].path, "/dav/notes/OwnNotes/Personal/");
        assert_eq!(seen[1].method, "PUT");
        assert_eq!(seen[1].path, "/dav/notes/OwnNotes/Personal/note-3.md");
        assert_eq!(seen[1].body, "text");
    }

    #[test]
    fn talks_to_a_webdav_folder() {
        // PROPFIND list, GET, PUT, PROPFIND stat (server omits ETag? it does
        // return one, so no stat), DELETE.
        let (port, seen) = serve(4, false);
        let mut dav =
            WebDav::new(&format!("http://127.0.0.1:{port}/dav/notes"), "tester", "hunter2").unwrap();
        assert_eq!(dav.base(), format!("http://127.0.0.1:{port}/dav/notes/OwnNotes/"));

        let files = dav.list().unwrap();
        assert_eq!(files.len(), 1, "only note-*.md counts, the .jpg is skipped");
        assert_eq!(files[0].name, "note-1.md");
        assert_eq!(files[0].rev, "\"e-1\"");

        let text = dav.download(&files[0]).unwrap();
        assert_eq!(text, "hello from the server");

        let uploaded = dav.upload("note-2.md", None, "new content").unwrap();
        assert_eq!(uploaded.rev, "\"e-2\"");

        dav.delete("note-1.md").unwrap();

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 4);
        assert_eq!(seen[0].method, "PROPFIND");
        assert_eq!(seen[0].path, "/dav/notes/OwnNotes/");
        assert_eq!(seen[0].depth.as_deref(), Some("1"));
        assert_eq!(seen[1].method, "GET");
        assert_eq!(seen[1].path, "/dav/notes/OwnNotes/note-1.md");
        assert_eq!(seen[2].method, "PUT");
        assert_eq!(seen[2].body, "new content");
        assert_eq!(seen[3].method, "DELETE");
        // The folder URL is normalised and Basic auth is sent on every call.
        let expected = format!("Basic {}", base64::engine::general_purpose::STANDARD.encode("tester:hunter2"));
        assert!(seen.iter().all(|r| r.auth.as_deref() == Some(expected.as_str())));
    }
}
