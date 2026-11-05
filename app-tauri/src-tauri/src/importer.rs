//! Bringing notes in from other apps: an Evernote `.enex` export, or a folder
//! of Markdown files. Both end up as ordinary notes with HTML bodies.

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};

pub struct Imported {
    pub title: String,
    pub body: String,
    pub created: Option<DateTime<Local>>,
}

/// One file from a picked folder.
pub struct SourceFile {
    pub name: String,
    pub text: String,
}

/// Parse an Evernote export. Each `<note>` becomes one note; the ENML wrapper
/// is stripped and the content used as the body.
pub fn parse_enex(text: &str) -> Vec<Imported> {
    let mut out = Vec::new();
    let mut rest = text;

    while let Some(start) = rest.find("<note>") {
        rest = &rest[start + "<note>".len()..];
        let Some(end) = rest.find("</note>") else { break };
        let block = &rest[..end];
        rest = &rest[end + "</note>".len()..];

        let title = field(block, "title").map(|t| decode(&t)).unwrap_or_default();
        let raw = match block.find("<content>") {
            Some(at) => {
                let after = &block[at + "<content>".len()..];
                after
                    .find("</content>")
                    .map(|end| &after[..end])
                    .unwrap_or(after)
            }
            None => continue,
        };
        let body = enml_body(raw);
        let created = field(block, "created").and_then(|stamp| parse_stamp(&stamp));

        out.push(Imported {
            title: if title.trim().is_empty() {
                "Untitled".to_string()
            } else {
                title.trim().to_string()
            },
            body,
            created,
        });
    }

    out
}

/// Take the CDATA out of `<content>` and reduce the ENML document to its body.
fn enml_body(raw: &str) -> String {
    let mut text = raw.trim();
    if let Some(start) = text.find("<![CDATA[") {
        text = &text[start + "<![CDATA[".len()..];
        if let Some(end) = text.rfind("]]>") {
            text = &text[..end];
        }
    }
    // Drop the XML and DOCTYPE prologues Evernote puts inside the CDATA.
    while let Some(start) = text.find("<?xml") {
        match text[start..].find("?>") {
            Some(end) => text = &text[start + end + 2..],
            None => break,
        }
    }
    while let Some(start) = text.find("<!DOCTYPE") {
        match text[start..].find('>') {
            Some(end) => text = &text[start + end + 1..],
            None => break,
        }
    }
    let text = text.trim();
    let text = text.strip_prefix("<en-note>").unwrap_or(text);
    let text = text.strip_suffix("</en-note>").unwrap_or(text);
    let text = text.trim();

    // `en-media` elements point at files that live in the same export; there is
    // nothing to show without them, so they are left out rather than shown
    // broken.
    strip_media(text)
}

/// Remove `<en-media …>` tags.
fn strip_media(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find("<en-media") {
        out.push_str(&rest[..start]);
        match rest[start..].find('>') {
            Some(end) => rest = &rest[start + end + 1..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn field(block: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    Some(block[start..end].trim().to_string())
}

/// Evernote timestamps look like `20240131T141500Z`.
fn parse_stamp(stamp: &str) -> Option<DateTime<Local>> {
    let naive = NaiveDateTime::parse_from_str(stamp.trim(), "%Y%m%dT%H%M%SZ").ok()?;
    Some(Utc.from_utc_datetime(&naive).with_timezone(&Local))
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn decode(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// The script that asks the Notes app for every note, as JSON. Locked notes
/// only give their title; anything that can't be read is skipped.
pub const APPLE_NOTES_SCRIPT: &str = r#"
const app = Application("Notes");
const out = [];
for (const note of app.notes()) {
  try {
    let folder = "";
    try { folder = note.container().name(); } catch (e) {}
    if (note.passwordProtected()) { out.push({ name: note.name(), folder, locked: true }); continue; }
    out.push({
      name: note.name(),
      body: note.body(),
      folder,
      created: note.creationDate().toISOString(),
      modified: note.modificationDate().toISOString(),
    });
  } catch (e) {}
}
JSON.stringify(out);
"#;

/// What the Notes script printed → notes. Returns the notes and how many
/// locked ones were left out.
pub fn parse_apple_notes(json: &str) -> (Vec<Imported>, usize) {
    #[derive(serde::Deserialize)]
    struct Entry {
        #[serde(default)]
        name: String,
        #[serde(default)]
        body: String,
        #[serde(default)]
        folder: String,
        #[serde(default)]
        created: Option<String>,
        #[serde(default)]
        locked: bool,
    }
    let entries: Vec<Entry> = serde_json::from_str(json.trim()).unwrap_or_default();
    let mut locked = 0;
    let mut out = Vec::new();
    for entry in entries {
        if entry.folder == "Recently Deleted" {
            continue;
        }
        if entry.locked {
            locked += 1;
            continue;
        }
        let title = entry.name.trim();
        let title = if title.is_empty() { "Untitled".to_string() } else { title.to_string() };
        out.push(Imported {
            body: drop_title_line(&entry.body, &title),
            title,
            created: entry
                .created
                .and_then(|stamp| DateTime::parse_from_rfc3339(&stamp).ok())
                .map(|at| at.with_timezone(&Local)),
        });
    }
    (out, locked)
}

/// Notes puts the title in the body as its first line; it is the note's
/// title here, so the copy in the text goes.
fn drop_title_line(body: &str, title: &str) -> String {
    let body = body.trim();
    if let Some(end) = body.find("</div>") {
        let first = &body[..end + "</div>".len()];
        let text = ownnotes_core::html::html_to_text(first);
        if text.trim() == title.trim() {
            return body[end + "</div>".len()..].trim().to_string();
        }
    }
    body.to_string()
}

/// A folder of Markdown files: the file name becomes the title, the text is
/// converted to HTML.
pub fn parse_markdown_folder(files: &[SourceFile]) -> Vec<Imported> {
    let mut out = Vec::new();
    for file in files {
        let name = file
            .name
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&file.name)
            .to_string();
        if !name.to_lowercase().ends_with(".md") && !name.to_lowercase().ends_with(".markdown") {
            continue;
        }
        let mut text = file.text.clone();
        // A leading `# Title` becomes the note's title, not a heading.
        let mut title = name
            .trim_end_matches(".markdown")
            .trim_end_matches(".md")
            .trim()
            .to_string();
        if let Some(first) = text.lines().next() {
            if let Some(rest) = first.strip_prefix("# ") {
                if !rest.trim().is_empty() {
                    title = rest.trim().to_string();
                    text = text.lines().skip(1).collect::<Vec<_>>().join("\n");
                    text = text.trim_start_matches('\n').to_string();
                }
            }
        }
        out.push(Imported {
            title,
            body: ownnotes_core::html::markdown_to_html(&text),
            created: None,
        });
    }
    out
}

/// Simplenote's JSON backup. Its "Export notes" writes `notes.json` as
/// `{"activeNotes": [...], "trashedNotes": [...]}` with `creationDate` and
/// `lastModified`; a bare list of notes is read too. Only active notes come in.
pub fn parse_simplenote(text: &str) -> Vec<Imported> {
    #[derive(serde::Deserialize)]
    struct Entry {
        #[serde(default)]
        content: String,
        #[serde(default)]
        tags: Vec<String>,
        #[serde(default, alias = "lastModified")]
        modified: Option<String>,
        #[serde(default, alias = "creationDate")]
        created: Option<String>,
        /// Notes written with Simplenote's Markdown switch on.
        #[serde(default)]
        markdown: bool,
    }
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Backup {
        Export {
            #[serde(rename = "activeNotes")]
            active: Vec<Entry>,
        },
        List(Vec<Entry>),
    }

    let entries = match serde_json::from_str::<Backup>(text) {
        Ok(Backup::Export { active }) => active,
        Ok(Backup::List(entries)) => entries,
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::new();
    for entry in entries {
        let content = entry.content.trim_end();
        if content.trim().is_empty() {
            continue;
        }
        let mut lines = content.lines();
        let first = lines.next().unwrap_or_default().trim();
        let rest: String = lines.collect::<Vec<_>>().join("\n");

        let title = if first.is_empty() {
            "Untitled".to_string()
        } else {
            first.chars().take(200).collect::<String>()
        };
        let mut body = String::new();
        if !rest.trim().is_empty() {
            if entry.markdown {
                body.push_str(&ownnotes_core::html::markdown_to_html(rest.trim_start()));
            } else {
                // Plain text: every line stays a line.
                for line in rest.trim_start_matches('\n').lines() {
                    if line.trim().is_empty() {
                        body.push_str("<p><br></p>");
                    } else {
                        body.push_str(&format!("<p>{}</p>", escape(line)));
                    }
                }
            }
        }
        if !entry.tags.is_empty() {
            let tags = entry
                .tags
                .iter()
                .map(|tag| format!("#{}", escape(&tag.trim().replace(' ', "-"))))
                .collect::<Vec<_>>()
                .join(" ");
            body.push_str(&format!("<p>{tags}</p>"));
        }
        if body.is_empty() {
            body.push_str("<p><br></p>");
        }

        out.push(Imported {
            title,
            body,
            created: entry
                .modified
                .or(entry.created)
                .and_then(|stamp| DateTime::parse_from_rfc3339(&stamp).ok())
                .map(|at| at.with_timezone(&Local)),
        });
    }
    out
}

/// Bear's archive: a zip of `.textbundle` folders, each with `text.md` and
/// its assets. Every note inside becomes a note here.
pub fn parse_bear(archive: &[u8]) -> Result<Vec<Imported>, String> {
    use std::io::Read;

    let reader = std::io::Cursor::new(archive);
    let mut zip = zip::ZipArchive::new(reader).map_err(|e| format!("That isn't a Bear archive: {e}"))?;
    let mut out = Vec::new();

    for index in 0..zip.len() {
        let mut entry = match zip.by_index(index) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        let name = entry.name().to_string();
        let lower = name.to_lowercase();
        // The note text is `text.md` inside each `.textbundle`.
        let is_text = lower.ends_with("/text.md") || lower.ends_with(".textbundle/text.md");
        let is_markdown = lower.ends_with(".md") && !lower.contains("/assets/");
        if !is_text && !is_markdown {
            continue;
        }

        let mut text = String::new();
        if entry.read_to_string(&mut text).is_err() {
            continue;
        }
        if text.trim().is_empty() {
            continue;
        }

        // The note's name is the bundle's folder.
        let bundle = name
            .split('/')
            .rev()
            .find(|part| part.to_lowercase().ends_with(".textbundle"))
            .map(|part| part.trim_end_matches(".textbundle").to_string());
        let mut title = bundle.filter(|name| !name.trim().is_empty()).unwrap_or_default();

        let mut body_text = text.clone();
        if let Some(first) = body_text.lines().next() {
            if let Some(rest) = first.strip_prefix("# ") {
                let heading = rest.trim().to_string();
                if !heading.is_empty() {
                    title = heading;
                    body_text = body_text.lines().skip(1).collect::<Vec<_>>().join("\n");
                    body_text = body_text.trim_start_matches('\n').to_string();
                }
            }
        }
        if title.trim().is_empty() {
            title = "Untitled".to_string();
        }

        out.push(Imported {
            title: title.trim().to_string(),
            body: ownnotes_core::html::markdown_to_html(&body_text),
            created: None,
        });
    }

    if out.is_empty() {
        return Err("No notes were found inside that archive".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENEX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE en-export SYSTEM "http://xml.evernote.com/pub/evernote-export3.dtd">
<en-export>
<note><title>Groceries &amp; things</title><content>
<![CDATA[<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE en-note SYSTEM "http://xml.evernote.com/pub/enml2.dtd">
<en-note><div>Milk</div><div>Eggs</div><en-media hash="abc" type="image/png"/></en-note>]]>
</content><created>20240131T141500Z</created></note>
<note><title>Second</title><content><![CDATA[<en-note>Just text</en-note>]]></content></note>
</en-export>"#;

    #[test]
    fn evernote_exports_become_notes() {
        let notes = parse_enex(ENEX);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].title, "Groceries & things");
        assert!(notes[0].body.contains("<div>Milk</div>"));
        assert!(!notes[0].body.contains("en-media"), "media tags are dropped");
        assert!(!notes[0].body.contains("<?xml"));
        assert!(notes[0].created.is_some());
        assert_eq!(notes[1].title, "Second");
        assert_eq!(notes[1].body, "Just text");
        assert!(notes[1].created.is_none());
    }

    #[test]
    fn a_bear_archive_becomes_notes() {
        use std::io::Write;

        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("Trip.textbundle/text.md", options).unwrap();
            zip.write_all(b"# Izmir\n\n- ferry\n- tea").unwrap();
            zip.start_file("Trip.textbundle/assets/pic.jpg", options).unwrap();
            zip.write_all(b"not text").unwrap();
            zip.start_file("Plain.textbundle/text.md", options).unwrap();
            zip.write_all(b"Just a line").unwrap();
            zip.finish().unwrap();
        }
        let notes = parse_bear(buffer.get_ref()).unwrap();
        assert_eq!(notes.len(), 2, "one note per bundle, assets ignored");
        assert_eq!(notes[0].title, "Izmir", "the heading names the note");
        assert!(notes[0].body.contains("<li>ferry</li>"));
        assert_eq!(notes[1].title, "Plain", "otherwise the bundle folder does");
    }

    #[test]
    fn a_simplenote_backup_becomes_notes() {
        let json = r#"[
          {"content":"Shopping\nmilk\neggs","tags":["home","errands"],"modified":"2024-02-01T10:00:00Z"},
          {"content":"Just a line","tags":[]},
          {"content":"   ","tags":[]}
        ]"#;
        let notes = parse_simplenote(json);
        assert_eq!(notes.len(), 2, "empty notes are skipped");
        assert_eq!(notes[0].title, "Shopping");
        assert!(notes[0].body.contains("<p>milk</p><p>eggs</p>"), "lines stay lines");
        assert!(notes[0].body.contains("#home #errands"));
        assert!(notes[0].created.is_some());
        assert_eq!(notes[1].title, "Just a line");
    }

    #[test]
    fn a_real_simplenote_export_is_read() {
        let json = r#"{"activeNotes":[
            {"id":"a1","content":"Plan\n<b>not bold</b>","creationDate":"2023-05-01T08:00:00.000Z",
             "lastModified":"2024-01-02T09:30:00.000Z","tags":["work"],"markdown":false},
            {"id":"a2","content":"Recipe\n**flour**","lastModified":"2024-01-03T09:30:00.000Z","tags":[],"markdown":true}
          ],"trashedNotes":[{"id":"t1","content":"Old","tags":[]}]}"#;
        let notes = parse_simplenote(json);
        assert_eq!(notes.len(), 2, "trashed notes stay out");
        assert_eq!(notes[0].title, "Plan");
        assert!(notes[0].body.contains("&lt;b&gt;not bold&lt;/b&gt;"), "plain text is escaped");
        assert!(notes[0].created.is_some());
        assert!(notes[1].body.contains("<strong>flour</strong>"), "markdown notes are converted");
    }

    #[test]
    fn apple_notes_come_in_without_the_repeated_title() {
        let json = r#"[
          {"name":"Trip","body":"<div><h1>Trip</h1></div><div>Pack the <b>tent</b></div>","folder":"Notes","created":"2024-03-01T10:00:00.000Z"},
          {"name":"Secret","folder":"Notes","locked":true},
          {"name":"Gone","body":"<div>Gone</div>","folder":"Recently Deleted"}
        ]"#;
        let (notes, locked) = parse_apple_notes(json);
        assert_eq!(notes.len(), 1);
        assert_eq!(locked, 1);
        assert_eq!(notes[0].title, "Trip");
        assert_eq!(notes[0].body, "<div>Pack the <b>tent</b></div>");
        assert!(notes[0].created.is_some());
    }

    #[test]
    fn a_folder_of_markdown_becomes_notes() {
        let files = vec![
            SourceFile {
                name: "Trip.md".into(),
                text: "# Trip to Izmir\n\n- ferry\n- tea".into(),
            },
            SourceFile {
                name: "notes/Plain.markdown".into(),
                text: "Hello **world**".into(),
            },
            SourceFile {
                name: "skip.txt".into(),
                text: "not markdown".into(),
            },
        ];
        let notes = parse_markdown_folder(&files);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].title, "Trip to Izmir");
        assert!(notes[0].body.contains("<ul>"));
        assert_eq!(notes[1].title, "Plain");
        assert!(notes[1].body.contains("<strong>world</strong>"));
    }
}
