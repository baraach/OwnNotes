// Cloud sync over WebDAV.
//
//   webdav.rs  PROPFIND / GET / PUT / DELETE against the folder you configure
//   engine.rs  works out what changed on each side and moves files
//
// Everything here runs on background threads; the UI hears back over channels.

use std::time::Duration;

pub mod engine;
mod webdav;

pub use webdav::WebDav;

/// Something went wrong talking to the server. The message is written for the
/// person reading it; `status` holds the HTTP status when there was one.
#[derive(Debug, Clone)]
pub struct CloudError {
    pub message: String,
    pub status: Option<u16>,
}

impl CloudError {
    pub fn new(message: impl Into<String>) -> Self {
        CloudError {
            message: message.into(),
            status: None,
        }
    }
}

pub type CloudResult<T> = Result<T, CloudError>;

pub(crate) fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("OwnNotes/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Turn a ureq error into something readable. WebDAV servers put a short
/// explanation in the body (and Nextcloud puts it in an XML `<s:message>`).
pub(crate) fn describe(err: ureq::Error) -> CloudError {
    match err {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let detail = body_detail(&body);
            let message = match code {
                401 => "The server didn't accept that username and password.".to_string(),
                403 => "The server refused to let this account use that folder.".to_string(),
                404 => "That folder isn't on the server. Check the address in Settings.".to_string(),
                405 => "The server doesn't allow that operation on this folder.".to_string(),
                409 => "The server couldn't create the folder (its parent may be missing).".to_string(),
                507 => "The server is out of space.".to_string(),
                _ => match detail {
                    Some(d) => format!("{d} (HTTP {code})"),
                    None => format!("The server answered HTTP {code}"),
                },
            };
            CloudError {
                message,
                status: Some(code),
            }
        }
        ureq::Error::Transport(t) => CloudError::new(format!("Couldn't reach the server: {t}")),
    }
}

/// Pull a human-readable line out of an error body: the `<s:message>` element
/// Nextcloud/ownCloud send, else the first non-empty line of text.
fn body_detail(body: &str) -> Option<String> {
    if let Some(start) = body.find("<s:message>") {
        let rest = &body[start + "<s:message>".len()..];
        if let Some(end) = rest.find("</s:message>") {
            let text = rest[..end].trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    let line = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('<'))?;
    let line: String = line.chars().take(160).collect();
    Some(line)
}

/// One file in the cloud folder.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteFile {
    pub id: String,
    pub name: String,
    /// A value that changes whenever the content changes (the ETag).
    pub rev: String,
}

/// What the sync engine needs from a provider.
pub trait Remote {
    fn list(&mut self) -> CloudResult<Vec<RemoteFile>>;
    fn download(&mut self, file: &RemoteFile) -> CloudResult<String>;
    /// Write the file. `existing` is the file already known under that name,
    /// if any (WebDAV overwrites in place, so it is only a hint).
    fn upload(&mut self, name: &str, existing: Option<&str>, content: &str) -> CloudResult<RemoteFile>;
    /// Remove the file from the server.
    fn delete(&mut self, id: &str) -> CloudResult<()>;
}

/// The WebDAV password lives in the system keychain (Keychain on macOS,
/// Credential Manager on Windows, the kernel keyring on Linux) — never in
/// settings.json.
pub mod secrets {
    /// Test runs with a throwaway data folder must not touch the real keychain.
    fn entry() -> Option<keyring::Entry> {
        if std::env::var_os("OWNNOTES_DATA_DIR").is_some() {
            return None;
        }
        keyring::Entry::new("OwnNotes", "webdav").ok()
    }

    pub fn load() -> Option<String> {
        entry()?.get_password().ok().filter(|p| !p.is_empty())
    }

    pub fn save(password: &str) -> Result<(), String> {
        match entry() {
            Some(e) => e.set_password(password).map_err(|e| e.to_string()),
            None => Err("the keychain is turned off for this run".into()),
        }
    }

    pub fn forget() {
        if let Some(e) = entry() {
            let _ = e.delete_credential();
        }
    }
}
