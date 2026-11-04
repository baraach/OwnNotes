// The data model: notebooks and notes.

use chrono::{DateTime, Datelike, Duration, Local};
use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Notebook {
    /// The notebook this one sits inside, if any.
    #[serde(default)]
    pub parent: Option<u64>,
    pub id: u64,
    pub name: String,
    /// Index into the notebook colour palette (for the sidebar tile).
    pub color: usize,
}

#[derive(Clone)]
pub struct Note {
    pub id: u64,
    pub notebook_id: u64,
    /// Notes live in one space; spaces don't see each other's notes.
    pub space: String,
    pub title: String,
    pub body: String,
    /// Pinned notes stay at the top of the list.
    pub pinned: bool,
    /// Starred notes show up under Favorites.
    pub starred: bool,
    /// True when the note is in the trash (soft-deleted).
    pub trashed: bool,
    /// Hidden behind the app passcode until unlocked.
    pub locked: bool,
    /// Other notebooks this note also appears in. The file is written into
    /// each of their folders, so it can be in two places at once.
    pub also_in: Vec<u64>,
    pub created: DateTime<Local>,
    pub updated: DateTime<Local>,
}

/// The space a note belongs to when nothing else says otherwise.
pub const DEFAULT_SPACE: &str = "Notes";

impl Note {
    pub fn new(id: u64, notebook_id: u64, title: &str, body: &str) -> Self {
        let now = Local::now();
        Note {
            id,
            notebook_id,
            space: DEFAULT_SPACE.to_string(),
            title: title.to_string(),
            body: body.to_string(),
            pinned: false,
            starred: false,
            trashed: false,
            locked: false,
            also_in: Vec::new(),
            created: now,
            updated: now,
        }
    }

    /// A one-line preview of the note body for the list, with Markdown
    /// markers removed. The list elides it to fit.
    pub fn preview(&self) -> String {
        let title = self.title.trim().to_lowercase();
        let text = self
            .body
            .lines()
            .map(clean_line)
            .filter(|l| !l.is_empty() && l.to_lowercase() != title)
            .take(3)
            .collect::<Vec<_>>()
            .join("  ");
        if text.is_empty() {
            "No additional text".to_string()
        } else {
            text
        }
    }

    /// How many unchecked `- [ ]` items the note has.
    /// Unticked checkboxes: `<input type="checkbox">` in HTML notes, and
    /// `- [ ]` lines in notes from the Markdown era.
    pub fn open_todos(&self) -> usize {
        let markdown = self
            .body
            .lines()
            .filter(|l| l.trim_start().starts_with("- [ ]"))
            .count();
        let mut html = 0;
        let mut rest = self.body.as_str();
        while let Some(at) = rest.find("<input") {
            let tag_end = rest[at..].find('>').map(|end| at + end).unwrap_or(rest.len());
            let tag = rest[at..tag_end].to_ascii_lowercase();
            if tag.contains("checkbox") && !tag.contains("checked") {
                html += 1;
            }
            rest = &rest[tag_end..];
        }
        markdown + html
    }

    pub fn updated_label(&self) -> String {
        friendly_time(self.updated)
    }

    /// The title to show, with a stand-in for notes that have none yet.
    pub fn display_title(&self) -> &str {
        if self.title.trim().is_empty() {
            "Untitled"
        } else {
            &self.title
        }
    }
}

/// Strip list, quote and heading markers and inline emphasis from one line.
fn clean_line(line: &str) -> String {
    let mut s = line.trim();
    for prefix in ["- [ ] ", "- [x] ", "- [X] ", "- ", "* ", "> "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
            break;
        }
    }
    if let Some((num, rest)) = s.split_once(". ") {
        if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
            s = rest;
        }
    }
    let s = s.trim_start_matches('#').trim();
    if s == "---" {
        return String::new();
    }
    let s = strip_tags(s);
    s.replace("**", "").replace("~~", "").replace('`', "")
}

/// Drop the small HTML tags the editor adds for underline, highlight and
/// colour, so a preview reads as plain text.
fn strip_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        match rest[start..].find('>') {
            Some(end) => rest = &rest[start + end + 1..],
            None => {
                out.push_str(&rest[start..]);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

/// "Today, 8:14 PM", "Yesterday, 9:02 AM", "Oct 30, 1:44 PM" or "Oct 30, 2024".
pub fn friendly_time(t: DateTime<Local>) -> String {
    let now = Local::now();
    let day = t.date_naive();
    if day == now.date_naive() {
        format!("Today, {}", t.format("%-I:%M %p"))
    } else if Some(day) == now.date_naive().pred_opt() {
        format!("Yesterday, {}", t.format("%-I:%M %p"))
    } else if t.year() == now.year() {
        t.format("%b %-d, %-I:%M %p").to_string()
    } else {
        t.format("%b %-d, %Y").to_string()
    }
}

/// The whole document: notebooks and notes, with sample content so the app
/// opens to something useful.
pub struct Document {
    pub notebooks: Vec<Notebook>,
    pub notes: Vec<Note>,
}

/// A new random id. Ids are random (not 1, 2, 3…) so notes made on two
/// devices never get the same file name when they sync. They stay below
/// 2^53 so they survive JSON round trips in any language.
fn random_id() -> u64 {
    let mut b = [0u8; 8];
    if getrandom::getrandom(&mut b).is_err() {
        // Extremely unlikely; fall back to the clock.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        b = nanos.to_le_bytes();
    }
    (u64::from_le_bytes(b) & ((1 << 53) - 1)).max(1_000_000)
}

impl Document {
    pub fn sample() -> Self {
        let mut doc = Document {
            notebooks: Vec::new(),
            notes: Vec::new(),
        };
        let personal = doc.add_notebook("Personal");
        let work = doc.add_notebook("Work");
        let ideas = doc.add_notebook("Ideas");

        // Notes are HTML; the welcome note shows everything the editor can do.
        let welcome = doc.add_note(
            personal,
            "Welcome to OwnNotes",
            "<p>Thanks for trying <strong>OwnNotes</strong>. Your notes are saved on this computer \
             and, if you like, in <em>your own</em> WebDAV folder — a Nextcloud, ownCloud or NAS you \
             control. Never on our servers.</p>\
             <h2>Create a checklist to manage your todos</h2>\
             <ul>\
               <li><input type=\"checkbox\" checked> Install OwnNotes</li>\
               <li><input type=\"checkbox\"> Point OwnNotes at your WebDAV folder in Settings</li>\
               <li><input type=\"checkbox\"> Write your first note</li>\
             </ul>\
             <h2>Write your thoughts with rich formatting</h2>\
             <ul>\
               <li>Write text in <strong>bold</strong>, <em>italic</em>, <u>underline</u>, \
                   <s>strike-through</s> and <code>code</code>\
                 <ul>\
                   <li>Nested ideas go in a list, like this</li>\
                   <li>Highlight in colours: <mark>orange</mark>, \
                       <mark style=\"background:#e8a0a0\">red</mark>, \
                       <mark style=\"background:#a9c7f0\">blue</mark>, \
                       <mark style=\"background:#bfe3bf\">green</mark></li>\
                 </ul>\
               </li>\
               <li>Add a link to another note with <strong>⌘K</strong>, or a web link like \
                   <a href=\"https://example.com\">this one</a></li>\
               <li>Drop a picture in, or use <strong>/</strong> for the insert menu</li>\
             </ul>\
             <h2>Add a quote</h2>\
             <blockquote>Your notes are files you own, in a folder you choose.</blockquote>\
             <h2>Keep it in sync</h2>\
             <p>Settings → Sync &amp; Storage takes any WebDAV address. The first sync asks whether \
             to merge, or to keep one side — nothing is deleted until you answer.</p>",
        );
        let groceries = doc.add_note(
            personal,
            "Groceries",
            "<ul><li><input type=\"checkbox\"> Eggs</li>\
             <li><input type=\"checkbox\"> Milk</li>\
             <li><input type=\"checkbox\" checked> Coffee</li>\
             <li><input type=\"checkbox\"> Bread</li></ul>",
        );
        let meeting = doc.add_note(
            work,
            "Meeting notes",
            "<h2>Quarter plan</h2>\
             <ul><li>Sync plan for the quarter</li>\
             <li>Pick our own storage instead of the vendor cloud</li></ul>\
             <blockquote>We own our data.</blockquote>",
        );
        let release = doc.add_note(
            work,
            "Release checklist",
            "<ol><li>Sync a note to the WebDAV folder</li>\
             <li>Edit it on the server</li>\
             <li>Sync again and check both versions</li>\
             <li>Ship it</li></ol>",
        );
        let app_ideas = doc.add_note(
            ideas,
            "App ideas",
            "<ul><li>A reader that saves articles offline</li>\
             <li>A budget tracker that syncs over WebDAV</li></ul>",
        );
        let reading = doc.add_note(
            ideas,
            "Reading list",
            "<ul><li><input type=\"checkbox\"> <em>The Pragmatic Programmer</em></li>\
             <li><input type=\"checkbox\"> <em>Designing Data-Intensive Applications</em></li></ul>",
        );

        // Notes in other languages, to show that each paragraph picks its own
        // direction: Arabic runs right-to-left, English and Turkish left-to-right.
        let arabic = doc.add_note(
            personal,
            "ملاحظات عربية",
            "<p>هذه ملاحظة باللغة العربية لاختبار اتجاه النص من اليمين إلى اليسار.</p>\
             <h2>قائمة المهام</h2>\
             <ul>\
               <li><input type=\"checkbox\" checked> كتابة الملاحظة الأولى</li>\
               <li><input type=\"checkbox\"> مراجعة الإعدادات</li>\
             </ul>\
             <p>ويمكن كتابة كلمات إنجليزية مثل <strong>OwnNotes</strong> داخل النص العربي دون أن ينكسر الاتجاه.</p>\
             <blockquote>الملاحظات ملفات تملكها أنت، في مجلد تختاره.</blockquote>",
        );
        let turkish = doc.add_note(
            personal,
            "Türkçe notlar",
            "<p>Bu not, Türkçe metnin soldan sağa doğru görünmesini denemek için.</p>\
             <h2>Yapılacaklar</h2>\
             <ul>\
               <li><input type=\"checkbox\" checked> İlk notu yaz</li>\
               <li><input type=\"checkbox\"> Ayarları gözden geçir</li>\
             </ul>\
             <p>Türkçe karakterler: ı, İ, ş, Ş, ğ, Ğ, ç, Ç, ö, Ö, ü, Ü.</p>",
        );
        let mixed = doc.add_note(
            personal,
            "Mixed — مختلط",
            "<p>An English paragraph, written left to right.</p>\
             <p>فقرة عربية في الملاحظة نفسها، تُعرض من اليمين إلى اليسار.</p>\
             <p>Türkçe bir paragraf da aynı notta soldan sağa akar.</p>\
             <p>ثم عودة إلى العربية مرة أخرى.</p>",
        );

        let picture = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='640' \
            height='200'%3E%3Cdefs%3E%3ClinearGradient id='g' x1='0' y1='0' x2='1' y2='1'%3E%3Cstop \
            offset='0' stop-color='%23f7c6b8'/%3E%3Cstop offset='1' stop-color='%2396a7d8'/%3E%3C/linearGradient%3E%3C/defs%3E%3Crect \
            width='640' height='200' rx='18' fill='url(%23g)'/%3E%3Ctext x='320' y='112' text-anchor='middle' \
            font-family='-apple-system,Segoe UI,sans-serif' font-size='30' fill='%2333415c'%3EOwnNotes%3C/text%3E%3C/svg%3E";
        let block = |tex: &str| {
            crate::math::to_mathml(tex, true)
                .map(|mathml| {
                    format!(
                        "<div class=\"math block\" contenteditable=\"false\" data-tex=\"{}\">\
                         {mathml}</div>",
                        tex.replace('"', "&quot;")
                    )
                })
                .unwrap_or_default()
        };
        let inline = crate::math::to_mathml(r"E = mc^2", false)
            .map(|mathml| {
                format!(
                    "<span class=\"math\" contenteditable=\"false\" data-tex=\"E = mc^2\">{mathml}</span>"
                )
            })
            .unwrap_or_default();
        let quadratic = block(r"x = \frac{-b \pm \sqrt{b^2 - 4ac}}{2a}");
        let gauss = block(r"\int_{-\infty}^{\infty} e^{-x^2}\,dx = \sqrt{\pi}");
        if let Some(note) = doc.note_mut(welcome) {
            note.body = format!(
                "{}<h2>Links</h2>\
                 <ul>\
                   <li>A web link: <a href=\"https://example.com\">example.com</a></li>\
                   <li>Another note: <a href=\"ownnotes://note/{groceries}\">Groceries</a> — \
                       click it to jump there</li>\
                 </ul>\
                 <h2>A table</h2>\
                 <table><thead><tr><th>Notebook</th><th>Notes</th></tr></thead>\
                 <tbody><tr><td>Personal</td><td>2</td></tr>\
                 <tr><td>Work</td><td>2</td></tr>\
                 <tr><td>Ideas</td><td>2</td></tr></tbody></table>\
                 <h2>An image</h2>\
                 <p>Pictures are stored next to the note and sync with it:</p>\
                 <img src=\"{picture}\" alt=\"OwnNotes\">\
                 <h2>Maths</h2>\
                 <p>Formulas are written in LaTeX and drawn as they should be. The quadratic \
                 formula:</p>\
                 {quadratic}\
                 <p>A Gaussian integral:</p>\
                 {gauss}\
                 <p>And inline, like {inline} in a sentence, the same works inside a paragraph.</p>",
                note.body
            );
        }

        // Spread the sample notes out in time so the list reads naturally.
        let now = Local::now();
        for (id, hours) in [
            (welcome, 0),
            (groceries, 3),
            (meeting, 26),
            (release, 50),
            (app_ideas, 120),
            (reading, 140),
            (arabic, 2),
            (turkish, 6),
            (mixed, 30),
        ] {
            if let Some(n) = doc.note_mut(id) {
                n.updated = now - Duration::hours(hours);
                n.created = n.updated;
            }
        }
        if let Some(n) = doc.note_mut(welcome) {
            n.pinned = true;
        }
        if let Some(n) = doc.note_mut(groceries) {
            n.starred = true;
        }
        doc
    }

    /// Rebuild a document from saved notebooks and notes.
    pub fn from_parts(notebooks: Vec<Notebook>, notes: Vec<Note>) -> Self {
        Document { notebooks, notes }
    }

    fn fresh_id(&self) -> u64 {
        loop {
            let id = random_id();
            if !self.has_notebook(id) && self.note(id).is_none() {
                return id;
            }
        }
    }

    pub fn add_notebook(&mut self, name: &str) -> u64 {
        let id = self.fresh_id();
        let color = self.notebooks.len(); // cycle through the palette
        self.notebooks.push(Notebook {
            parent: None,
            id,
            name: name.to_string(),
            color,
        });
        id
    }

    /// Add a notebook inside another one.
    pub fn add_sub_notebook(&mut self, name: &str, parent: u64) -> u64 {
        let id = self.add_notebook(name);
        if let Some(notebook) = self.notebooks.iter_mut().find(|n| n.id == id) {
            notebook.parent = Some(parent);
        }
        id
    }

    /// How deep a notebook sits, for drawing the tree.
    pub fn notebook_depth(&self, id: u64) -> usize {
        let mut depth = 0;
        let mut current = id;
        while let Some(notebook) = self.notebooks.iter().find(|n| n.id == current) {
            match notebook.parent {
                Some(parent) => {
                    depth += 1;
                    current = parent;
                    if depth > 8 {
                        break;
                    }
                }
                None => break,
            }
        }
        depth
    }

    pub fn rename_notebook(&mut self, id: u64, name: &str) {
        if let Some(nb) = self.notebooks.iter_mut().find(|n| n.id == id) {
            nb.name = name.to_string();
        }
    }

    pub fn set_notebook_color(&mut self, id: u64, color: usize) {
        if let Some(nb) = self.notebooks.iter_mut().find(|n| n.id == id) {
            nb.color = color;
        }
    }

    /// Remove a notebook and move its notes to the trash.
    pub fn delete_notebook(&mut self, id: u64) {
        let parent = self.notebooks.iter().find(|n| n.id == id).and_then(|n| n.parent);
        // Its notes go to the trash, filed under a notebook that still exists,
        // so they don't bring the deleted one back as "Recovered".
        let home = parent
            .filter(|p| *p != id)
            .or_else(|| self.notebooks.iter().find(|n| n.id != id).map(|n| n.id));
        for note in self.notes.iter_mut() {
            note.also_in.retain(|other| *other != id);
            if note.notebook_id == id {
                note.trashed = true;
                note.pinned = false;
                if let Some(home) = home {
                    note.notebook_id = home;
                }
            }
        }
        // Notebooks inside it move up a level instead of pointing at nothing.
        for child in self.notebooks.iter_mut().filter(|n| n.parent == Some(id)) {
            child.parent = parent;
        }
        self.notebooks.retain(|n| n.id != id);
    }

    pub fn add_note(&mut self, notebook_id: u64, title: &str, body: &str) -> u64 {
        let id = self.fresh_id();
        self.notes.push(Note::new(id, notebook_id, title, body));
        id
    }

    pub fn note(&self, id: u64) -> Option<&Note> {
        self.notes.iter().find(|n| n.id == id)
    }

    pub fn note_mut(&mut self, id: u64) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    /// Every notebook a note appears in: its own first, then the others.
    pub fn notebooks_of(&self, note: &Note) -> Vec<u64> {
        let mut out = vec![note.notebook_id];
        for id in &note.also_in {
            if *id != note.notebook_id && self.has_notebook(*id) && !out.contains(id) {
                out.push(*id);
            }
        }
        out
    }

    pub fn has_notebook(&self, id: u64) -> bool {
        self.notebooks.iter().any(|n| n.id == id)
    }

    /// Notes with no notebook are filed under this name on disk.
    pub const NO_NOTEBOOK: &'static str = "No notebook";

    pub fn notebook_name(&self, id: u64) -> &str {
        self.notebooks
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.name.as_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(if id == 0 { Self::NO_NOTEBOOK } else { "Notes" })
    }

    pub fn notebook_color(&self, id: u64) -> usize {
        self.notebooks
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.color)
            .unwrap_or(0)
    }

    /// Notes not in the trash.
    pub fn live_notes(&self) -> impl Iterator<Item = &Note> {
        self.notes.iter().filter(|n| !n.trashed)
    }

    pub fn count_in(&self, notebook_id: u64) -> usize {
        self.live_notes()
            .filter(|n| n.notebook_id == notebook_id || n.also_in.contains(&notebook_id))
            .count()
    }

    pub fn favorites_count(&self) -> usize {
        self.live_notes().filter(|n| n.starred).count()
    }

    /// Notes that still have unchecked to-do items.
    pub fn todo_count(&self) -> usize {
        self.live_notes().filter(|n| n.open_todos() > 0).count()
    }

    /// How many notes are in the trash.
    pub fn trash_count(&self) -> usize {
        self.notes.iter().filter(|n| n.trashed).count()
    }
}
