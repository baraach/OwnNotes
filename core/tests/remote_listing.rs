// What the WebDAV listing finds: every note, however deep, and only once.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use ownnotes_core::cloud::{Remote, WebDav};

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(root: &Path) -> Server {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/webdav_test_server.py");
    let mut child = Command::new("python3")
        .arg(script)
        .arg("--root")
        .arg(root)
        .arg("--log")
        .arg(root.join("requests.log"))
        .stdout(Stdio::piped())
        .spawn()
        .expect("the test server needs python3");
    let stdout = child.stdout.take().expect("no stdout");
    let mut line = String::new();
    BufReader::new(stdout).read_line(&mut line).expect("no port");
    let port = line.trim().parse().expect("a port number");
    Server { child, port }
}

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ownnotes-list-it-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn trashed_notes_and_versions_are_listed_once_each() {
    let root = temp("nested");
    let server_root = root.join("server");
    let server = start(&server_root);

    // The server's side of a synced library: a note, a trashed note one
    // level deeper, and an old version two levels deep.
    let dav = server_root.join("OwnNotes");
    for folder in ["Work/.deleted", "Versions/Work", "Work/note-1_attachments"] {
        std::fs::create_dir_all(dav.join(folder)).unwrap();
    }
    std::fs::write(dav.join("Work/note-1.html"), "<p>one</p>").unwrap();
    std::fs::write(dav.join("Work/.deleted/note-2.html"), "<p>two</p>").unwrap();
    std::fs::write(dav.join("Versions/Work/note-1-1700000000000.md"), "old").unwrap();
    // An attachment that happens to be named like a note. It is not one: if
    // it were listed, the sync would download it as note 5 and then delete
    // it from the server as "deleted locally".
    std::fs::write(dav.join("Work/note-1_attachments/note-5.md"), "attached").unwrap();

    let mut remote = WebDav::new(
        &format!("http://127.0.0.1:{}/dav/", server.port),
        "me",
        "pw",
    )
    .unwrap();

    // A traversal that walked into a folder's own entry would never return;
    // finishing at all is half the test.
    let listed = remote.list().unwrap();
    let mut names: Vec<&str> = listed.iter().map(|f| f.name.as_str()).collect();
    names.sort();

    assert_eq!(
        names,
        [
            "Versions/Work/note-1-1700000000000.md",
            "Work/.deleted/note-2.html",
            "Work/note-1.html",
        ],
        "every file, once, at whatever depth it sits"
    );
}
