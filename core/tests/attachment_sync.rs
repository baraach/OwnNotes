// Attachments, end to end, against the real test server.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use ownnotes_core::attachments;
use ownnotes_core::cloud::WebDav;

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
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../tools/webdav_test_server.py");
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
    let dir = std::env::temp_dir().join(format!(
        "ownnotes-attach-it-{tag}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn files_travel_both_ways_and_follow_deletions() {
    let root = temp("sync");
    let server_root = root.join("server");
    let data = root.join("data");
    let server = start(&server_root);
    let mut remote = WebDav::new(
        &format!("http://127.0.0.1:{}/dav/", server.port),
        "me",
        "pw",
    )
    .unwrap();

    attachments::save(&data, 7, "pic.jpg", b"one").unwrap();
    attachments::save(&data, 7, "notes.pdf", b"two").unwrap();
    attachments::save(&data, 9, "other.png", b"three").unwrap();

    let folders = HashMap::from([(7u64, "Personal".to_string()), (9u64, "Work".to_string())]);

    // Up.
    let moved = attachments::sync(&mut remote, &data, &folders, &HashSet::new()).unwrap();
    assert_eq!(moved.uploaded, 3, "every file goes up once");
    assert_eq!(moved.downloaded, 0);
    assert!(server_root
        .join("OwnNotes/Personal/note-7_attachments/pic.jpg")
        .is_file());
    assert!(server_root
        .join("OwnNotes/Work/note-9_attachments/other.png")
        .is_file());

    // Nothing to do the second time.
    let moved = attachments::sync(&mut remote, &data, &folders, &HashSet::new()).unwrap();
    assert_eq!((moved.uploaded, moved.downloaded), (0, 0));

    // Down, after losing the local copies.
    std::fs::remove_dir_all(attachments::dir(&data)).unwrap();
    let moved = attachments::sync(&mut remote, &data, &folders, &HashSet::new()).unwrap();
    assert_eq!(moved.downloaded, 3, "everything comes back");
    assert_eq!(
        std::fs::read(attachments::dir(&data).join("7/pic.jpg")).unwrap(),
        b"one"
    );

    // A note moved to another notebook takes its files with it.
    let moved_to_work = HashMap::from([(7u64, "Work".to_string()), (9u64, "Work".to_string())]);
    let moved = attachments::sync(&mut remote, &data, &moved_to_work, &HashSet::new()).unwrap();
    assert_eq!(moved.uploaded, 2, "the moved note's files go up again");
    assert!(server_root
        .join("OwnNotes/Work/note-7_attachments/pic.jpg")
        .is_file());
    assert!(
        !server_root
            .join("OwnNotes/Personal/note-7_attachments/pic.jpg")
            .exists(),
        "the copies in the old notebook folder are removed"
    );

    // A deleted note loses its files on both sides.
    let left = HashMap::from([(9u64, "Work".to_string())]);
    attachments::sync(&mut remote, &data, &left, &HashSet::from([7u64])).unwrap();
    assert!(!attachments::dir(&data).join("7").exists());
    assert!(!server_root
        .join("OwnNotes/Work/note-7_attachments")
        .exists());

    let _ = std::fs::remove_dir_all(&root);
}
