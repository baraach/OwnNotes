// Sync settings: where else your notes are kept.
//
// OwnNotes has one cloud option: a WebDAV server you control (Nextcloud,
// ownCloud, a NAS, Apache mod_dav, …). The notes live in a folder on that
// server, in the same plain-Markdown format used on this computer. Nothing is
// sent anywhere else, and there is no account with us to sign in to.
//
// The password is kept in the system keychain, never in settings.json.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum SyncBackend {
    /// Notes stay only on this device. (Older settings files said "Folder",
    /// "ICloud", "GoogleDrive" or "OneDrive" for options that no longer exist.)
    #[serde(
        alias = "Folder",
        alias = "ICloud",
        alias = "GoogleDrive",
        alias = "OneDrive"
    )]
    Off,
    /// Notes are copied to a WebDAV folder you run or rent.
    WebDav,
}

/// How the very first sync with a folder is settled. Asked once, when the
/// server is added, and remembered in settings.json afterwards.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum FirstSync {
    /// Not answered yet: show the question on first contact.
    #[default]
    Ask,
    /// Keep every note from both sides.
    Merge,
    /// Keep local: this computer's notes replace the folder's.
    #[serde(alias = "OverwriteServer")]
    KeepLocal,
    /// Keep server: the folder's notes replace this computer's.
    #[serde(alias = "OverwriteComputer")]
    KeepServer,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncSettings {
    pub backend: SyncBackend,
    /// The collection to keep the notes in, e.g.
    /// `https://cloud.example.com/remote.php/dav/files/me/OwnNotes/`.
    pub webdav_url: String,
    pub webdav_user: String,
    /// Never saved to settings.json; kept in the system keychain.
    #[serde(skip)]
    pub webdav_password: String,
    /// The answer given the first time this server was used.
    pub first_sync: FirstSync,
    /// The server + folder that answer belongs to; a different address asks again.
    pub first_sync_for: String,
    /// Upload as soon as a note changes.
    pub sync_on_change: bool,
    /// How often to check for changes made on other devices, in minutes.
    /// 0 = only on launch, when a note changes, and when I press Sync now.
    /// WebDAV has no push, so the other side's edits are found by this check.
    pub auto_sync_minutes: u32,
}

impl Default for SyncSettings {
    fn default() -> Self {
        SyncSettings {
            backend: SyncBackend::Off,
            webdav_url: String::new(),
            webdav_user: String::new(),
            webdav_password: String::new(),
            first_sync: FirstSync::Ask,
            first_sync_for: String::new(),
            sync_on_change: true,
            auto_sync_minutes: 1,
        }
    }
}

impl SyncSettings {
    /// True when WebDAV is chosen and has enough to connect.
    pub fn active(&self) -> bool {
        self.backend == SyncBackend::WebDav
            && !self.webdav_url.trim().is_empty()
            && !self.webdav_user.trim().is_empty()
    }

    /// True when the password still has to be typed in (nothing in the
    /// keychain and nothing entered this session).
    pub fn needs_password(&self) -> bool {
        self.active() && self.webdav_password.is_empty()
    }

    /// Compare everything that is written to disk, ignoring the password
    /// (which lives in the keychain, not in settings.json).
    pub fn same_as_saved(&self, other: &Self) -> bool {
        self.backend == other.backend
            && self.webdav_url == other.webdav_url
            && self.webdav_user == other.webdav_user
            && self.first_sync == other.first_sync
            && self.first_sync_for == other.first_sync_for
            && self.sync_on_change == other.sync_on_change
            && self.auto_sync_minutes == other.auto_sync_minutes
    }
}
