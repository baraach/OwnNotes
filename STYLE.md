# How this code is written

Notes to myself, from reading the code that is already here.

## Comments say why, not what

```rust
// Attachments never change once added and can be large; copying
// them every day would fill the disk. They stay in their own folder
// (and on the server) instead.
if name == "backups" || name == attachments::LOCAL_FOLDER {
    continue;
}
```

Prose, sentence case, full stops. The code says *what*; a comment earns its
place by explaining the reason, the constraint, or what was tried and rejected.

## Commands carry a one-line contract

```rust
/// Evernote export → notes. `text` is the whole `.enex` file.
```

Shape of the input, shape of the output, in a single line where possible.

## Errors are sentences in the user's voice

```
"Fill in the server address and username first."
"A note couldn't be encrypted; nothing changed"
"Enter the current passcode to change it"
```

What to do, and — when something was attempted — what state it was left in.
Never a code, never a noun phrase.

## Best-effort work is allowed to fail, and says so

```rust
let Ok(entries) = std::fs::read_dir(dir) else { return };
let _ = std::fs::copy(&path, target.join(&name));
```

A backup that can't copy one file doesn't abort the app. Anything destructive
is the opposite: it stops before touching anything else.

## Small helpers with one job

`prune_backups`, `copy_dir`, `settle_copies`, `notes_in_container`. If a
function needs "and" to describe it, it is two functions.

## Settings live in `extras.ui`, clamped

```rust
.unwrap_or(7).clamp(1, 60)
```

A hand-edited `app.json` must not be able to break the app.

## Defensive when deleting

```rust
// Only folders named like the ones made here (`20240131-141500`) are removed.
```

Pattern-match before removing anything from disk.

## Tests are named as behaviours

`a_copy_stays_when_the_notes_own_file_is_missing`,
`the_copy_survives_whichever_file_is_read_first`,
`a_note_written_with_windows_line_endings_still_loads`.
A failing test should read like a bug report.

## JavaScript

- `/** … */` above entry points; `//` above a block whose intent isn't obvious
- functions that produce something return it; the caller decides whether to
  toast (`const path = await exportPng(note); if (path) toast(…)`)
- every user-visible string goes through `t()`, keyed by its English text
- async handlers keep their element in a variable — `event.currentTarget` is
  null after an `await`
