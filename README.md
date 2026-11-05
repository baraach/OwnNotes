<p align="center">
  <img src="assets/icon/icon-256.png" width="112" alt="OwnNotes icon" />
</p>

<h1 align="center">OwnNotes</h1>

<p align="center">
  <b>Notes that are files you own.</b><br/>
  Rich text, right-to-left support, and sync through your own WebDAV folder —
  on macOS, Windows, Linux, iOS and Android. No account, no vendor, no lock-in.
</p>

<p align="center">
  <img alt="macOS" src="https://img.shields.io/badge/macOS-11%2B-black?logo=apple&logoColor=white" />
  <img alt="Windows" src="https://img.shields.io/badge/Windows-10%2B-0078D4?logo=windows&logoColor=white" />
  <img alt="Linux" src="https://img.shields.io/badge/Linux-FCC624?logo=linux&logoColor=black" />
  <img alt="iOS" src="https://img.shields.io/badge/iOS-15%2B-000000?logo=apple&logoColor=white" />
  <img alt="Android" src="https://img.shields.io/badge/Android-7%2B-3DDC84?logo=android&logoColor=white" />
  <img alt="Rust" src="https://img.shields.io/badge/Rust-2021-orange?logo=rust&logoColor=white" />
  <img alt="Tauri" src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white" />
  <img alt="Languages" src="https://img.shields.io/badge/languages-6-4c8bf5" />
  <img alt="License" src="https://img.shields.io/badge/license-GPL--3.0-blue" />
</p>

---

## ✨ What it is

A notes app built the opposite way round from the cloud ones: your notes are
**plain files in a folder you choose**, and if you want them on more than one
device you point the app at a **WebDAV folder you already have** — Nextcloud,
Koofr, a NAS, anything.

```
notes/Work/note-4183.html      ← readable, syncable, yours
```

The same Rust core and file format runs in the desktop app and in the iOS and
Android apps. The desktop keeps its three-pane window; phones get a touch-first,
one-pane layout with platform-specific navigation and chrome.

## 📸 Screenshots

<p align="center">
  <img src="docs/screenshots/macos.png" width="900" alt="OwnNotes on macOS with a note open: floating sidebar, note list and rich text editor" />
</p>

<p align="center">
  <img src="docs/screenshots/windows.png" width="440" alt="OwnNotes on Windows 11 with the Fluent toolbar, notebook tree and a note open" />
  <img src="docs/screenshots/linux.png" width="440" alt="OwnNotes on Linux with the Adwaita header bar, notebook tree and a note open" />
</p>

<p align="center">
  <sub>macOS, Windows 11 and Linux: the same three-pane layout and sample
  notes, each following its own system design. The notebook tree stays on the
  left, with the rich note and format bar on the right.</sub>
</p>

<p align="center">
  <img src="docs/screenshots/ios.png" width="300" alt="OwnNotes on iPhone with the iOS list, floating navigation and Search action" />
  <img src="docs/screenshots/android.png" width="300" alt="OwnNotes on Android with the Material search bar, note list and bottom navigation" />
</p>

<p align="center">
  <sub>iPhone: native large titles, inset grouped lists, floating navigation and
  a dedicated Search action. Android: Material 3 search, grouped notes,
  floating navigation and compose button.</sub>
</p>

## 🌟 Features

### ✍️ Writing
- **Rich text** — bold, italic, underline, strikethrough, `code`, colours and highlights
- **Blocks** — headings *H1–H6*, lists, checklists, quotes, tables, code blocks, dividers
- **Collapsible sections**, **LaTeX** formulas rendered as MathML, **embedded videos**
- **Images and files** — attachments stored beside the note and synced with it
- **Markdown as you type** — `#`, `-`, `1.`, `>`, `- [ ]` convert on space
- **One writing surface everywhere** — the same note editor, format bar and
  autosave on desktop and phones

### 📱 iPhone and Android
- **Platform-specific layouts** — iOS large titles, inset grouped lists, a
  floating glass navigation bar, a separate Search action and edge-swipe back;
  Android follows Material 3 with a search bar, grouped notes, floating
  navigation, compose action and tonal colours
- **Touch-first structure** — Notes, Browse and Settings are bottom
  destinations; iOS puts Search beside the bar, Android keeps the search field
  above the list, and Browse stays navigable instead of covering the bar
- **Keyboard-aware editor** — the format toolbar sits above the keyboard using
  iOS's visual viewport and Android's IME/system-bar insets
- **Safe areas and motion** — notches, home indicators, Android gesture bars,
  reduced-motion support and platform-native sheets
- **The same data and sync** — notebooks, tags, favourites, locks and WebDAV
  sync carry over from the desktop app

### 🗂 Organising
- 🗄 **Spaces** — separate universes of notes
- 📚 **Nested notebooks**, and a note can live in **several notebooks at once**
- 🏷 **Tags** — colours, nesting (`work/2026/q1`), rename, delete, drag to reorder
- 📌 **Quick Access**, pinning, favourites
- 🔀 **Drag and drop** notes onto notebooks, tags or the trash — move or add, your choice
- 🧩 **Merge** several notes into one (with undo), **multi-select**, **manual order**
- 🔎 **Unsynced** and **Uncategorized** lists, per-view sorting, saved searches

### 🔒 Privacy
- 🔐 **App lock** and note, notebook and space locks
- 🛡 Locked notes are **encrypted**, not just hidden — AES-256-GCM with a key
  derived from your passcode (PBKDF2-SHA256, 120 000 rounds). What reaches your
  server for a locked note is ciphertext.

### ☁️ Sync
- 🌐 **WebDAV only** — your folder, your credentials
- 🤝 First-sync decision: **merge**, **keep this computer**, or **keep the server**
- 🕓 **Version history** with restore, **scheduled backups**, conflict copies
- ⏱ Automatic sync on change and on an interval, plus a manual button and status
- 📱 The same sync runs on desktop, iOS and Android; saved credentials use the
  platform keychain/keystore

### 🌍 Languages
- English
- **العربية** — the whole interface mirrors for right-to-left
- Türkçe · Deutsch · Polski · Français · Español

Note text follows its own language per paragraph, so one note can hold Arabic
and English and each flows correctly.

### 📥 Import · 📤 Export
On the desktop app:
- **Import** from Evernote (`.enex`), Markdown, Simplenote (`.json`), Bear
  (`.bear` archives) and Apple Notes
- **Export** to Markdown, HTML, Text, PNG (single or bulk), PDF, or print

File import and export, print and backup tools are desktop-only for now; the
phone apps focus on writing, organising and sync.

## ⌨️ Desktop shortcuts

| | | | |
|---|---|---|---|
| `⌘N` new note | `⌘S` save now | `⌘K` jump to a note | `⌘J` switch space |
| `⌘F` find in note | `⇧⌘F` focus mode | `⌘E` export | `⌘P` print / PDF |
| `⌥⌘P` save as picture | `⇧⌘P` pin | `⇧⌘S` Quick Access | `⌘⌫` trash or restore |

On Windows and Linux, use `Ctrl` instead of `⌘`.

## 🚀 Build

Requires Rust and the [Tauri prerequisites](https://tauri.app/start/prerequisites/).
Desktop builds are available for macOS, Windows and Linux. iOS needs Xcode;
Android needs a JDK, the Android SDK and an NDK. The exact toolchains, data
locations and generated project layout are in [docs/PLATFORMS.md](docs/PLATFORMS.md).

```bash
git clone https://github.com/baraach/OwnNotes.git
cd OwnNotes
cargo run -p ownnotes-tauri           # desktop development build
```

A distributable desktop app:

```bash
cargo install tauri-cli --version '^2'
cd app-tauri/src-tauri && cargo tauri build
# → target/release/bundle/<platform>/...
```

Phone builds use the same interface and core:

```bash
cd app-tauri/src-tauri
cargo tauri ios build --debug --target aarch64-sim --no-sign
cargo tauri android build --debug --target aarch64 --apk
```

These are simulator/emulator builds. Device, TestFlight and store builds need
your own signing identity; see [docs/PLATFORMS.md](docs/PLATFORMS.md).

## 🧪 Tests

```bash
cargo test --workspace
```

57 tests across the core, plus integration tests that start a real WebDAV
server to exercise attachment sync, and tests for every importer.

The mobile UI shell has a [Maestro smoke flow](mobile-tests/smoke.yaml) used on
the iOS simulator:

```bash
maestro test --device <simulator-udid> mobile-tests/smoke.yaml
```

## 🗂 Project layout

```
core/                         notes, storage, sync, crypto — no UI at all
app-tauri/src-tauri/          shared Tauri entry, state and commands
app-tauri/src-tauri/src/platform/
                              macOS, Windows, Linux, iOS, Android layers
app-tauri/ui/                 shared interface (no build step)
app-tauri/ui/platform/        desktop corrections and complete phone designs
app-tauri/src-tauri/gen/apple/    Xcode project (iOS)
app-tauri/src-tauri/gen/android/  Gradle project (Android)
mobile-tests/                 mobile UI smoke flow
```

The iOS and Android layouts deliberately do not share a mobile stylesheet, so
one platform's design cannot restyle the other. See
[docs/PLATFORMS.md](docs/PLATFORMS.md) for what belongs where and how to build
each target.

## 🧭 How a note is stored

```
notes/Work/note-4183.html
---
id: 4183
title: "Meeting notes"
notebook: 1
space: Notes
pinned: false
starred: false
trashed: false
locked: false
created: 2026-11-04T09:00:00+03:00
updated: 2026-11-04T11:30:00+03:00
---
<p>Quarter plan</p><ul><li>Sync the plan</li></ul>
```

The header makes the file self-describing; the body is ordinary HTML, so it
opens in any browser and diffs sensibly in git.

## 🎨 Style

See [STYLE.md](STYLE.md) — comments explain *why*, errors are sentences in the
user's voice, and anything destructive stops before touching more than it must.

## 📄 License

GPL-3.0. See [LICENSE](LICENSE).
