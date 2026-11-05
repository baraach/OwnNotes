// The UI: three panes over the Rust core, with a real rich-text editor.

const rawInvoke = window.__TAURI__.core.invoke;

/** Commands that change notes or notebooks: each one asks for a sync soon. */
const CHANGES_NOTES = new Set([
  "save_note", "create_note", "create_notebook", "set_flags", "delete_note", "move_note",
  "rename_notebook", "colour_notebook", "remove_notebook", "restore_version", "save_attachment",
  "add_to_notebook", "remove_from_notebook", "move_note_space", "rename_tag", "empty_trash",
  "lock_note", "remove_note_lock", "set_container_lock", "import_enex", "import_markdown",
  "import_simplenote", "import_apple_notes",
]);

async function invoke(command, args, options) {
  const result = await rawInvoke(command, args, options);
  if (CHANGES_NOTES.has(command)) notesChanged();
  return result;
}

const $ = (id) => document.getElementById(id);

/** Macs use Command; Windows and Linux use Ctrl. */
const PLATFORM = document.documentElement.dataset.platform || "mac";
const IS_MAC = PLATFORM === "mac";

function modKey(event) {
  return IS_MAC ? event.metaKey : event.ctrlKey;
}

const mobileQuery = window.matchMedia("(max-width: 900px)");
let mobileBackUnlisten = null;
let mobileBackBusy = false;

/** Phones and tablets get one pane at a time; the desktop window doesn't. */
function isMobile() {
  return PLATFORM === "ios" || PLATFORM === "android" || mobileQuery.matches;
}

function syncMobileTabs() {
  // Android searches from the bar above the list, so it has no Search tab.
  // A closing page stays unhidden while it fades, but it has already been left.
  const settings = $("settings-modal");
  const active = !settings.hidden && !settings.classList.contains("is-closing")
    ? "settings"
    : document.body.classList.contains("mobile-nav")
    ? "browse"
    : document.body.classList.contains("mobile-search") && PLATFORM !== "android"
      ? "search"
      : "notes";
  for (const tab of document.querySelectorAll("[data-mobile-tab]")) {
    tab.classList.toggle("on", tab.dataset.mobileTab === active);
  }
  moveTabIndicator();
  enterMobileView(active);
}

/** Slide the active-tab pill under whichever tab is on. */
function moveTabIndicator() {
  const pill = $("tab-indicator");
  const tab = document.querySelector(".mobile-tabs button.on");
  // A hidden bar measures as zero; the pill stays put until the bar is back.
  if (!pill || !tab || !tab.offsetWidth) return;
  // iOS shows Search as a circle of its own, which the pill can't reach.
  const apart = getComputedStyle(tab).position === "absolute";
  pill.style.opacity = apart ? "0" : "1";
  if (apart) return;
  pill.style.width = tab.offsetWidth + "px";
  pill.style.transform = `translateX(${tab.offsetLeft}px)`;
}

let lastMobileTab = null;

/** Replay the entrance of the screen a tab change just revealed. */
function enterMobileView(active) {
  const previous = lastMobileTab;
  if (active === previous) return;
  lastMobileTab = active;
  if (PLATFORM !== "ios" && PLATFORM !== "android") return;
  // Search filters the list in place, so nothing new has arrived.
  if (active === "search" || previous === "search") return;
  const pane = active === "browse" ? $("sidebar") : active === "settings" ? $("settings-modal") : $("list-pane");
  pane.classList.remove("view-enter");
  void pane.offsetWidth;
  pane.classList.add("view-enter");
  setTimeout(() => pane.classList.remove("view-enter"), 700);
}

let editorExitTimer = null;

/** The editor is already closed; this only keeps it on screen while it slides out. */
function playEditorExit() {
  document.body.classList.add("editor-leaving");
  clearTimeout(editorExitTimer);
  editorExitTimer = setTimeout(() => document.body.classList.remove("editor-leaving"), 300);
}

function closeMobileEditor() {
  if (isMobile()) {
    const body = document.body;
    const wasOpen = body.classList.contains("mobile-editor");
    body.classList.remove("mobile-editor");
    // A back swipe has already carried the editor off the screen.
    if (wasOpen && !body.classList.contains("mobile-swiping")) playEditorExit();
  }
  syncMobileTabs();
  syncMobileBack();
}

/** Show the note list again after the editor, without losing the selection. */
function openMobileEditor() {
  if (!isMobile()) return;
  closeMobileNav();
  $("search").blur();
  clearTimeout(editorExitTimer);
  document.body.classList.remove("editor-leaving");
  document.body.classList.add("mobile-editor");
  syncMobileTabs();
  syncMobileBack();
}

function closeMobileNav() {
  if (!isMobile()) return;
  document.body.classList.remove("mobile-nav");
  const scrim = $("mobile-scrim");
  if (scrim) scrim.hidden = true;
  const sidebar = $("sidebar");
  if (sidebar) {
    sidebar.setAttribute("inert", "");
    sidebar.setAttribute("aria-hidden", "true");
  }
  syncMobileTabs();
  syncMobileBack();
}

function toggleMobileNav() {
  if (!isMobile()) return;
  const sidebar = $("sidebar");
  const scrim = $("mobile-scrim");
  if (document.body.classList.contains("mobile-nav")) {
    closeMobileNav();
    return;
  }
  document.body.classList.add("mobile-nav");
  if (scrim) scrim.hidden = false;
  if (sidebar) {
    sidebar.classList.remove("is-hidden");
    sidebar.removeAttribute("inert");
    sidebar.setAttribute("aria-hidden", "false");
  }
  syncMobileTabs();
  syncMobileBack();
}

function applyMobileMode() {
  const mobile = isMobile();
  document.documentElement.dataset.mobile = mobile ? "true" : "false";
  if (mobile) {
    closeMobileEditor();
    closeMobileNav();
    syncMobileTabs();
    syncMobileBack();
    return;
  }
  document.body.classList.remove("mobile-editor", "mobile-nav", "mobile-search");
  const scrim = $("mobile-scrim");
  if (scrim) scrim.hidden = true;
  const sidebar = $("sidebar");
  if (sidebar) {
    sidebar.removeAttribute("inert");
    sidebar.setAttribute("aria-hidden", "false");
  }
  syncMobileTabs();
  syncMobileBack();
}

/** The topmost transient layer on a phone, if any. */
function topMobileModal() {
  const modals = [...document.querySelectorAll(".modal:not([hidden])")].filter(
    (modal) => modal.id !== "lock-screen" && getComputedStyle(modal).display !== "none"
  );
  return modals.length ? modals[modals.length - 1] : null;
}

function topMobilePopover() {
  return [...document.querySelectorAll(".context-menu, .palette, .text-style-pop, .slash-menu, .table-tools")]
    .find((element) => !element.hidden && getComputedStyle(element).display !== "none") || null;
}

function closeTopMobileLayer() {
  const popover = topMobilePopover();
  if (popover) {
    // Menus are built per use; the others stay in the page and open again.
    if (popover.matches(".context-menu")) popover.remove();
    else popover.hidden = true;
    document.querySelector(".menu-scrim")?.remove();
    return true;
  }
  const modal = topMobileModal();
  if (modal) {
    if (modal.id === "settings-modal") (settingsAtRoot ? $("s-close") : $("settings-back"))?.click();
    else if (modal.id === "tour") $("tour-later")?.click();
    else if (modal.id === "export-modal") $("export-close")?.click();
    else if (modal.id === "history-modal") $("h-close")?.click();
    else {
      const close = modal.querySelector(".dialog-cancel, .formula-cancel");
      if (close) close.click();
      else modal.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    }
    return true;
  }
  if (document.body.classList.contains("mobile-nav")) {
    closeMobileNav();
    return true;
  }
  if (document.body.classList.contains("mobile-search")) {
    leaveMobileSearch();
    return true;
  }
  if (document.body.classList.contains("mobile-editor")) {
    closeMobileEditor();
    return true;
  }
  return false;
}

function leaveMobileSearch() {
  document.body.classList.remove("mobile-search");
  $("search").blur();
  $("search").value = "";
  state.search = "";
  $("save-search").hidden = true;
  renderCards();
  syncMobileTabs();
  syncMobileBack();
}

/** Android's back gesture closes the editor or drawer before leaving the app. */
async function syncMobileBack() {
  if (PLATFORM !== "android" || !isMobile()) {
    if (mobileBackUnlisten) {
      const off = mobileBackUnlisten;
      mobileBackUnlisten = null;
      try { off(); } catch {}
    }
    return;
  }
  const needed =
    document.body.classList.contains("mobile-editor") ||
    document.body.classList.contains("mobile-nav") ||
    document.body.classList.contains("mobile-search") ||
    !!topMobileModal() ||
    !!topMobilePopover();
  if (!needed) {
    if (mobileBackUnlisten) {
      const off = mobileBackUnlisten;
      mobileBackUnlisten = null;
      try { off(); } catch {}
    }
    return;
  }
  if (mobileBackUnlisten || mobileBackBusy) return;
  const app = window.__TAURI__?.app;
  if (!app?.onBackButtonPress) return;
  mobileBackBusy = true;
  try {
    mobileBackUnlisten = await app.onBackButtonPress(() => {
      closeTopMobileLayer();
      syncMobileBack();
    });
  } catch {
    mobileBackUnlisten = null;
  } finally {
    mobileBackBusy = false;
  }
  if (!document.body.classList.contains("mobile-editor") && !document.body.classList.contains("mobile-nav") && !document.body.classList.contains("mobile-search") && !topMobileModal() && !topMobilePopover() && mobileBackUnlisten) {
    const off = mobileBackUnlisten;
    mobileBackUnlisten = null;
    try { off(); } catch {}
  }
}

const mobileBackObserver = new MutationObserver(() => {
  if (PLATFORM === "android") syncMobileBack();
});
mobileBackObserver.observe(document.body, {
  subtree: true,
  childList: true,
  attributes: true,
  attributeFilter: ["hidden"],
});
mobileQuery.addEventListener("change", applyMobileMode);

/** "⌘⇧F" → "Ctrl+Shift+F" on the platforms that use Ctrl. */
function platformLabel(text) {
  if (IS_MAC || typeof text !== "string" || !/[⌘⇧⌥⌫]/.test(text)) return text;
  return text
    .replace(/⇧/g, "Shift+")
    .replace(/⌥/g, "Alt+")
    .replace(/⌘/g, "Ctrl+")
    .replace(/⌫/g, "Backspace")
    .replace(/Shift\+Ctrl\+/g, "Ctrl+Shift+")
    .replace(/Alt\+Ctrl\+/g, "Ctrl+Alt+")
    .replace(/Shift\+Alt\+/g, "Alt+Shift+");
}
window.platformLabel = platformLabel;

/** Rewrite shortcut labels in the fixed UI and in menus. Never touches
 *  the note text or the user's note titles. */
function applyPlatformLabels(root = document) {
  if (IS_MAC) return;
  const base = root instanceof Document ? root.body : root;
  if (!base) return;

  for (const el of base.querySelectorAll("[title], [placeholder], [aria-label]")) {
    for (const attr of ["title", "placeholder", "aria-label"]) {
      const value = el.getAttribute(attr);
      if (!value) continue;
      const next = platformLabel(value);
      if (next !== value) el.setAttribute(attr, next);
    }
  }

  for (const el of base.querySelectorAll("kbd, .keys, .tour-list, .welcome-points, .menu-hint")) {
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    const nodes = [];
    while (walker.nextNode()) nodes.push(walker.currentNode);
    for (const node of nodes) {
      const next = platformLabel(node.nodeValue);
      if (next !== node.nodeValue) node.nodeValue = next;
    }
  }
}
window.applyPlatformLabels = applyPlatformLabels;

const state = {
  notes: [],
  notebooks: [],
  spaces: [],
  space: null,
  unsynced: [],
  selected: new Set(),
  unlocked: new Set(),
  decrypted: new Map(),

  /** Locked notebooks and spaces, and the ones opened this session. */
  locks: { notebooks: [], spaces: [] },
  unlockedBooks: new Set(),
  unlockedSpaces: new Set(),
  prefs: {},
  view: { kind: "all" },
  current: null,
  search: "",
  saveTimer: null,
  loading: false,
};

// ------------------------------------------------------------ data

async function refresh() {
  const snap = await invoke("load");
  state.unsynced = await unsyncedNotes();
  try {
    state.locks = await rawInvoke("locks");
  } catch {
    // older backend: nothing is locked
  }
  state.notes = snap.notes;
  for (const note of state.notes) {
    const plain = state.decrypted.get(note.id);
    if (plain !== undefined && note.locked) note.body = plain;
  }
  state.notebooks = snap.notebooks;
  // Spaces come from the notes in them, so a space created a moment ago
  // has none yet: keep it (and stay in it) until it gets its first note.
  const fresh = snap.spaces || [];
  const empty = state.spaces.filter((space) => !fresh.includes(space) && emptySpaces.has(space));
  state.spaces = [...fresh, ...empty].sort();
  if (!state.space || !state.spaces.includes(state.space)) {
    state.space = state.spaces[0] || "Notes";
  }
  state.prefs = snap.prefs;
  if ((snap.problems || []).length && !state.warned) {
    state.warned = true;
    const first = snap.problems[0];
    const more = snap.problems.length > 1 ? " " + t("(+{n} more)", { n: snap.problems.length - 1 }) : "";
    toast(t("Some files couldn't be read: {file}", { file: first }) + more);
  }
  applyTheme();
  render();
}

/** Spaces made in this session that have no notes yet. */
const emptySpaces = new Set();

const uiPrefs = {};

async function loadUiPrefs() {
  for (const key of [
    "show_sub_notes",
    "show_sub_list",
    "compact_cards",
    "saved_searches",
    "backup_hours",
    "backup_keep",
    "spell_lang",
    "image_size",
    "covers",
    "collapsed",
    "theme_name",
    "editor_font",
    "markdown_mode",
    "toured",
    "lang",
    "onboarded",
    "book_order",
    "tag_order",
    "tag_colours",
    "collapsed_tags",
    "glass",
    "pane_widths",
  ]) {
    try {
      const value = await rawInvoke("ui_pref", { key, value: null });
      if (value !== null) uiPrefs[key] = value;
    } catch {
      // an older data folder simply has no saved choice
    }
  }
}

async function setUiPref(key, value) {
  uiPrefs[key] = value;
  try {
    await rawInvoke("ui_pref", { key, value });
  } catch (error) {
    toast(String(error));
  }
}

function pref(key, fallback) {
  return uiPrefs[key] === undefined ? fallback : uiPrefs[key];
}

/** The notebook and everything under it. */
function notebookFamily(id) {
  const all = [id];
  for (let i = 0; i < all.length; i += 1) {
    for (const book of state.notebooks) {
      if (book.parent === all[i]) all.push(book.id);
    }
  }
  return new Set(all);
}

/** The key a view's own sort order is remembered under. */
function sortKey() {
  const view = state.view;
  if (view.kind === "notebook") return `sort:notebook:${view.id}`;
  if (view.kind === "tag") return `sort:tag:${view.tag}`;
  return `sort:${view.kind}`;
}

function sortOrder() {
  // Backend prefs arrive capitalised ("Updated"); the rest of the code and
  // the saved per-view choice use lower case.
  return String(pref(sortKey(), state.prefs.sort || "updated")).toLowerCase();
}

/** Does a note belong in the open view, before the search is applied? */
function inView(note) {
  switch (state.view.kind) {
    case "favorites":
      return !note.trashed && note.starred;
    case "todo":
      return !note.trashed && note.todos > 0;
    case "trash":
      return note.trashed;
    case "notebook": {
      if (note.trashed) return false;
      if ((note.notebooks || [note.notebook_id]).includes(state.view.id)) return true;
      if (!pref("show_sub_notes", true)) return false;
      return notebookFamily(state.view.id).has(note.notebook_id);
    }
    case "uncategorized":
      return !note.trashed && note.notebook_id === 0;
    case "unsynced":
      return !note.trashed && state.unsynced.includes(note.id);
    case "uncategorized":
      return !note.trashed && noteTags(note).length === 0;
    case "tag": {
      if (note.trashed) return false;
      const wanted = state.view.tag;
      return noteTags(note).some((tag) => tag === wanted || tag.startsWith(`${wanted}/`));
    }
    default:
      return !note.trashed;
  }
}

/** Notes edited since the last sync, or never synced at all. */
async function unsyncedNotes() {
  try {
    return await rawInvoke("unsynced");
  } catch {
    return [];
  }
}

function bookHidden(id) {
  return state.locks.notebooks.includes(id) && !state.unlockedBooks.has(id);
}

function spaceHidden(name) {
  return state.locks.spaces.includes(name) && !state.unlockedSpaces.has(name);
}

/** Locked (itself, or by its notebook or space) and not opened with the
 *  passcode in this session. */
function isHidden(note) {
  // Opened with the passcode this session: shown, whatever locks it.
  if (state.unlocked.has(note.id)) return false;
  if (note.locked) return true;
  if (spaceHidden(note.space)) return true;
  return (note.notebooks || [note.notebook_id]).some(bookHidden);
}

/** Ask for the app passcode; true when it matches. */
async function askPasscode(title) {
  const code = await Dialog.ask({ title, label: "Your app passcode.", password: true, ok: "Unlock" });
  if (code === null) return null;
  const ok = await rawInvoke("lock_check", { passcode: code }).catch(() => false);
  if (!ok) {
    toast("That passcode doesn't match");
    return null;
  }
  return code;
}

/** Lock or unlock a notebook ("notebook", id) or a space ("space", name). */
async function setContainerLock(kind, key, locked) {
  const code = await Dialog.ask({
    title: locked ? (kind === "notebook" ? "Lock this notebook" : "Lock this space") : "Remove the lock",
    label: locked
      ? "Its notes are hidden behind your app passcode (on this computer)."
      : "Your app passcode. It stays unlocked after this.",
    password: true,
    ok: locked ? "Lock" : "Remove lock",
  });
  if (code === null) return;
  try {
    await invoke("set_container_lock", { kind, key: String(key), locked, passcode: code });
    if (kind === "notebook") locked ? state.unlockedBooks.delete(key) : state.unlockedBooks.add(key);
    else locked ? state.unlockedSpaces.delete(key) : state.unlockedSpaces.add(key);
    await refresh();
    toast(locked ? (kind === "notebook" ? "The notebook is locked" : "The space is locked") : "Lock removed");
  } catch (error) {
    toast(String(error));
  }
}

/** Notebooks whose contents are hidden right now. */
function lockedNotebooks() {
  return new Set(
    state.notebooks
      .filter((book) => state.locks.notebooks.includes(book.id) && !state.unlockedBooks.has(book.id))
      .map((book) => book.id)
  );
}

function visibleNotes() {
  const query = state.search.trim().toLowerCase();
  const hidden = lockedNotebooks();
  // Search narrows the open view rather than jumping to every note.
  let notes = state.notes.filter((note) => {
    if (state.space && note.space !== state.space) return false;
    if (hidden.size && (note.notebooks || [note.notebook_id]).some((id) => hidden.has(id))) return false;
    if (!inView(note)) return false;
    if (!query) return true;
    // A locked note is only found by its title.
    if (state.titleOnly || isHidden(note)) return note.title.toLowerCase().includes(query);
    return note.title.toLowerCase().includes(query) || note.body.toLowerCase().includes(query);
  });
  // One card per note, however many notebooks it is filed in.
  const seen = new Set();
  notes = notes.filter((note) => {
    if (seen.has(note.id)) return false;
    seen.add(note.id);
    return true;
  });

  const order = sortOrder();
  notes.sort((a, b) => {
    if (order !== "manual" && a.pinned !== b.pinned) return a.pinned ? -1 : 1;
    if (order === "title") return (a.title || "").localeCompare(b.title || "");
    if (order === "manual") {
      const rank = pref("manual_order", {});
      return (rank[a.id] ?? Number.MAX_SAFE_INTEGER) - (rank[b.id] ?? Number.MAX_SAFE_INTEGER);
    }
    // Newest first, by the real dates (ids aren't in time order).
    if (order === "created") return (b.created_ms ?? 0) - (a.created_ms ?? 0);
    return (b.updated_ms ?? 0) - (a.updated_ms ?? 0);
  });
  return notes;
}

// ------------------------------------------------------------ render

const ICONS = {
  all: '<svg viewBox="0 0 24 24"><path d="M4 5h10l6 6v8H4z"/><path d="M14 5v6h6"/></svg>',
  star: '<svg viewBox="0 0 24 24"><path d="m12 3 2.7 5.6 6.3.9-4.5 4.4 1 6.1-5.5-2.9-5.5 2.9 1-6.1L3 9.5l6.3-.9z"/></svg>',
  todo: '<svg viewBox="0 0 24 24"><rect x="4" y="4" width="16" height="16" rx="3"/><path d="m8 12 3 3 5-6"/></svg>',
  trash: '<svg viewBox="0 0 24 24"><path d="M4 7h16"/><path d="M9 7V5h6v2"/><path d="M6 7l1 13h10l1-13"/></svg>',
  pin: '<svg viewBox="0 0 24 24"><path d="M12 17v5"/><path d="M9 3h6l-1 6 3 3v2H7v-2l3-3z"/></svg>',
};

function renderSpaces() {
  const spaces = state.spaces.length ? state.spaces : [state.space || "Notes"];
  $("space-name").textContent = state.space;
  $("space").dataset.spaces = spaces.join("\u0000");
}

$("space").addEventListener("click", () => openSpaceMenu());

function openSpaceMenu() {
  const spaces = state.spaces.length ? state.spaces : [state.space || "Notes"];
  const rect = $("space").getBoundingClientRect();
  const items = spaces.map((space) => ({
    label: space,
    icon: '<svg viewBox="0 0 24 24"><path d="M4 7h16"/><path d="M4 12h16"/><path d="M4 17h10"/></svg>',
    check: space === state.space,
    run: () => {
      state.space = space;
      state.view = { kind: "all" };
      state.savedIndex = null;
      render();
    },
  }));
  items.push({ separator: true });
  items.push({
    label: "Edit space",
    icon: '<svg viewBox="0 0 24 24"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/></svg>',
    run: () => renameSpace(),
  });
  items.push({
    label: "Copy link",
    icon: '<svg viewBox="0 0 24 24"><path d="M10 14a4.5 4.5 0 0 0 6.4 0l3-3a4.5 4.5 0 0 0-6.4-6.4l-1 1"/><path d="M14 10a4.5 4.5 0 0 0-6.4 0l-3 3a4.5 4.5 0 0 0 6.4 6.4l1-1"/></svg>',
    run: () => copyText(`ownnotes://space/${encodeURIComponent(state.space)}`),
  });
  const spaceLocked = state.locks.spaces.includes(state.space);
  items.push({
    label: spaceLocked ? "Remove lock from space…" : "Lock this space…",
    icon: '<svg viewBox="0 0 24 24"><rect x="5" y="10.5" width="14" height="10" rx="2"/><path d="M8 10.5V7.5a4 4 0 0 1 8 0v3"/></svg>',
    run: () => setContainerLock("space", state.space, !spaceLocked),
  });
  items.push({ separator: true });
  items.push({
    label: "New Space",
    icon: '<svg viewBox="0 0 24 24"><path d="M12 5v14"/><path d="M5 12h14"/></svg>',
    run: () => newSpace(),
  });
  Dialog.menu(rect.left, rect.bottom + 4, items, { anchor: rect });
}

async function newSpace() {
  const name = await Dialog.ask({
    title: "New Space",
    label: "Notes in one space never appear in another.",
    value: "",
    placeholder: "Work, Home, Research…",
    ok: "Create",
  });
  if (!name || !name.trim()) return;
  state.space = name.trim();
  if (!state.spaces.includes(state.space)) {
    state.spaces.push(state.space);
    emptySpaces.add(state.space);
  }
  state.view = { kind: "all" };
  render();
  toast(t("Space \"{name}\" is ready", { name: state.space }));
}

/** Rename the open space: every note in it moves with it. */
async function renameSpace() {
  const from = state.space;
  const name = await Dialog.ask({ title: "Edit space", value: from, ok: "Rename" });
  const to = (name || "").trim();
  if (!to || to === from) return;
  const affected = state.notes.filter((note) => note.space === from);
  for (const note of affected) {
    await invoke("move_note_space", { id: note.id, space: to });
  }
  state.spaces = state.spaces.map((space) => (space === from ? to : space));
  state.space = to;
  await refresh();
  toast(t("Renamed to \"{name}\"", { name: to }));
}

/** Close the editor if its note disappeared; keep the drawer inert. */
function syncMobileLayout() {
  if (!isMobile()) return;
  if (!currentNote()) document.body.classList.remove("mobile-editor");
  if (!document.body.classList.contains("mobile-nav")) {
    const sidebar = $("sidebar");
    if (sidebar) {
      sidebar.setAttribute("inert", "");
      sidebar.setAttribute("aria-hidden", "true");
    }
  }
}

function render() {
  renderSpaces();
  renderQuick();
  renderNav();
  renderNotebooks();
  renderTrashRow();
  renderTags();
  renderSaved();
  renderTemplatesRow();
  renderCards();
  paintSort();
  renderEditor();
  syncMobileLayout();
}

function row({ glyph, tile, label, count, active, indent, onClick, onDrop }) {
  label = t(label);
  const el = document.createElement("button");
  el.className = "row" + (active ? " active" : "");
  if (indent) el.style.paddingLeft = 9 + indent * 16 + "px";
  if (tile) {
    const box = document.createElement("span");
    box.className = "tile";
    if (tile.picture) {
      box.classList.add("picture");
      box.style.backgroundImage = `url("${tile.picture}")`;
    } else {
      box.style.background = tile.colour;
      box.textContent = tile.letter;
    }
    el.appendChild(box);
  } else {
    const g = document.createElement("span");
    g.className = "glyph";
    g.innerHTML = glyph;
    el.appendChild(g);
  }
  const text = document.createElement("span");
  text.className = "label";
  text.textContent = label;
  el.appendChild(text);
  if (count !== undefined && count !== null) {
    const c = document.createElement("span");
    c.className = "count";
    c.textContent = count;
    el.appendChild(c);
  }
  el.addEventListener("click", onClick);
  if (onDrop) {
    el.addEventListener("dragover", (event) => {
      event.preventDefault();
      el.classList.add("dropping");
    });
    el.addEventListener("dragleave", () => el.classList.remove("dropping"));
    el.addEventListener("drop", (event) => {
      event.preventDefault();
      el.classList.remove("dropping");
      const raw = event.dataTransfer.getData("application/x-ownnotes-notes");
      let ids = [];
      try {
        ids = raw ? JSON.parse(raw) : [];
      } catch {
        ids = [];
      }
      if (!ids.length) {
        const single = Number(event.dataTransfer.getData("text/ownnotes-note"));
        if (single) ids = [single];
      }
      const book = event.dataTransfer.getData("application/x-ownnotes-book");
      const tag = event.dataTransfer.getData("application/x-ownnotes-tag");
      const dragged = book ? `book:${book}` : tag ? `tag:${tag}` : "";
      if (ids.length || dragged) onDrop(ids, dragged);
    });
  }
  return el;
}

/** Whether dropping a note on a notebook adds it there or moves it. */
function dropAction() {
  return pref("drop_action", "move");
}

/** Put one tag before another, and remember the order. */
async function reorderTags(moved, before) {
  const all = [...new Set(state.notes.flatMap((note) => noteTags(note)))];
  const order = all.filter((tag) => tag !== moved);
  const at = order.indexOf(before);
  order.splice(at < 0 ? order.length : at, 0, moved);
  await setUiPref("tag_order", order);
  renderTags();
}

/** Put one notebook before another, and remember the order. */
async function reorderBooks(moved, before) {
  const order = state.notebooks
    .map((book) => book.id)
    .filter((id) => id !== moved);
  const at = order.indexOf(before);
  order.splice(at < 0 ? order.length : at, 0, moved);
  await setUiPref("book_order", order);
  renderNotebooks();
}

async function dropNotes(ids, action) {
  for (const id of ids) await dropNote(id, action, true);
  state.selected.clear();
  await refresh();
}

async function dropNote(noteId, action, quiet = false) {
  const note = state.notes.find((n) => n.id === noteId);
  if (!note) return;
  try {
    if (action.kind === "notebook") {
      if (dropAction() === "add") {
        await invoke("add_to_notebook", { id: noteId, notebook: action.id });
        toast(t("Added to {name}", { name: state.notebooks.find((b) => b.id === action.id)?.name || t("notebook") }));
      } else {
        await invoke("move_note", { id: noteId, notebook: action.id });
        toast(t("Moved to {name}", { name: state.notebooks.find((b) => b.id === action.id)?.name || t("notebook") }));
      }
    } else if (action.kind === "trash") {
      await invoke("set_flags", { id: noteId, pinned: null, starred: null, trashed: true });
      toast("Moved to Trash", {
        label: "Undo",
        run: async () => {
          await invoke("set_flags", { id: noteId, pinned: null, starred: null, trashed: false });
          await refresh();
        },
      });
    } else if (action.kind === "quick") {
      await toggleQuick(noteId);
    } else if (action.kind === "tag") {
      await addTagTo(note, action.tag);
    }
    if (!quiet) await refresh();
  } catch (error) {
    toast(String(error));
  }
}

/** Write one #tag into a note, unless it is already there. */
async function addTagTo(note, tag) {
  const body = note.body || "";
  if (new RegExp(`(^|\\s)#${tag}\\b`, "i").test(body.replace(/<[^>]+>/g, " "))) {
    toast(t("#{tag} is already in this note", { tag }));
    return;
  }
  await invoke("save_note", { id: note.id, title: note.title, body: `${body}<p>#${tag}</p>` });
  toast(t("Tagged #{tag}", { tag }));
}

function renderNav() {
  const nav = $("nav");
  nav.innerHTML = "";
  const live = state.notes.filter((n) => !n.trashed);
  const current = state.view.kind;

  nav.appendChild(
    row({
      glyph: ICONS.all,
      label: "All Notes",
      count: live.length,
      active: current === "all",
      onClick: () => setView({ kind: "all" }),
    })
  );
  nav.appendChild(
    row({
      glyph: ICONS.star,
      label: "Favorites",
      count: live.filter((n) => n.starred).length,
      active: current === "favorites",
      onClick: () => setView({ kind: "favorites" }),
    })
  );
  nav.appendChild(
    row({
      glyph: '<svg viewBox="0 0 24 24"><path d="M20 12a8 8 0 1 1-2.3-5.6"/><path d="M20 4v5h-5"/></svg>',
      label: "Unsynced",
      count: state.unsynced.length,
      active: current === "unsynced",
      onClick: () => setView({ kind: "unsynced" }),
    })
  );
  const uncategorized = row({
    glyph: '<svg viewBox="0 0 24 24"><path d="M4 5h16l-6 7.5V19l-4 1.5v-8z"/></svg>',
    label: "Uncategorized",
    count: state.notes.filter(
      (n) => !n.trashed && (!state.space || n.space === state.space) && n.notebook_id === 0
    ).length,
    active: current === "uncategorized",
    onClick: () => setView({ kind: "uncategorized" }),
  });
  uncategorized.title = t("Notes that are not in any notebook");
  nav.appendChild(uncategorized);
  nav.appendChild(
    row({
      glyph: ICONS.todo,
      label: "To-do",
      count: live.filter((n) => n.todos > 0).length,
      active: current === "todo",
      onClick: () => setView({ kind: "todo" }),
    })
  );
}

const COLOURS = ["#3d7ed6", "#3ea66b", "#e58a2e", "#c25bb0", "#2fa8a3", "#d54c48", "#7d6cdb", "#9c8452"];

function covers() {
  return pref("covers", {});
}

function renderNotebooks() {
  const box = $("notebooks");
  box.innerHTML = "";
  const hidden = new Set(pref("collapsed", []));
  const kids = new Map();
  for (const book of state.notebooks) {
    if (book.parent) {
      if (!kids.has(book.parent)) kids.set(book.parent, []);
      kids.get(book.parent).push(book.id);
    }
  }
  const covered = covers();

  const visible = (book) => {
    let parent = book.parent;
    let guard = 0;
    while (parent && guard < 20) {
      if (hidden.has(parent)) return false;
      const next = state.notebooks.find((b) => b.id === parent);
      parent = next ? next.parent : null;
      guard += 1;
    }
    return true;
  };

  const order = pref("book_order", []);
  const sorted = [...state.notebooks].sort((a, b) => {
    const ai = order.indexOf(a.id);
    const bi = order.indexOf(b.id);
    if (ai >= 0 && bi >= 0) return ai - bi;
    if (ai >= 0) return -1;
    if (bi >= 0) return 1;
    return 0;
  });

  for (const book of sorted) {
    if (!visible(book)) continue;
    const hasKids = kids.has(book.id);
    const line = row({
      tile: {
        colour: COLOURS[book.color % COLOURS.length],
        letter: state.locks.notebooks.includes(book.id) && !state.unlockedBooks.has(book.id)
          ? "🔒"
          : covered[book.id] && !/^(https?:|data:image\/)/i.test(covered[book.id])
          ? covered[book.id]
          : (book.name[0] || " ").toUpperCase(),
        picture: covered[book.id] && /^(https?:|data:image\/)/i.test(covered[book.id])
          ? covered[book.id]
          : null,
      },
      label: book.name,
      count: book.count,
      active: state.view.kind === "notebook" && state.view.id === book.id,
      indent: book.depth || 0,
      onClick: () => (state.locks.notebooks.includes(book.id) && !state.unlockedBooks.has(book.id)
        ? unlockNotebook(book)
        : setView({ kind: "notebook", id: book.id })),
      onDrop: (ids, dragging) => {
        if (dragging && dragging.startsWith("book:")) {
          return reorderBooks(Number(dragging.slice(5)), book.id);
        }
        return dropNotes(ids, { kind: "notebook", id: book.id });
      },
    });
    if (state.locks.notebooks.includes(book.id)) line.classList.add("locked-row");
    if (hasKids) {
      const caret = document.createElement("span");
      caret.className = "caret";
      caret.textContent = hidden.has(book.id) ? "▸" : "▾";
      caret.addEventListener("click", async (event) => {
        event.stopPropagation();
        const next = new Set(pref("collapsed", []));
        if (next.has(book.id)) next.delete(book.id);
        else next.add(book.id);
        await setUiPref("collapsed", [...next]);
        renderNotebooks();
        animateNav("notebooks");
      });
      line.insertBefore(caret, line.firstChild);
    }
    line.draggable = true;
    line.addEventListener("dragstart", (event) => {
      event.dataTransfer.setData("application/x-ownnotes-book", String(book.id));
      event.dataTransfer.effectAllowed = "move";
    });
    line.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      event.stopPropagation();
      Dialog.menu(event.clientX, event.clientY, [
        {
          label: "Change cover…",
          run: async () => {
            const next = { ...covers() };
            const value = await Dialog.ask({
              title: t("Cover for \"{name}\"", { name: book.name }),
              label: "A character, an emoji, or a picture address (https://…). Leave empty for the first letter.",
              value: next[book.id] || "",
              ok: "Set",
            });
            if (value === null) return;
            const text = value.trim();
            if (/^(https?:|data:image\/)/i.test(text)) next[book.id] = text;
            else if (text) next[book.id] = [...text].slice(0, 2).join("");
            else delete next[book.id];
            await setUiPref("covers", next);
            renderNotebooks();
          },
        },
        {
          label: book.locked ? "Unlock this notebook…" : "Lock this notebook…",
          run: () => setContainerLock("notebook", book.id, !state.locks.notebooks.includes(book.id)),
        },
        { label: "Rename…", run: () => renameNotebook(book) },
        {
          label: "Delete…",
          danger: true,
          run: async () => {
            const yes = await Dialog.confirm({
              title: `Delete "${book.name}"?`,
              label: "Its notes go to the Trash, where you can still restore them. Notebooks inside it move up a level.",
            });
            if (!yes) return;
            try {
              await invoke("remove_notebook", { id: book.id });
              if (state.view.kind === "notebook" && state.view.id === book.id) state.view = { kind: "all" };
              await refresh();
            } catch (error) {
              toast(String(error));
            }
          },
        },
        {
          label: "New sub-notebook…",
          run: async () => {
            const name = await Dialog.ask({ title: "New sub-notebook", value: "", ok: "Create" });
            if (!name) return;
            const id = await invoke("create_notebook", { name, parent: book.id });
            await refresh();
            setView({ kind: "notebook", id });
          },
        },
      ]);
    });
    box.appendChild(line);
  }
}

async function unlockNotebook(book) {
  const code = await Dialog.ask({
    title: `Unlock "${book.name}"`,
    label: "Your app passcode.",
    password: true,
    ok: "Unlock",
  });
  if (code === null) return;
  try {
    // For this session only; the lock stays.
    await rawInvoke("open_container", { kind: "notebook", key: String(book.id), passcode: code });
    state.unlockedBooks.add(book.id);
    await refresh();
  } catch (error) {
    toast(String(error));
  }
}

async function renameNotebook(book) {
  const name = await Dialog.ask({ title: "Rename notebook", value: book.name, ok: "Rename" });
  if (!name || name.trim() === book.name) return;
  await invoke("rename_notebook", { id: book.id, name: name.trim() });
  await refresh();
}

function renderTrashRow() {
  const box = $("trash-row");
  box.innerHTML = "";
  box.appendChild(
    row({
      glyph: ICONS.trash,
      label: "Trash",
      count: state.notes.filter((n) => n.trashed).length,
      active: state.view.kind === "trash",
      onClick: () => setView({ kind: "trash" }),
      onDrop: (ids) => dropNotes(ids, { kind: "trash" }),
    })
  );
}

function renderCards() {
  if (state.view.kind === "templates") {
    renderTemplatesPane();
    return;
  }
  const notes = visibleNotes();
  $("list-count").textContent = notes.length;
  $("list-title").textContent =
    state.view.kind === "notebook"
      ? state.notebooks.find((b) => b.id === state.view.id)?.name || t("Notebook")
      : state.view.kind === "tag"
      ? "#" + state.view.tag
      : t(
          state.view.kind === "unsynced"
            ? "Unsynced"
            : state.view.kind === "uncategorized"
            ? "Uncategorized"
            : { all: "All Notes", favorites: "Favorites", todo: "To-do", trash: "Trash" }[state.view.kind]
        );

  const box = $("cards");
  box.innerHTML = "";
  if (!notes.length && state.view.kind !== "templates") {
    const empty = document.createElement("p");
    empty.className = "list-empty";
    empty.textContent = t(state.search.trim()
      ? "Nothing here matches that search"
      : state.view.kind === "trash"
      ? "The trash is empty"
      : state.view.kind === "unsynced"
      ? "Everything here is in sync"
      : state.view.kind === "favorites"
      ? "Star a note to find it here"
      : state.view.kind === "todo"
      ? "No notes have open to-dos"
      : "No notes yet — press New Note");
    box.appendChild(empty);
  }
  if (state.view.kind === "notebook" && !state.search.trim()) {
    for (const book of state.notebooks.filter((b) => b.parent === state.view.id)) {
      const entry = document.createElement("button");
      entry.className = "card sub-book";
      entry.innerHTML =
        '<span class="top"><span class="name"></span></span><span class="meta"></span>';
      entry.querySelector(".name").textContent = book.name;
      entry.querySelector(".meta").textContent = plural(book.count, "{n} note", "{n} notes");
      entry.addEventListener("click", () => setView({ kind: "notebook", id: book.id }));
      box.appendChild(entry);
    }
  }
  if (state.view.kind === "trash" && notes.length) {
    const empty = document.createElement("button");
    empty.className = "empty-trash";
    empty.textContent = t("Empty Trash ({n})", { n: notes.length });
    empty.addEventListener("click", async () => {
      const yes = await Dialog.confirm({
        title: plural(notes.length, "Delete {n} note forever?", "Delete {n} notes forever?"),
        label: "Their files and versions go too. This can't be undone.",
        ok: "Empty Trash",
      });
      if (!yes) return;
      const removed = await invoke("empty_trash", { ids: visibleNotes().map((n) => n.id) });
      state.current = null;
      await refresh();
      toast(plural(removed, "Deleted {n} note", "Deleted {n} notes"));
    });
    box.appendChild(empty);
  }
  const lockedBook = state.view.kind === "notebook" && bookHidden(state.view.id);
  const lockedSpace = spaceHidden(state.space);
  if (lockedBook || lockedSpace) {
    const panel = document.createElement("div");
    panel.className = "list-lock";
    panel.innerHTML = `
      <svg viewBox="0 0 24 24"><rect x="5" y="10.5" width="14" height="10" rx="2"/><path d="M8 10.5V7.5a4 4 0 0 1 8 0v3"/></svg>
      <p></p>
      <button class="primary">${t("Unlock")}</button>`;
    panel.querySelector("p").textContent = lockedSpace
      ? t("The space “{name}” is locked.", { name: state.space })
      : t("This notebook is locked.");
    panel.querySelector("button").addEventListener("click", async () => {
      const code = await Dialog.ask({
        title: lockedSpace ? "Unlock this space" : "Unlock this notebook",
        label: "Your app passcode.",
        password: true,
        ok: "Unlock",
      });
      if (code === null) return;
      try {
        await rawInvoke("open_container", {
          kind: lockedSpace ? "space" : "notebook",
          key: lockedSpace ? state.space : String(state.view.id),
          passcode: code,
        });
        if (lockedSpace) state.unlockedSpaces.add(state.space);
        else state.unlockedBooks.add(state.view.id);
        await refresh();
      } catch (error) {
        toast(String(error));
      }
    });
    box.appendChild(panel);
    $("list-count").textContent = "";
    return;
  }
  for (const note of notes) {
    const card = document.createElement("button");
    card.className = "card" + (note.id === state.current ? " active" : "");
    const top = document.createElement("span");
    top.className = "top";
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = note.title.trim() || t("Untitled");
    top.appendChild(name);
    if (note.pinned) {
      const pin = document.createElement("span");
      pin.className = "badge";
      pin.innerHTML = ICONS.pin;
      top.appendChild(pin);
    }
    const preview = document.createElement("span");
    preview.className = "preview";
    preview.textContent = isHidden(note) ? t("Locked") : note.preview;
    const meta = document.createElement("span");
    meta.className = "meta";
    const places = (note.notebooks || [note.notebook_id]).length;
    const stamp = when(note.updated_ms) || note.updated;
    meta.textContent = places > 1 ? `${stamp} · ${plural(places, "in {n} notebook", "in {n} notebooks")}` : stamp;
    const text = document.createElement("span");
    text.className = "card-text";
    text.append(top, preview, meta);
    card.appendChild(text);
    const image = isHidden(note) ? null : firstImage(note.body);
    if (image) {
      const thumb = document.createElement("img");
      thumb.className = "thumb";
      thumb.alt = "";
      thumb.loading = "lazy";
      thumb.src = image;
      thumb.addEventListener("error", () => thumb.remove());
      card.appendChild(thumb);
      card.classList.add("with-thumb");
    }
    card.draggable = true;
    card.addEventListener("dragstart", (event) => {
      const ids = state.selected.has(note.id) && state.selected.size > 1
        ? [...state.selected]
        : [note.id];
      event.dataTransfer.setData("text/ownnotes-note", String(note.id));
      event.dataTransfer.setData("application/x-ownnotes-notes", JSON.stringify(ids));
      event.dataTransfer.effectAllowed = "move";
      if (ids.length > 1) event.dataTransfer.setDragImage(card, 20, 20);
    });
    if (state.selected.has(note.id)) card.classList.add("picked");
    card.addEventListener("click", (event) => {
      if (modKey(event) || event.shiftKey) {
        event.preventDefault();
        if (state.selected.has(note.id)) state.selected.delete(note.id);
        else state.selected.add(note.id);
        renderCards();
        return;
      }
      if (state.selected.size) {
        state.selected.clear();
        renderCards();
      }
      selectNote(note.id);
    });
    card.addEventListener("dragover", (event) => {
      if (sortOrder() !== "manual") return;
      event.preventDefault();
      card.classList.add("drop-before");
    });
    card.addEventListener("dragleave", () => card.classList.remove("drop-before"));
    card.addEventListener("drop", async (event) => {
      card.classList.remove("drop-before");
      if (sortOrder() !== "manual") return;
      event.preventDefault();
      const moved = Number(event.dataTransfer.getData("text/ownnotes-note"));
      if (!moved || moved === note.id) return;
      const order = { ...pref("manual_order", {}) };
      const list = visibleNotes().map((n) => n.id).filter((id) => id !== moved);
      const at = list.indexOf(note.id);
      list.splice(at < 0 ? list.length : at, 0, moved);
      list.forEach((id, index) => {
        order[id] = index;
      });
      await setUiPref("manual_order", order);
      renderCards();
    });

    card.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      const icon = {
        pin: '<svg viewBox="0 0 24 24"><path d="M12 17v5"/><path d="M9 3h6l-1 6 3 3v2H7v-2l3-3z"/></svg>',
        star: '<svg viewBox="0 0 24 24"><path d="m12 3 2.7 5.6 6.3.9-4.5 4.4 1 6.1-5.5-2.9-5.5 2.9 1-6.1L3 9.5l6.3-.9z"/></svg>',
        copy: '<svg viewBox="0 0 24 24"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5h10"/></svg>',
        link: '<svg viewBox="0 0 24 24"><path d="M10 14a4.5 4.5 0 0 0 6.4 0l3-3a4.5 4.5 0 0 0-6.4-6.4l-1 1"/><path d="M14 10a4.5 4.5 0 0 0-6.4 0l-3 3a4.5 4.5 0 0 0 6.4 6.4l1-1"/></svg>',
        book: '<svg viewBox="0 0 24 24"><rect x="5" y="3.5" width="13" height="17" rx="2"/><path d="M9 3.5v17"/></svg>',
        tag: '<svg viewBox="0 0 24 24"><path d="M4 4h9l7 7-9 9-7-7z"/><circle cx="8.5" cy="8.5" r="1.3"/></svg>',
        space: '<svg viewBox="0 0 24 24"><rect x="4" y="4" width="7" height="7" rx="1.5"/><rect x="13" y="4" width="7" height="7" rx="1.5"/><rect x="4" y="13" width="7" height="7" rx="1.5"/><rect x="13" y="13" width="7" height="7" rx="1.5"/></svg>',
        export: '<svg viewBox="0 0 24 24"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M4 21h16"/></svg>',
        template: '<svg viewBox="0 0 24 24"><rect x="4" y="3" width="16" height="18" rx="2"/><path d="M8 8h8"/><path d="M8 12h8"/></svg>',
        trash: '<svg viewBox="0 0 24 24"><path d="M4 7h16"/><path d="M9 7V5h6v2"/><path d="M6 7l1 13h10l1-13"/></svg>',
      };
      const picked = [...state.selected];
      if (picked.length > 1 && picked.includes(note.id)) {
        const join = {
          label: `Merge ${picked.length} notes…`,
          icon: '<svg viewBox="0 0 24 24"><path d="M7 4v7a3 3 0 0 0 3 3h7"/><path d="m14 11 3 3-3 3"/><path d="M7 20v-3"/></svg>',
          run: () => mergeNotes(picked),
        };
        Dialog.menu(event.clientX, event.clientY, [
          join,
          {
            label: `Export ${picked.length} notes…`,
            icon: '<svg viewBox="0 0 24 24"><path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M4 21h16"/></svg>',
            run: () => exportMany(picked),
          },
          {
            label: plural(picked.length, "Move {n} note to Trash", "Move {n} notes to Trash"),
            icon: '<svg viewBox="0 0 24 24"><path d="M4 7h16"/><path d="M9 7V5h6v2"/><path d="M6 7l1 13h10l1-13"/></svg>',
            danger: true,
            run: () => trashMany(picked),
          },
        ]);
        return;
      }
      const items = [
        {
          label: "Open in new window",
          icon: '<svg viewBox="0 0 24 24"><rect x="4" y="4" width="11" height="11" rx="2"/><path d="M14 9h6v11H9v-4"/></svg>',
          run: () => invoke("open_note_window", { id: note.id }).catch((error) => toast(String(error))),
        },
        { separator: true },
        {
          label: note.sealed ? "Remove lock…" : "Lock this note…",
          icon: '<svg viewBox="0 0 24 24"><rect x="5" y="10.5" width="14" height="10" rx="2"/><path d="M8 10.5V7.5a4 4 0 0 1 8 0v3"/></svg>',
          run: () => toggleNoteLock(note),
        },
        { separator: true },
        {
          label: note.pinned ? "Unpin from top" : "Pin to top",
          icon: icon.pin,
          hint: "⇧⌘P",
          run: () => toggleFlag("pinned"),
        },
        {
          label: quickList().includes(note.id)
            ? "Remove from Quick Access"
            : "Add to Quick Access",
          icon: icon.star,
          hint: "⇧⌘S",
          run: () => toggleQuick(note.id),
        },
        { separator: true },
        {
          label: "Duplicate",
          icon: icon.copy,
          run: () =>
            isHidden(note) ? toast("Unlock the note first") : invoke("create_note", { notebookId: note.notebook_id, space: note.space }).then(async (id) => {
              await invoke("save_note", { id, title: (note.title || "") + " (copy)", body: await ownCopies(id, note.body) });
              await refresh();
              selectNote(id);
            }),
        },
        {
          label: "Copy link to note",
          icon: icon.link,
          hint: "⌥⌘L",
          run: () => copyText(`ownnotes://note/${note.id}`),
        },
        { separator: true },
        { label: "Move to another notebook…", icon: icon.book, run: () => moveNote(note) },
        { label: "Add to notebooks…", icon: icon.book, run: () => chooseNotebooks(note) },
        {
          label: "Remove from notebook",
          icon: icon.book,
          run: async () => {
            await invoke("move_note", { id: note.id, notebook: 0 });
            await refresh();
            toast("Moved to Uncategorized");
          },
        },
        { label: "Add tags…", icon: icon.tag, run: () => addTags(note) },
        { label: "Move to other space…", icon: icon.space, run: () => moveToSpace(note) },
        { separator: true },
        { label: "Export…", icon: icon.export, hint: "⌘E", run: () => openExport() },
        {
          label: "Copy to Templates",
          icon: icon.template,
          run: async () => {
            if (isHidden(note)) return toast("Unlock the note first");
            await invoke("save_template", { name: note.title || "Template", body: note.body });
            await renderTemplatesRow();
            toast("Saved to Templates");
          },
        },
        { separator: true },
        {
          label: note.trashed ? "Delete forever…" : "Move to trash",
          icon: icon.trash,
          hint: "⌘⌫",
          danger: !note.trashed,
          run: async () => {
            if (!note.trashed) {
              await invoke("set_flags", { id: note.id, pinned: null, starred: null, trashed: true });
              await refresh();
              toast("Moved to Trash", {
                label: "Undo",
                run: async () => {
                  await invoke("set_flags", { id: note.id, pinned: null, starred: null, trashed: false });
                  await refresh();
                },
              });
              return;
            }
            const yes = await Dialog.confirm({
              title: "Delete this note forever?",
              label: "The note, its files and its versions go. This can't be undone.",
              ok: "Delete",
            });
            if (!yes) return;
            await invoke("empty_trash", { ids: [note.id] });
            state.current = null;
            await refresh();
            toast("Note deleted");
          },
        },
      ];
      const mobileHidden = ["Open in new window", "Export…"];
      const shown = isMobile() ? items.filter((item) => !mobileHidden.includes(item.label)) : items;
      Dialog.menu(event.clientX, event.clientY, shown);
    });
    box.appendChild(card);
  }
}

/** The first picture in a note, for the thumbnail on its card. */
function firstImage(body) {
  const match = /<img[^>]+src="([^"]+)"/i.exec(body || "");
  if (!match) return null;
  const src = match[1].replace(/&amp;/g, "&");
  const allowed = /^(attachment:\/\/localhost\/|https:\/\/|data:image\/)/i.test(src);
  if (!allowed) return null;
  return typeof Editor === "undefined" ? src : Editor.shown(src);
}

function renderEditor() {
  const note = state.notes.find((n) => n.id === state.current);
  const editor = $("editor-pane");
  const empty = editor.querySelector(".empty");

  editor.classList.toggle("no-note", !note);
  if (!note) {
    if (!empty) {
      const div = document.createElement("div");
      div.className = "empty";
      div.textContent = t("Pick a note, or press New Note");
      editor.appendChild(div);
    }
    return;
  }
  if (empty) empty.remove();

  // A locked note shows a screen instead of its contents.
  const locked = isHidden(note);
  editor.classList.toggle("locked", locked);
  document.body.classList.toggle("note-locked", locked);
  let cover = editor.querySelector(".lock-cover");
  if (locked) {
    if (!cover) {
      cover = document.createElement("div");
      cover.className = "lock-cover";
      cover.innerHTML = `
        <div class="lock-box">
          <svg viewBox="0 0 24 24" class="lock-glyph"><rect x="5" y="10.5" width="14" height="10" rx="2"/><path d="M8 10.5V7.5a4 4 0 0 1 8 0v3"/></svg>
          <p class="lock-title"></p>
          <p class="hint">${t("This note is locked. Its contents are hidden.")}</p>
          <button class="primary lock-open">${t("Unlock")}</button>
        </div>`;
      cover.querySelector(".lock-open").addEventListener("click", () => openLockedNote(state.current));
      editor.appendChild(cover);
    }
    cover.querySelector(".lock-title").textContent = note.title.trim() || t("Untitled");
    state.loading = false;
    return;
  }
  if (cover) cover.remove();

  state.loading = true;
  if ($("title").value !== note.title) $("title").value = note.title;
  // The body is HTML; notes from the Markdown era arrive as plain text.
  // Anything that could run code is removed before it is shown (editor.js).
  // The note already in the editor is left alone unless its text really
  // changed (a sync brought a new version), so a background refresh never
  // moves the cursor.
  if (state.shownId !== note.id || state.shownBody !== note.body) {
    const shown = typeof Editor === "undefined" ? note.body : Editor.load(note.body);
    if ($("body").innerHTML !== shown) $("body").innerHTML = shown;
    state.shownId = note.id;
    state.shownBody = note.body;
    // What the editor holds before any typing: compared on save, since
    // cleaning can change the HTML (e.g. "&" → "&amp;") without an edit.
    state.shownHTML = $("body").innerHTML;
  }
  const places = (note.notebooks || [note.notebook_id]).length;
  const home = note.notebook_id === 0
    ? t("Uncategorized")
    : state.notebooks.find((b) => b.id === note.notebook_id)?.name || t("Notebook");
  $("chip").textContent = places > 1 ? `${home} +${places - 1}` : home;
  updateWords();
  $("trash-note").title = note.trashed ? "Restore from Trash" : "Move to Trash";
  $("trash-note").classList.toggle("on", note.trashed);
  $("pin").classList.toggle("on", note.pinned);
  $("star").classList.toggle("on", note.starred);
  state.loading = false;
}

function updateWords() {
  const text = $("body").innerText.trim();
  const words = text ? text.split(/\s+/).length : 0;
  $("words").textContent = plural(words, "{n} word", "{n} words");
}

// ------------------------------------------------------------ actions

function setView(view) {
  flush();
  closeMobileNav();
  closeMobileEditor();
  if (isMobile()) {
    // On a phone the whole pane scrolls, and a new view starts at its top.
    $("list-pane").scrollTop = 0;
    document.body.classList.remove("mobile-search");
    $("search").blur();
    syncMobileTabs();
  }
  state.view = view;
  state.search = "";
  $("search").value = "";
  const first = visibleNotes()[0];
  state.current = first ? first.id : null;
  render();
  animateList();
  animateNote();
  animateSelection();
}

/** A fresh list slides in when the view changes; ordinary refreshes don't. */
function animateList() {
  const box = $("cards");
  box.classList.remove("list-enter");
  void box.offsetWidth;
  box.classList.add("list-enter");
  clearTimeout(listEnterTimer);
  listEnterTimer = setTimeout(() => box.classList.remove("list-enter"), 420);
}

/** The note page crossfades when a different note is opened. */
function animateNote() {
  const editor = $("editor-pane");
  editor.classList.remove("note-enter");
  void editor.offsetWidth;
  editor.classList.add("note-enter");
  clearTimeout(noteEnterTimer);
  noteEnterTimer = setTimeout(() => editor.classList.remove("note-enter"), 300);
}

/** Expanding a notebook or tag folds its rows in. */
function animateNav(id) {
  const box = $(id);
  box.classList.remove("nav-enter");
  void box.offsetWidth;
  box.classList.add("nav-enter");
  clearTimeout(navTimer);
  navTimer = setTimeout(() => box.classList.remove("nav-enter"), 320);
}

/** The sidebar and list selections fade in rather than appearing. */
function animateSelection() {
  const sidebar = $("sidebar");
  const cards = $("cards");
  for (const el of [sidebar, cards]) {
    el.classList.remove("selection-enter");
    void el.offsetWidth;
    el.classList.add("selection-enter");
  }
  clearTimeout(selectionTimer);
  selectionTimer = setTimeout(() => {
    sidebar.classList.remove("selection-enter");
    cards.classList.remove("selection-enter");
  }, 260);
}

function selectNote(id) {
  if (state.current === id) {
    if (isMobile()) openMobileEditor();
    return;
  }
  flush();
  state.current = id;
  renderCards();
  renderEditor();
  animateNote();
  const cards = $("cards");
  cards.classList.remove("selection-enter");
  void cards.offsetWidth;
  cards.classList.add("selection-enter");
  clearTimeout(selectionTimer);
  selectionTimer = setTimeout(() => cards.classList.remove("selection-enter"), 240);
  if (isMobile()) openMobileEditor();
}

function currentNote() {
  return state.notes.find((n) => n.id === state.current);
}

function scheduleSave() {
  if (state.loading) return;
  clearTimeout(state.saveTimer);
  state.saveTimer = setTimeout(saveNow, 350);
}

async function saveNow() {
  const note = currentNote();
  if (!note || state.shownId !== note.id) return;
  if (isHidden(note)) return;
  const title = $("title").value;
  const body = storedBody();
  // Nothing changed: don't touch the note (or its "edited" time).
  if (title === note.title && (body === note.body || $("body").innerHTML === state.shownHTML)) return;
  note.title = title;
  note.body = body;
  state.shownBody = body;
  state.shownHTML = $("body").innerHTML;
  await invoke("save_note", { id: note.id, title, body });
  updateWords();
}

/** The editor's HTML as it is saved (attachment links in their stored form). */
function storedBody() {
  const html = $("body").innerHTML;
  return typeof Editor === "undefined" ? html : Editor.store(html);
}

function flush() {
  clearTimeout(state.saveTimer);
  return saveNow();
}

async function toggleFlag(flag) {
  const note = currentNote();
  if (!note) return;
  const value = !note[flag];
  note[flag] = value;
  await invoke("set_flags", { id: note.id, pinned: flag === "pinned" ? value : null, starred: flag === "starred" ? value : null, trashed: null });
  render();
}

$("editor-menu").addEventListener("click", (event) => {
  const note = currentNote();
  if (!note) return;
  const rect = event.currentTarget.getBoundingClientRect();
  const icon = (path) => `<svg viewBox="0 0 24 24">${path}</svg>`;
  const items = [
    { label: "Export…", icon: icon('<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M4 21h16"/>'), hint: "⌘E", run: () => openExport() },
    { label: "Search this note", icon: icon('<circle cx="11" cy="11" r="6.5"/><path d="m20 20-4.2-4.2"/>'), hint: "⌘F", run: () => openFind() },
    { label: "Copy link to note", icon: icon('<path d="M10 14a4.5 4.5 0 0 0 6.4 0l3-3a4.5 4.5 0 0 0-6.4-6.4l-1 1"/><path d="M14 10a4.5 4.5 0 0 0-6.4 0l-3 3a4.5 4.5 0 0 0 6.4 6.4l1-1"/>'), run: () => copyText(`ownnotes://note/${note.id}`) },
    { separator: true },
    { label: "Duplicate", icon: icon('<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5h10"/>'), run: () => isHidden(note) ? toast("Unlock the note first") : invoke("create_note", { notebookId: note.notebook_id, space: note.space }).then(async (id) => { await invoke("save_note", { id, title: (note.title || "") + " (copy)", body: await ownCopies(id, note.body) }); await refresh(); selectNote(id); }) },
    { label: "Copy to Templates", icon: icon('<rect x="4" y="3" width="16" height="18" rx="2"/><path d="M8 8h8"/><path d="M8 12h8"/>'), run: async () => { if (isHidden(note)) return toast("Unlock the note first"); await invoke("save_template", { name: note.title || "Template", body: note.body }); await renderTemplatesRow(); toast("Saved to Templates"); } },
    { label: "Move to other space…", icon: icon('<rect x="4" y="4" width="7" height="7" rx="1.5"/><rect x="13" y="4" width="7" height="7" rx="1.5"/><rect x="4" y="13" width="7" height="7" rx="1.5"/><rect x="13" y="13" width="7" height="7" rx="1.5"/>'), run: () => moveToSpace(note) },
    { separator: true },
    { label: "Print…", icon: icon('<path d="M7 9V4h10v5"/><rect x="4" y="9" width="16" height="7" rx="2"/><path d="M7 14h10v6H7z"/>'), hint: "⌘P", run: () => invoke("print_window").catch((error) => toast(String(error))) },
    {
      label: "Move completed to-dos to the bottom",
      icon: icon('<rect x="4" y="4" width="16" height="16" rx="3"/><path d="m8.5 12 2.5 2.5 4.5-5"/>'),
      run: () => {
        let moved = 0;
        for (const list of $("body").querySelectorAll("ul, ol")) {
          const items = [...list.children].filter((child) => child.tagName === "LI");
          const done = items.filter((item) => item.querySelector('input[type="checkbox"]:checked'));
          const todo = items.filter((item) => !item.querySelector('input[type="checkbox"]:checked'));
          if (!done.length) continue;
          for (const item of [...todo, ...done]) list.appendChild(item);
          moved += done.length;
        }
        if (moved) {
          scheduleSave();
          toast(plural(moved, "Moved {n} finished item down", "Moved {n} finished items down"));
        } else {
          toast("Nothing is ticked in this note");
        }
      },
    },
    { label: "Spell check dictionary", icon: icon('<path d="M4 5h9l7 7-9 9-7-7z"/>'), run: () => { activeTab = "editor"; settingsAtRoot = false; openSettings(); } },
    { separator: true },
    { label: "Text direction: left to right", icon: icon('<path d="M4 6h16"/><path d="M4 12h10"/><path d="M4 18h13"/>'), run: () => applyDirection("ltr") },
    { label: "Text direction: right to left", icon: icon('<path d="M4 6h16"/><path d="M10 12h10"/><path d="M7 18h13"/>'), run: () => applyDirection("rtl") },
    { label: "Text direction: follow the text", icon: icon('<path d="M4 6h16"/><path d="M7 12h10"/><path d="M4 18h16"/>'), run: () => applyDirection("auto") },
    { separator: true },
    { label: note.trashed ? "Restore from Trash" : "Move to trash", icon: icon('<path d="M4 7h16"/><path d="M9 7V4.5h6V7"/><path d="M6 7l1 13h10l1-13"/>'), hint: "⌘⌫", danger: !note.trashed, run: () => invoke("set_flags", { id: note.id, pinned: null, starred: null, trashed: !note.trashed }).then(refresh) },
  ];
  const shown = isMobile() ? items.filter((item) => !["Export…", "Print…"].includes(item.label)) : items;
  Dialog.menu(rect.right - 240, rect.bottom + 6, shown, { anchor: rect, align: "end" });
});

$("pin").addEventListener("click", () => toggleFlag("pinned"));

$("print").addEventListener("click", async () => {
  const note = currentNote();
  if (!note) return;
  await flush();
  const use = await Dialog.pick({
    title: "Print or save as PDF",
    placeholder: "Choose",
    items: [
      { label: "Print…", sub: "The system print panel, with Save as PDF", value: "print" },
      { label: "Open a clean page in the browser", sub: "Older fallback", value: "browser" },
    ],
  });
  if (!use) return;
  try {
    if (use === "print") await invoke("print_window");
    else await invoke("print_note", { id: note.id });
  } catch (error) {
    toast(String(error));
  }
});

/** Copy text, falling back to the system clipboard when the web API is off. */
// ------------------------------------------------------------ picture export

/** A PNG of the note, drawn by the page itself. */
async function exportPng(which = null, asPdf = false) {
  const note = which || currentNote();
  if (!note) return toast("Pick a note first");
  if (note.id === state.current) await flush();

  const width = Math.min(820, Math.max(480, $("body").clientWidth || 720));
  const source = document.createElement("div");
  if (note.id === state.current) {
    source.innerHTML = $("body").innerHTML;
  } else {
    source.innerHTML = Editor.load(state.decrypted.get(note.id) ?? note.body);
  }
  const clone = source;

  // Pictures have to travel inside the picture, or the capturing image
  // can't reach them.
  for (const image of [...clone.querySelectorAll("img")]) {
    try {
      const response = await fetch(image.getAttribute("src"));
      const blob = await response.blob();
      const data = await new Promise((resolve) => {
        const reader = new FileReader();
        reader.onload = () => resolve(reader.result);
        reader.readAsDataURL(blob);
      });
      image.setAttribute("src", data);
    } catch {
      image.remove();
    }
  }

  const title = (note.title || t("Untitled")).trim();
  const dark = document.documentElement.dataset.theme === "dark";
  const paper = dark ? "#242529" : "#ffffff";
  const ink = dark ? "#e8e9ec" : "#1f2328";
  const css = `
    .shot { width:${width}px; padding:36px 40px; box-sizing:border-box;
      background:${paper}; color:${ink}; font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;
      font-size:15px; line-height:1.6; }
    .shot h1 { font-size:1.6em; margin:.3em 0 .2em; }
    .shot h2 { font-size:1.35em; margin:.5em 0 .2em; }
    .shot h3 { font-size:1.15em; margin:.5em 0 .2em; }
    .shot blockquote { border-left:3px solid #e6e8eb; margin:10px 0; padding-left:12px; color:#6b727d; }
    .shot code, .shot pre { font-family:ui-monospace,Menlo,monospace; }
    .shot pre { background:#f4f5f7; padding:10px 12px; border-radius:8px; white-space:pre-wrap; }
    .shot img { max-width:100%; }
    .shot table { border-collapse:collapse; }
    .shot td, .shot th { border:1px solid #d8dbe0; padding:5px 8px; }
    .shot .note-title { font-size:26px; font-weight:600; margin:0 0 14px; }`;

  // Measure the page before capturing it: the SVG needs real dimensions.
  const sheet = document.createElement("div");
  sheet.className = "shot";
  sheet.setAttribute("style", "position:fixed;left:-20000px;top:0;");
  sheet.innerHTML =
    `<style>${css}</style><div class="note-title">` +
    title.replace(/&/g, "&amp;").replace(/</g, "&lt;") +
    `</div>` +
    clone.innerHTML;
  document.body.appendChild(sheet);
  const height = Math.max(120, Math.ceil(sheet.getBoundingClientRect().height));
  const html = sheet.outerHTML.replace(/ style="position:fixed;left:-20000px;top:0;"/, "");
  sheet.remove();

  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">` +
    `<foreignObject width="${width}" height="${height}">` +
    `<div xmlns="http://www.w3.org/1999/xhtml"><style>${css}</style>${html}</div>` +
    `</foreignObject></svg>`;

  const image = new Image();
  const loaded = new Promise((resolve, reject) => {
    image.onload = () => resolve();
    image.onerror = () => reject(new Error("The page couldn't be drawn"));
  });
  image.src = "data:image/svg+xml;charset=utf-8," + encodeURIComponent(svg);
  try {
    await loaded;
  } catch (error) {
    return toast(String(error));
  }

  const scale = 2;
  const canvas = document.createElement("canvas");
  canvas.width = width * scale;
  canvas.height = height * scale;
  const context = canvas.getContext("2d");
  context.scale(scale, scale);
  context.fillStyle = paper;
  context.fillRect(0, 0, canvas.width, canvas.height);
  context.drawImage(image, 0, 0);

  if (asPdf) {
    const jpeg = await new Promise((resolve) => canvas.toBlob(resolve, "image/jpeg", 0.92));
    if (!jpeg) return toast("The page couldn't be drawn");
    const bytes = new Uint8Array(await jpeg.arrayBuffer());
    return invoke("save_pdf", bytes, {
      headers: {
        "x-name": encodeURIComponent(title.replace(/[\\/:]/g, "-")),
        "x-width": String(canvas.width),
        "x-height": String(canvas.height),
      },
    });
  }

  const blob = await new Promise((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!blob) return toast("The picture couldn't be made");
  const bytes = new Uint8Array(await blob.arrayBuffer());
  const path = await invoke("save_png", bytes, {
    headers: { "x-name": encodeURIComponent(title.replace(/[\\/:]/g, "-")) },
  });
  return path;
}

async function copyText(text) {
  try {
    if (navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
      toast("Copied");
      return;
    }
  } catch (error) {
    // fall through to the system tool
  }
  try {
    await invoke("copy_text", { text });
    toast("Copied");
  } catch (error) {
    toast(t("Couldn't copy: {error}", { error: String(error) }));
  }
}

/** Force the direction of the blocks in the selection, or of the note. */
function applyDirection(dir) {
  const selection = window.getSelection();
  const blocks = new Set();
  if (selection && selection.rangeCount && !selection.isCollapsed) {
    const range = selection.getRangeAt(0);
    for (const child of $("body").children) {
      if (range.intersectsNode(child)) blocks.add(child);
    }
  }
  if (!blocks.size) {
    for (const child of $("body").children) blocks.add(child);
  }
  for (const block of blocks) {
    if (dir === "auto") block.removeAttribute("dir");
    else block.setAttribute("dir", dir);
  }
  scheduleSave();
  toast(dir === "auto" ? "Direction follows the text" : `Direction: ${dir.toUpperCase()}`);
}

/** Several notes become one; the originals go to the trash. */
/** Show a locked note for this session, after the passcode. The lock stays. */
async function openLockedNote(id) {
  const note = state.notes.find((n) => n.id === id);
  if (!note) return;
  const code = await Dialog.ask({ title: "Unlock this note", label: "Your app passcode.", password: true, ok: "Unlock" });
  if (code === null) return;
  try {
    // The backend opens the sealed text and keeps the passcode for this
    // session, so what is typed is sealed again when it is saved.
    const plain = await rawInvoke("unlock_note", { id: note.id, passcode: code });
    state.unlocked.add(note.id);
    state.decrypted.set(note.id, plain);
    await refresh();
  } catch (error) {
    toast(String(error));
  }
}

/** Lock a note, or remove its lock for good, behind the app passcode. */
async function toggleNoteLock(note) {
  const locking = !(note.sealed ?? note.locked);
  const code = await Dialog.ask({
    title: locking ? "Lock this note" : "Unlock this note",
    label: locking
      ? "It is hidden behind your app passcode (set one with the lock button at the top first)."
      : "Your app passcode. The note stays unlocked after this.",
    password: true,
    placeholder: "",
    ok: locking ? "Lock" : "Unlock",
  });
  if (code === null) return;
  try {
    if (locking) {
      await flush();
      await invoke("lock_note", { id: note.id, passcode: code });
      state.unlocked.delete(note.id);
      state.decrypted.delete(note.id);
      await refresh();
      toast("Note locked — its text is encrypted now");
    } else {
      // For good: the text is stored readable again.
      await invoke("remove_note_lock", { id: note.id, passcode: code });
      state.unlocked.delete(note.id);
      state.decrypted.delete(note.id);
      await refresh();
      toast("Lock removed");
    }
  } catch (error) {
    toast(String(error));
  }
}

function escapeText(text) {
  return String(text).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** A note's own copies of pictures and files that `body` borrows from other
 *  notes, so emptying the trash later can't break them. */
async function ownCopies(id, body) {
  try {
    return await rawInvoke("adopt_attachments", { id, body });
  } catch {
    return body;
  }
}

async function mergeNotes(ids) {
  const notes = ids
    .map((id) => state.notes.find((note) => note.id === id))
    .filter(Boolean);
  if (notes.length < 2) return;
  if (notes.some(isHidden)) return toast("Unlock the locked notes first");

  const yes = await Dialog.confirm({
    title: `Merge ${notes.length} notes?`,
    label: "They become one note, joined in the order shown. The originals move to the trash.",
    ok: "Merge",
    danger: false,
  });
  if (!yes) return;

  const title = notes
    .map((note) => note.title.trim() || t("Untitled"))
    .join(" + ")
    .slice(0, 120);
  const body = notes
    .map((note) => `<h2>${escapeText(note.title.trim() || t("Untitled"))}</h2>${note.body}`)
    .join("<hr>");
  const first = notes[0];

  await invoke("create_note", { notebookId: first.notebook_id, space: first.space }).then(async (id) => {
    await invoke("save_note", { id, title, body: await ownCopies(id, body) });
    for (const note of notes) {
      await invoke("set_flags", { id: note.id, pinned: null, starred: null, trashed: true });
    }
    state.selected.clear();
    await refresh();
    selectNote(id);
    toast("Merged into one note", {
      label: "Undo",
      run: async () => {
        for (const original of notes) {
          await invoke("set_flags", { id: original.id, pinned: null, starred: null, trashed: false });
        }
        // It isn't in the trash, and only trashed notes can be deleted.
        await invoke("set_flags", { id, pinned: null, starred: null, trashed: true });
        await invoke("empty_trash", { ids: [id] });
        state.current = notes[0].id;
        await refresh();
      },
    });
  });
}

async function exportPngMany(ids) {
  let saved = 0;
  for (const id of ids) {
    const note = state.notes.find((n) => n.id === id);
    if (!note) continue;
    try {
      await exportPng(note);
      saved += 1;
    } catch (error) {
      toast(String(error));
    }
  }
  state.selected.clear();
  await renderCards();
  toast(plural(saved, "Saved {n} picture to Downloads", "Saved {n} pictures to Downloads"));
}

async function exportMany(ids) {
  const mode = await Dialog.pick({
    title: "Export notes",
    placeholder: "Choose a format",
    items: [
      { label: "Markdown", sub: "One .md file per note", value: "md" },
      { label: "HTML", sub: "One .html file per note", value: "html" },
      { label: "Text", sub: "One .txt file per note", value: "txt" },
      { label: "Picture (PNG)", sub: "One .png per note, as it looks", value: "png" },
    ],
  });
  if (!mode) return;
  if (mode === "png") return exportPngMany(ids);
  let saved = 0;
  for (const id of ids) {
    try {
      await invoke("export_note", { id, format: mode });
      saved += 1;
    } catch (error) {
      toast(String(error));
    }
  }
  state.selected.clear();
  await renderCards();
  toast(plural(saved, "Exported {n} note to Downloads", "Exported {n} notes to Downloads"));
}

async function trashMany(ids) {
  const yes = await Dialog.confirm({
    title: plural(ids.length, "Move {n} note to Trash?", "Move {n} notes to Trash?"),
    label: "You can bring them back from the trash.",
  });
  if (!yes) return;
  for (const id of ids) {
    await invoke("set_flags", { id, pinned: null, starred: null, trashed: true });
  }
  state.selected.clear();
  await refresh();
}

async function addTags(note) {
  const value = await Dialog.ask({
    title: "Add tags",
    label: "Separate several with spaces. They are written into the note as #tags.",
    placeholder: "work ideas",
    ok: "Add",
  });
  if (!value || !value.trim()) return;
  const tags = value
    .split(/[\s,]+/)
    .map((tag) => tag.replace(/^#/, "").trim())
    .filter(Boolean)
    .map((tag) => `#${tag}`)
    .join(" ");
  if (!tags) return;
  await invoke("save_note", { id: note.id, title: note.title, body: `${note.body}<p>${tags}</p>` });
  await refresh();
  toast("Tags added");
}

async function moveToSpace(note) {
  const name = await Dialog.ask({
    title: "Move to space",
    label: "Type the name of a space; a new one is created if needed.",
    value: note.space,
    ok: "Move",
  });
  if (!name || !name.trim() || name.trim() === note.space) return;
  try {
    await invoke("move_note_space", { id: note.id, space: name.trim() });
    await refresh();
    toast(t("Moved to {name}", { name: name.trim() }));
  } catch (error) {
    toast(String(error));
  }
}

async function moveNote(note) {
  const id = await Dialog.pick({
    title: "Move to notebook",
    placeholder: "Find a notebook",
    items: state.notebooks.map((b) => ({
      label: b.name,
      sub: b.id === note.notebook_id ? "Current notebook" : `${b.count} notes`,
      value: b.id,
      colour: COLOURS[b.color % COLOURS.length],
    })),
  });
  if (id === null || id === note.notebook_id) return;
  await flush();
  await invoke("move_note", { id: note.id, notebook: id });
  await refresh();
  toast(t("Moved to {name}", { name: state.notebooks.find((b) => b.id === id)?.name || t("notebook") }));
}

/**
 * Which notebooks is this note in? A note can have one main notebook
 * (`notebook_id`, where its file is written) and any number of extras
 * (`also_in`). Ticking and unticking here keeps both in step: at least one
 * notebook must remain, and if the main one is removed the first remaining
 * notebook takes over.
 */
async function chooseNotebooks(note) {
  await flush();
  const current = new Set(note.notebooks || [note.notebook_id]);
  const ids = await Dialog.multiPick({
    title: "Notebooks",
    hint: "A note can be in more than one notebook.",
    hintWhenEmpty: "No notebook — it stays uncategorized.",
    placeholder: "Find a notebook",
    empty: "No notebooks yet",
    minimum: 0,
    items: state.notebooks.map((book) => ({
      label: book.name,
      sub: book.id === note.notebook_id
        ? t("Main notebook")
        : plural(book.count, "{n} note", "{n} notes"),
      value: book.id,
      colour: COLOURS[book.color % COLOURS.length],
      checked: current.has(book.id),
    })),
  });
  if (ids === null) return;

  if (!ids.length) {
    if (note.notebook_id === 0) return;
    try {
      await invoke("move_note", { id: note.id, notebook: 0 });
      await refresh();
      toast("Moved to Uncategorized");
    } catch (error) {
      toast(String(error));
      await refresh();
    }
    return;
  }

  const wanted = new Set(ids);
  const oldPrimary = note.notebook_id;
  const primary = wanted.has(oldPrimary) ? oldPrimary : ids[0];
  const unchanged = primary === oldPrimary
    && ids.length === current.size
    && ids.every((id) => current.has(id));
  if (unchanged) return;

  try {
    if (primary !== oldPrimary) {
      await invoke("move_note", { id: note.id, notebook: primary });
      current.delete(oldPrimary);
      current.add(primary);
      // The old main notebook can also be in `also_in`; moving alone
      // doesn't remove it there.
      if (!wanted.has(oldPrimary)) {
        await invoke("remove_from_notebook", { id: note.id, notebook: oldPrimary });
      }
    }
    for (const id of ids) {
      if (!current.has(id)) await invoke("add_to_notebook", { id: note.id, notebook: id });
    }
    for (const id of current) {
      if (!wanted.has(id)) await invoke("remove_from_notebook", { id: note.id, notebook: id });
    }
    await refresh();
    toast("Notebooks updated");
  } catch (error) {
    toast(String(error));
    await refresh();
  }
}

$("chip-button").addEventListener("click", () => {
  const note = currentNote();
  if (note) chooseNotebooks(note);
});
$("star").addEventListener("click", () => toggleFlag("starred"));

$("trash-note").addEventListener("click", async () => {
  const note = currentNote();
  if (!note) return;
  await invoke("set_flags", { id: note.id, pinned: null, starred: null, trashed: !note.trashed });
  await refresh();
});

$("title").addEventListener("input", scheduleSave);
$("body").addEventListener("input", () => {
  scheduleSave();
  updateWords();
});

const SORT_LABELS = {
  updated: "Last edited",
  created: "Date created",
  title: "Title",
  manual: "Manual",
};

function paintSort() {
  const label = SORT_LABELS[sortOrder()] || SORT_LABELS.updated;
  $("sort").title = `${t("Sort notes")}: ${t(label)}`;
}

async function chooseSort(value) {
  await setUiPref(sortKey(), value);
  renderCards();
  paintSort();
}

$("sort").addEventListener("click", () => {
  const button = $("sort");
  const rect = button.getBoundingClientRect();
  const current = sortOrder();
  Dialog.menu(rect.left, rect.bottom + 4, Object.entries(SORT_LABELS).map(([value, label]) => ({
    label,
    check: value === current,
    checked: value === current,
    run: () => chooseSort(value),
  })), { anchor: rect, align: "end" });
});

$("search-scope").addEventListener("click", () => {
  state.titleOnly = !state.titleOnly;
  $("search-scope").classList.toggle("on", !!state.titleOnly);
  renderCards();
});

$("search").addEventListener("focus", () => {
  if (!isMobile()) return;
  // Results appear in the note list, which Browse covers.
  closeMobileNav();
  document.body.classList.add("mobile-search");
  // Android can drop the IME request made while the search chrome is changing.
  if (PLATFORM === "android") {
    setTimeout(() => {
      try { window.OwnNotesAndroid?.showKeyboard?.(); } catch {}
    }, 80);
  }
  syncMobileTabs();
  syncMobileBack();
});

$("search").addEventListener("input", (event) => {
  state.search = event.target.value;
  state.savedIndex = null;
  $("save-search").hidden = !state.search.trim();
  renderCards();
  if (isMobile() && state.search.trim()) closeMobileEditor();
});

$("new-note").addEventListener("click", async () => {
  await flush();
  const notebook =
    state.view.kind === "notebook" ? state.view.id : state.view.kind === "uncategorized" ? 0 : state.notebooks[0]?.id || 1;
  const id = await invoke("create_note", { notebookId: notebook, space: state.space });
  await refresh();
  state.current = id;
  state.view = { kind: "notebook", id: notebook };
  render();
  if (isMobile()) openMobileEditor();
  $("title").focus();
});

$("new-note-2").addEventListener("click", () => $("new-note").click());

$("new-notebook").addEventListener("click", async () => {
  const name = await Dialog.ask({
    title: "New notebook",
    value: `Notebook ${state.notebooks.length + 1}`,
    ok: "Create",
  });
  if (!name || !name.trim()) return;
  const id = await invoke("create_notebook", { name, parent: null });
  await refresh();
  setView({ kind: "notebook", id });
});

$("toggle-sidebar").addEventListener("click", () => {
  if (isMobile()) {
    toggleMobileNav();
    return;
  }
  const sidebar = $("sidebar");
  const hidden = sidebar.classList.toggle("is-hidden");
  // `inert` keeps the collapsed rows out of the tab order as well.
  sidebar.toggleAttribute("inert", hidden);
  sidebar.setAttribute("aria-hidden", hidden ? "true" : "false");

  clearTimeout(sidebarTimer);
  sidebar.classList.remove("sidebar-enter");
  if (!hidden) {
    // Let the rows catch up one after another while the pane unfolds.
    [...sidebar.querySelectorAll(".row, .section-title")].forEach((el, i) => {
      el.style.setProperty("--i", Math.min(i, 10));
    });
    void sidebar.offsetWidth;
    sidebar.classList.add("sidebar-enter");
    sidebarTimer = setTimeout(() => sidebar.classList.remove("sidebar-enter"), 520);
  }
});

$("mobile-back").addEventListener("click", closeMobileEditor);
$("mobile-scrim").addEventListener("click", closeMobileNav);

for (const tab of document.querySelectorAll("[data-mobile-tab]")) {
  tab.addEventListener("click", () => {
    const name = tab.dataset.mobileTab;
    // Settings is a tab like the others, so any tab leaves it.
    if (!$("settings-modal").hidden) $("s-close").click();
    if (name === "notes") {
      // Tapping the tab you are already on goes back to the top.
      if (tab.classList.contains("on")) $("list-pane").scrollTo({ top: 0, behavior: "smooth" });
      closeMobileNav();
      closeMobileEditor();
      document.body.classList.remove("mobile-search");
      $("search").value = "";
      state.search = "";
      $("save-search").hidden = true;
      $("search").blur();
      renderCards();
    } else if (name === "browse") {
      closeMobileEditor();
      document.body.classList.remove("mobile-search");
      $("search").blur();
      toggleMobileNav();
    } else if (name === "search") {
      closeMobileNav();
      closeMobileEditor();
      document.body.classList.add("mobile-search");
      $("search").focus();
    } else if (name === "settings") {
      closeMobileNav();
      closeMobileEditor();
      document.body.classList.remove("mobile-search");
      $("search").blur();
      openSettings();
    }
    syncMobileTabs();
    syncMobileBack();
  });
}

$("mobile-lock").addEventListener("click", () => {
  closeMobileNav();
  $("lock").click();
});

$("mobile-compose").addEventListener("click", () => {
  closeMobileNav();
  closeMobileEditor();
  $("new-note").click();
});

$("mobile-new-note").addEventListener("click", () => {
  closeMobileNav();
  closeMobileEditor();
  $("new-note").click();
});

$("mobile-sort").addEventListener("click", () => $("sort").click());
$("mobile-search-cancel").addEventListener("click", leaveMobileSearch);

/** iOS shrinks only the visual viewport for the keyboard, so the app follows it. */
function trackIosKeyboard() {
  const viewport = window.visualViewport;
  if (PLATFORM !== "ios" || !viewport) return;
  const root = document.documentElement;
  const update = () => {
    const keyboard = Math.max(0, Math.round(window.innerHeight - viewport.height));
    root.style.setProperty("--keyboard-inset", keyboard + "px");
    root.classList.toggle("keyboard-open", keyboard > 80);
    // WebKit scrolls the whole page to reveal a field; the panes scroll instead.
    if (window.scrollY) window.scrollTo(0, 0);
  };
  viewport.addEventListener("resize", update);
  viewport.addEventListener("scroll", update);
}

/** The iOS back swipe: the editor follows a finger from the leading edge. */
function armIosSwipeBack() {
  if (PLATFORM !== "ios") return;
  const editor = $("editor-pane");
  let start = null;
  let moved = 0;
  let tracking = false;
  const settle = (done) => {
    const width = editor.offsetWidth;
    const rtl = document.documentElement.dir === "rtl";
    editor.style.transition = "transform 240ms cubic-bezier(0.2, 0.8, 0.2, 1)";
    editor.style.transform = done ? `translateX(${rtl ? -width : width}px)` : "translateX(0)";
    setTimeout(() => {
      editor.style.transition = "";
      editor.style.transform = "";
      if (done) closeMobileEditor();
      document.body.classList.remove("mobile-swiping");
    }, 240);
  };
  editor.addEventListener("touchstart", (event) => {
    start = null;
    if (!document.body.classList.contains("mobile-editor") || event.touches.length !== 1) return;
    const touch = event.touches[0];
    const rtl = document.documentElement.dir === "rtl";
    if ((rtl ? window.innerWidth - touch.clientX : touch.clientX) > 24) return;
    start = { x: touch.clientX, y: touch.clientY, rtl };
    moved = 0;
    tracking = false;
  }, { passive: true });
  editor.addEventListener("touchmove", (event) => {
    if (!start) return;
    const touch = event.touches[0];
    const across = (touch.clientX - start.x) * (start.rtl ? -1 : 1);
    if (!tracking) {
      if (Math.abs(touch.clientY - start.y) > 10 && Math.abs(touch.clientY - start.y) > across) {
        start = null;
        return;
      }
      if (across < 10) return;
      tracking = true;
      document.body.classList.add("mobile-swiping");
    }
    event.preventDefault();
    moved = Math.max(0, across);
    editor.style.transform = `translateX(${start.rtl ? -moved : moved}px)`;
  }, { passive: false });
  const finish = () => {
    if (start && tracking) settle(moved > editor.offsetWidth * 0.33);
    start = null;
    tracking = false;
  };
  editor.addEventListener("touchend", finish);
  editor.addEventListener("touchcancel", finish);
}

/** The Material touch ripple, spreading from where the finger landed. */
function armAndroidRipple() {
  if (PLATFORM !== "android") return;
  const hosts = ".card, .row, .mobile-tabs button, .icon, .ghost, .primary, .mobile-compose, .context-menu button, .settings-tabs button, .format-bar button";
  document.addEventListener("pointerdown", (event) => {
    const host = event.target.closest?.(hosts);
    if (!host) return;
    const box = host.getBoundingClientRect();
    const size = Math.max(box.width, box.height) * 2;
    const ink = document.createElement("span");
    ink.className = "ripple";
    ink.style.width = ink.style.height = size + "px";
    ink.style.left = event.clientX - box.left - size / 2 + "px";
    ink.style.top = event.clientY - box.top - size / 2 + "px";
    host.appendChild(ink);
    ink.addEventListener("animationend", () => ink.remove());
    // Hiding the host cancels the animation; left behind, it would replay on return.
    ink.addEventListener("animationcancel", () => ink.remove());
  }, { passive: true });
}

/** Keep Android's status-bar icons readable against the app's own theme. */
function followAndroidTheme() {
  const bridge = window.OwnNotesAndroid;
  if (PLATFORM !== "android" || !bridge) return;
  const root = document.documentElement;
  const update = () => {
    try { bridge.setLightBars(root.dataset.theme !== "dark"); } catch {}
  };
  new MutationObserver(update).observe(root, { attributes: true, attributeFilter: ["data-theme"] });
  update();
}

/** Keep what is being typed visible once the keyboard has shrunk the app. */
function revealFocusedField() {
  const field = document.activeElement;
  if (!field) return;
  const pages = field.closest?.(".settings-pages");
  if (pages && field.matches("input, select, textarea")) {
    const box = field.getBoundingClientRect();
    const view = pages.getBoundingClientRect();
    if (box.bottom > view.bottom - 24 || box.top < view.top + 80) {
      pages.scrollTop += box.top - (view.top + view.height / 2);
    }
    return;
  }
  if (field.id !== "body") return;
  const selection = window.getSelection();
  if (!selection?.rangeCount) return;
  const caret = selection.getRangeAt(0).getClientRects()[0];
  if (!caret) return;
  const limit = document.querySelector(".format-bar").getBoundingClientRect().top - 16;
  if (caret.bottom > limit) field.scrollTop += caret.bottom - limit;
}

// Both fire when the bar comes back too, such as after the keyboard or a rotation.
if (PLATFORM === "ios" || PLATFORM === "android") {
  new ResizeObserver(moveTabIndicator).observe($("mobile-tabs"));
  new ResizeObserver(() => requestAnimationFrame(revealFocusedField)).observe(document.querySelector(".app"));
  // The tabs swap sides with the language; the pill has to follow them.
  new MutationObserver(moveTabIndicator).observe(document.documentElement, { attributes: true, attributeFilter: ["dir"] });
}
trackIosKeyboard();
armIosSwipeBack();
armAndroidRipple();
followAndroidTheme();

// ------------------------------------------------------------ formatting

const bar = document.querySelector(".format-bar");
bar.addEventListener("mousedown", (event) => event.preventDefault());

for (const button of bar.querySelectorAll("button[data-cmd]")) {
  button.addEventListener("click", () => {
    document.execCommand(button.dataset.cmd, false, button.dataset.value || null);
    $("body").focus();
    scheduleSave();
  });
}

for (const swatch of bar.querySelectorAll("button[data-colour]")) {
  swatch.addEventListener("click", () => {
    document.execCommand("foreColor", false, swatch.dataset.colour);
    $("body").focus();
    scheduleSave();
  });
}

document.getElementById("checklist").addEventListener("click", () => {
  // A real checkbox: it can be ticked, counts as a to-do, and its tick is
  // saved (editor.js keeps the "checked" attribute in step).
  insertHTML('<ul class="checklist"><li><input type="checkbox">&nbsp;</li></ul>');
});
document.getElementById("note-link").addEventListener("click", insertNoteLink);
document.getElementById("collapse").addEventListener("click", () => {
  insertHTML(`<details open><summary>${t("Section")}</summary>${t("Write here…")}</details>`);
});

bar.querySelector("button[data-hl]").addEventListener("click", () => {
  document.execCommand("hiliteColor", false, "#f7e06e");
  $("body").focus();
  scheduleSave();
});

// ------------------------------------------------------------ settings

let toastTimer = null;
let showWelcome = false;
let listEnterTimer = 0;
let noteEnterTimer = 0;
let selectionTimer = 0;
let infoCloseTimer = 0;
let sidebarTimer = 0;
let navTimer = 0;

/** A short message, with an optional "Undo" button. */
function toast(message, action = null) {
  message = t(message);
  if (action) action = { ...action, label: t(action.label) };
  const el = $("toast");
  el.innerHTML = "";
  const text = document.createElement("span");
  text.textContent = message;
  el.appendChild(text);
  if (action) {
    const button = document.createElement("button");
    button.className = "toast-action";
    button.textContent = action.label;
    button.addEventListener("click", async () => {
      el.hidden = true;
      clearTimeout(toastTimer);
      try {
        await action.run();
      } catch (error) {
        toast(String(error));
      }
    });
    el.appendChild(button);
  }
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.hidden = true), action ? 7000 : 2600);
}

const ACCENTS = [
  ["#1f6fd1", "#e8f0fc"],
  ["#7c5cd6", "#efeafc"],
  ["#0fa3a3", "#e3f6f6"],
  ["#3ea66b", "#e8f6ee"],
  ["#e58a2e", "#fdf1e3"],
  ["#d64545", "#fcecec"],
  ["#c25bb0", "#f9ecf6"],
  ["#55606e", "#eef0f2"],
];

async function openSettings() {
  const [url, user, connected] = await invoke("sync_status");
  $("s-url").value = url;
  $("s-user").value = user;
  $("s-pass").value = "";
  savedSync = { url, user };
  if (syncHeld) syncStatus("Sync is paused until you choose what to sync.", "warn");
  else if (connected && AutoSync.last)
    syncStatus(t("Last synced {time}.", { time: AutoSync.last.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }) }), "good");
  else syncStatus(connected ? "Set up. Press Test to check the connection." : "Not set up yet.");
  const options = await rawInvoke("sync_options").catch(() => AutoSync.options);
  $("s-on-change").checked = options.sync_on_change;
  $("s-every").value = String(options.auto_sync_minutes);
  if (!$("s-every").value) $("s-every").value = "1";
  $("settings-modal").hidden = false;
  renderSettings();
  paintConnect();
  translateDom($("settings-modal"));
}

function renderSettings() {
  syncSettingsNav();
  for (const button of document.querySelectorAll(".settings-tabs button")) {
    button.classList.toggle("active", button.dataset.tab === activeTab);
    if (!button.dataset.en) button.dataset.en = button.textContent;
    button.textContent = t(button.dataset.en);
  }
  for (const page of document.querySelectorAll(".settings-pages section")) {
    page.hidden = page.dataset.page !== activeTab;
  }

  const named = pref("theme_name", "");
  for (const button of document.querySelectorAll("#theme-mode button")) {
    button.classList.toggle("active", !named && button.dataset.mode === state.prefs.theme);
  }
  const looks = $("theme-looks");
  if (looks) {
    looks.innerHTML = "";
    for (const name of NAMED_THEMES) {
      const dot = document.createElement("button");
      dot.className = "theme-chip" + (named === name ? " on" : "");
      dot.textContent = name;
      dot.addEventListener("click", () => chooseTheme(name));
      looks.appendChild(dot);
    }
    const none = document.createElement("button");
    none.className = "theme-chip" + (named ? "" : " on");
    none.textContent = t("Default");
    none.addEventListener("click", () => chooseTheme(state.prefs.theme));
    looks.appendChild(none);
  }

  const accents = $("accent-row");
  accents.innerHTML = "";
  ACCENTS.forEach(([colour], index) => {
    const dot = document.createElement("button");
    dot.style.setProperty("--c", colour);
    dot.classList.toggle("on", index === (state.prefs.accent || 0));
    dot.addEventListener("click", () => {
      state.prefs.accent = index;
      applyPrefs();
      savePrefs();
      renderSettings();
    });
    accents.appendChild(dot);
  });

  $("t-glass").checked = pref("glass", true);

  $("g-counts").checked = state.prefs.show_counts !== false;
  $("g-compact").checked = !!state.prefs.compact_list;
  $("g-subnotes").checked = pref("show_sub_notes", true);
  $("g-language").value = pref("lang", "en");
  $("g-drop").value = dropAction();
  $("g-backup-hours").value = String(pref("backup_hours", 24));
  $("g-backup-keep").value = String(pref("backup_keep", 7));

  const books = $("g-notebooks");
  books.innerHTML = "";
  for (const book of state.notebooks) {
    const row = document.createElement("div");
    row.className = "nb-row";

    const dot = document.createElement("button");
    dot.className = "nb-dot";
    dot.style.setProperty("--c", COLOURS[book.color % COLOURS.length]);
    dot.addEventListener("click", async () => {
      await invoke("colour_notebook", { id: book.id, colour: (book.color + 1) % COLOURS.length });
      await refresh();
      renderSettings();
    });

    const input = document.createElement("input");
    input.type = "text";
    input.value = book.name;
    input.addEventListener("change", async () => {
      await invoke("rename_notebook", { id: book.id, name: input.value });
      await refresh();
    });

    const remove = document.createElement("button");
    remove.className = "ghost";
    remove.textContent = t("Delete");
    remove.addEventListener("click", async () => {
      const yes = await Dialog.confirm({
        title: `Delete "${book.name}"?`,
        label: "Its notes move to another notebook. Nothing is lost.",
      });
      if (!yes) return;
      await invoke("remove_notebook", { id: book.id });
      await refresh();
      renderSettings();
    });

    row.append(dot, input, remove);
    books.appendChild(row);
  }

  $("e-size").value = state.prefs.editor_font_size || 15;
  $("e-spell").value = pref("spell_lang", "");
  $("e-font").value = pref("editor_font", "");
  $("e-markdown").checked = pref("markdown_mode", false);
  $("e-image").value = String(pref("image_size", 100));
  $("e-size-label").textContent = `${state.prefs.editor_font_size || 15}px`;
  if (typeof Editor !== "undefined") Editor.renderSettings();
  // Some rows are only added once their setting is known.
  groupSettingsRows();
}

let activeTab = "sync";
// Phones open Settings on a list of its pages; a deep link clears this first.
let settingsAtRoot = true;

for (const button of document.querySelectorAll(".settings-tabs button")) {
  button.addEventListener("click", () => {
    activeTab = button.dataset.tab;
    settingsAtRoot = false;
    renderSettings();
    document.querySelector(".settings-pages").scrollTop = 0;
    syncMobileBack();
  });
}

$("settings-back").addEventListener("click", () => {
  settingsAtRoot = true;
  renderSettings();
});

/** The header of Settings on a phone: the list, or the page pushed over it. */
function syncSettingsNav() {
  const modal = $("settings-modal");
  const native = PLATFORM === "ios" || PLATFORM === "android";
  const atRoot = native && settingsAtRoot;
  modal.classList.toggle("at-root", atRoot);
  modal.classList.toggle("at-page", !atRoot);
  $("settings-back").hidden = atRoot;
  const tab = document.querySelector(`.settings-tabs button[data-tab="${activeTab}"]`);
  $("settings-title").textContent = atRoot ? t("Settings") : tab?.textContent || t("Settings");
  syncMobileTabs();
}

/** Runs of settings rows become one grouped list on a phone. */
function groupSettingsRows() {
  if (PLATFORM !== "ios" && PLATFORM !== "android") return;
  for (const section of document.querySelectorAll(".settings-pages section")) {
    let group = null;
    for (const child of [...section.children]) {
      if (child.isConnected && child.matches(".settings-group")) {
        group = child;
        continue;
      }
      // A caption above a control reads as one row with it.
      const control = child.nextElementSibling;
      if (child.matches("p.hint") && control?.matches("select, .size-slider")) {
        const row = document.createElement("div");
        row.className = "option-row";
        const name = document.createElement("span");
        name.append(...child.childNodes);
        child.replaceWith(row);
        row.append(name, control);
        group = group || document.createElement("div");
        if (!group.isConnected) {
          group.className = "settings-group";
          section.insertBefore(group, row);
        }
        group.appendChild(row);
        continue;
      }
      // The editor adds its own switch inside a wrapper it keeps a reference to.
      const isRow = child.matches("label, .check, .option-row") || child.querySelector(":scope > .check:only-child");
      if (!child.isConnected || !isRow || child.matches(".desktop-only")) {
        if (child.isConnected && child.parentElement === section) group = null;
        continue;
      }
      if (!group) {
        group = document.createElement("div");
        group.className = "settings-group";
        section.insertBefore(group, child);
      }
      group.appendChild(child);
    }
  }
}
groupSettingsRows();

for (const button of document.querySelectorAll("#theme-mode button")) {
  button.addEventListener("click", () => {
    state.prefs.theme = button.dataset.mode;
    applyPrefs();
    savePrefs();
    renderSettings();
  });
}

$("t-glass").addEventListener("change", async (event) => {
  const box = event.currentTarget;
  await setUiPref("glass", box.checked);
  applyGlass();
});

$("g-counts").addEventListener("change", () => {
  state.prefs.show_counts = $("g-counts").checked;
  applyPrefs();
  savePrefs();
  render();
});

$("g-drop").addEventListener("change", async () => {
  await setUiPref("drop_action", $("g-drop").value);
  toast($("g-drop").value === "add" ? "Dropping adds to the notebook" : "Dropping moves the note");
});

$("g-language").addEventListener("change", () => setLanguage($("g-language").value));

/** Change the interface language, and mirror the layout if it is RTL. */
async function setLanguage(code) {
  await setUiPref("lang", code);
  applyLanguageState(code);
  // The chrome is rebuilt so every string and panel follows at once.
  for (const button of document.querySelectorAll(".settings-tabs button")) {
    if (button.dataset.en) button.textContent = t(button.dataset.en);
  }
  render();
  renderSettings();
}

$("g-subnotes").addEventListener("change", async () => {
  await setUiPref("show_sub_notes", $("g-subnotes").checked);
  render();
});

$("g-compact").addEventListener("change", () => {
  state.prefs.compact_list = $("g-compact").checked;
  applyPrefs();
  savePrefs();
  render();
});

$("e-size").addEventListener("input", () => {
  state.prefs.editor_font_size = Number($("e-size").value);
  $("e-size-label").textContent = `${state.prefs.editor_font_size}px`;
  applyPrefs();
  savePrefs();
});

$("export-md").addEventListener("click", () => exportAll("md"));
$("export-html").addEventListener("click", () => exportAll("html"));

async function exportAll(format) {
  try {
    await flush();
    const path = await invoke("export_all", { format });
    $("export-path").textContent = t("Saved to {path}", { path });
    toast("Exported every note");
  } catch (error) {
    toast(String(error));
  }
}

function pickFilesForImport(kind) {
  const input = document.createElement("input");
  input.type = "file";
  input.multiple = kind === "md";
  input.accept =
    kind === "enex" ? ".enex" : kind === "simplenote" ? ".json" : ".md,.markdown,.txt";
  input.addEventListener("change", async () => {
    const files = [...input.files];
    if (!files.length) return;
    syncStatus("Importing…");
    try {
      const notebook = state.view.kind === "notebook" ? state.view.id : null;
      let count;
      if (kind === "enex") {
        count = await invoke("import_enex", { text: await files[0].text(), notebook, space: state.space });
      } else if (kind === "simplenote") {
        count = await invoke("import_simplenote", { text: await files[0].text(), notebook, space: state.space });
      } else {
        const pairs = [];
        for (const file of files) pairs.push([file.name, await file.text()]);
        count = await invoke("import_markdown", { files: pairs, notebook, space: state.space });
      }
      await refresh();
      toast(plural(count, "Imported {n} note", "Imported {n} notes"));
    } catch (error) {
      toast(String(error));
    }
  });
  input.click();
}

$("import-enex").addEventListener("click", () => pickFilesForImport("enex"));
$("import-md").addEventListener("click", () => pickFilesForImport("md"));
$("import-simplenote").addEventListener("click", () => pickFilesForImport("simplenote"));
$("import-bear").addEventListener("click", async () => {
  const input = document.createElement("input");
  input.type = "file";
  input.accept = ".bear,.zip,.textbundle";
  input.addEventListener("change", async () => {
    const file = input.files && input.files[0];
    if (!file) return;
    syncStatus("Importing…");
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const notebook = state.view.kind === "notebook" ? state.view.id : null;
      const count = await invoke("import_bear", bytes);
      await refresh();
      toast(plural(count, "Imported {n} note from Bear", "Imported {n} notes from Bear"));
    } catch (error) {
      toast(String(error));
    }
  });
  input.click();
});

$("import-apple").addEventListener("click", async () => {
  const yes = await Dialog.confirm({
    title: "Import from Apple Notes?",
    label: "OwnNotes reads every note from the Notes app on this Mac and adds a copy here. Your notes in Notes stay as they are. macOS may ask whether OwnNotes can control Notes.",
    ok: "Import",
    danger: false,
  });
  if (!yes) return;
  toast("Reading notes from Apple Notes…");
  try {
    const notebook = state.view.kind === "notebook" ? state.view.id : null;
    const result = await invoke("import_apple_notes", { notebook, space: state.space });
    await refresh();
    const skipped = result.locked
      ? " " + plural(result.locked, "({n} locked note skipped)", "({n} locked notes skipped)")
      : "";
    toast(plural(result.imported, "Imported {n} note", "Imported {n} notes") + skipped);
  } catch (error) {
    toast(String(error));
  }
});

$("e-markdown").addEventListener("change", async () => {
  await setUiPref("markdown_mode", $("e-markdown").checked);
  toast($("e-markdown").checked ? "Markdown: on" : "Markdown: off");
});

$("e-font").addEventListener("change", async () => {
  await setUiPref("editor_font", $("e-font").value);
  applyFont();
});

function applyLanguage() {
  applyLanguageState(pref("lang", "en"));
}

function applyFont() {
  const font = pref("editor_font", "");
  if (font) document.documentElement.dataset.font = font;
  else delete document.documentElement.dataset.font;
}

$("e-spell").addEventListener("change", async () => {
  await setUiPref("spell_lang", $("e-spell").value);
  applySpelling();
});

$("e-image").addEventListener("change", async () => {
  await setUiPref("image_size", Number($("e-image").value));
  toast(t("New pictures at {size}", { size: $("e-image").selectedOptions[0].textContent }));
});

function applySpelling() {
  const lang = pref("spell_lang", "");
  const body = $("body");
  if (lang) {
    body.setAttribute("lang", lang);
    body.setAttribute("spellcheck", "true");
  } else {
    body.removeAttribute("lang");
    body.setAttribute("spellcheck", "false");
  }
}

$("g-backup-hours").addEventListener("change", () => {
  setUiPref("backup_hours", Number($("g-backup-hours").value));
  toast(t("Backups: {when}", { when: $("g-backup-hours").selectedOptions[0].textContent }));
});

$("g-backup-keep").addEventListener("change", () => {
  setUiPref("backup_keep", Number($("g-backup-keep").value));
  toast(t("Keeping the last {n}", { n: $("g-backup-keep").value }));
});

$("g-backup-open").addEventListener("click", async () => {
  try {
    await invoke("open_backups");
  } catch (error) {
    toast(String(error));
  }
});

$("g-backup").addEventListener("click", async () => {
  const path = await invoke("make_backup");
  $("g-backup-path").textContent = t("Saved to {path}", { path });
});

$("g-delete").addEventListener("click", async () => {
  const yes = await Dialog.confirm({
    title: "Delete everything?",
    label: "Every note, the sync history and the WebDAV password go from this computer. Notes on your server are left alone.",
    ok: "Delete everything",
  });
  if (!yes) return;
  await invoke("delete_everything");
  location.reload();
});

$("info-close").addEventListener("click", closeInfoPanel);

// "System" follows the computer's light or dark mode, also when it changes.
const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");
const NAMED_THEMES = ["Sepia", "Nord", "Graphite", "Forest", "Rose"];

function applyTheme() {
  const mode = state.prefs.theme || "System";
  const named = pref("theme_name", "");
  if (named) {
    document.documentElement.dataset.theme = named.toLowerCase();
    syncWindowTheme();
    return;
  }
  const dark = mode === "Dark" || (mode === "System" && darkQuery.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  syncWindowTheme();
}

/** Windows and Linux draw the native title bar above our toolbar; ask it to
 *  follow the app theme. macOS keeps its own traffic-light appearance. */
function syncWindowTheme() {
  if (IS_MAC || PLATFORM === "ios" || PLATFORM === "android") return;
  try {
    const win = window.__TAURI__?.window?.getCurrentWindow?.();
    if (!win || typeof win.setTheme !== "function") return;
    const named = pref("theme_name", "");
    const mode = state.prefs.theme || "System";
    if (!named && mode === "System") {
      Promise.resolve(win.setTheme(null)).catch(() => {});
      return;
    }
    const darkThemes = new Set(["dark", "nord", "graphite", "forest"]);
    const theme = darkThemes.has(document.documentElement.dataset.theme) ? "dark" : "light";
    Promise.resolve(win.setTheme(theme)).catch(() => {});
  } catch {}
}

/** The modern interface is a taste; Settings › Themes keeps it, the CSS does the work. */
function applyGlass() {
  document.documentElement.dataset.glass = pref("glass", true) ? "on" : "off";
}

/** Light / Dark / System, or one of the named palettes. */
async function chooseTheme(name) {
  if (NAMED_THEMES.includes(name)) {
    await setUiPref("theme_name", name);
  } else {
    await setUiPref("theme_name", "");
    state.prefs.theme = name;
    await savePrefs();
  }
  applyTheme();
  renderSettings();
}
darkQuery.addEventListener("change", applyTheme);

async function applyPrefs() {
  applyTheme();
  applyGlass();
  applyPaneWidths();
  requestAnimationFrame(positionGrips);
  const [accent, soft] = ACCENTS[(state.prefs.accent || 0) % ACCENTS.length];
  document.documentElement.style.setProperty("--accent", accent);
  document.documentElement.style.setProperty("--accent-soft", soft);
  document.documentElement.style.setProperty("--editor-size", (state.prefs.editor_font_size || 15) + "px");
  document.body.classList.toggle("hide-counts", state.prefs.show_counts === false);
  document.body.classList.toggle("compact-list", !!state.prefs.compact_list);
  if (typeof Editor !== "undefined") Editor.applyPrefs();
}

async function savePrefs() {
  await invoke("save_prefs", { prefs: state.prefs });
}

$("open-settings").addEventListener("click", openSettings);
$("s-close").addEventListener("click", () => {
  $("settings-modal").hidden = true;
  settingsAtRoot = true;
  syncMobileTabs();
});

// ----- sync settings
//
// The Merge / Keep local / Keep server question pops up once: right after a
// new WebDAV server is saved and turns out to hold notes already. "Sync now"
// never brings it up again by itself; "Choose what to sync…" does.

/** "Decide later" was picked: sync waits until the person chooses. */
let syncHeld = false;
/** What the settings page showed when it opened, to spot a new server. */
let savedSync = { url: "", user: "" };

function syncForm() {
  return {
    url: $("s-url").value.trim(),
    user: $("s-user").value.trim(),
    password: $("s-pass").value,
    backend_on: $("s-url").value.trim() !== "",
  };
}

function syncStatus(text, kind = "") {
  const el = $("s-status");
  el.textContent = t(text);
  el.className = "hint sync-status" + (kind ? " " + kind : "");
}

function busy(on) {
  for (const id of ["s-connect", "s-sync", "s-save"]) $(id).disabled = on;
}

function reportSynced(report) {
  if (report.busy) {
    syncStatus("A sync is already running — try again in a moment.");
    return;
  }
  if (report.error) {
    syncStatus(report.error, "bad");
    setPill("error", report.error);
    return;
  }
  AutoSync.last = new Date();
  syncStatus(report.changed ? `Synced — ${report.changed} change(s).` : "Synced — already up to date.", "good");
  setPill("ok");
  if (report.again) notesChanged();
}

/** Run a sync; `ask` shows the question even for a folder that already syncs. */
// Undo and redo act on the note being edited.
function editHistory(command) {
  if (!currentNote()) return;
  const body = $("body");
  const selection = window.getSelection();
  const saved = selection && selection.rangeCount ? selection.getRangeAt(0).cloneRange() : null;
  body.focus();
  document.execCommand(command);
  if (saved) {
    selection.removeAllRanges();
    selection.addRange(saved);
  }
  scheduleSave();
}

// The heading menu: H1–H6 and Normal, with the one in use ticked.
function currentBlockTag() {
  const selection = window.getSelection();
  let node = selection && selection.anchorNode;
  while (node && node !== $("body")) {
    if (node.nodeType === 1 && /^H[1-6]$/i.test(node.tagName)) return node.tagName.toLowerCase();
    if (node.nodeType === 1 && node.matches(INLINE_HEADING)) return node.className;
    node = node.parentNode;
  }
  return "p";
}

// Apply a heading level (h1–h6) or "p" (Normal) from the heading menu.
// With nothing selected, or a whole line selected, the whole line changes.
// With only some words selected, those words take the heading's size and
// weight but keep their place in the line: they become an inline
// <span class="hN" style="…">. The inline style is what WebKit keeps through
// insertHTML (it drops a bare class), and it keeps the look in exports too.
const INLINE_HEADING = "span.h1, span.h2, span.h3, span.h4, span.h5, span.h6";
const INLINE_HEADING_STYLE = {
  h1: "font-size: 1.55em; font-weight: 700;",
  h2: "font-size: 1.25em; font-weight: 700;",
  h3: "font-size: 1.08em; font-weight: 700;",
  h4: "font-size: 1.05em; font-weight: 700;",
  h5: "font-size: 1em; font-weight: 700;",
  h6: "font-size: 0.95em; font-weight: 700;",
};

function formatSelectionAsBlock(tag) {
  const body = $("body");
  const selection = window.getSelection();
  const BLOCKS = "p, div, li, h1, h2, h3, h4, h5, h6, blockquote, pre";
  const elementOf = (node) => (node && node.nodeType === 1 ? node : node && node.parentElement);
  const blockOf = (node) => {
    const found = elementOf(node)?.closest(BLOCKS);
    return found && found !== body && body.contains(found) ? found : null;
  };
  const inlineOf = (node) => {
    const found = elementOf(node)?.closest(INLINE_HEADING);
    return found && body.contains(found) ? found : null;
  };
  const whole = () => document.execCommand("formatBlock", false, tag);

  // Characters of a line: where one is, and selecting a run of them.
  const offsetIn = (block, container, offset) => {
    const lead = document.createRange();
    lead.setStart(block, 0);
    lead.setEnd(container, offset);
    return lead.toString().length;
  };
  const spanOf = (block, from, to) => {
    const walker = document.createTreeWalker(block, NodeFilter.SHOW_TEXT);
    const run = document.createRange();
    let seen = 0;
    let started = false;
    let node;
    while ((node = walker.nextNode())) {
      if (!started && seen + node.length > from) {
        run.setStart(node, from - seen);
        started = true;
      }
      if (started && seen + node.length >= to) {
        run.setEnd(node, to - seen);
        return run;
      }
      seen += node.length;
    }
    return null;
  };
  const select = (range) => {
    selection.removeAllRanges();
    selection.addRange(range);
  };

  // Put `html` in place of characters [from, to) of `block`, then select
  // what came in and give it the class `level` (or none, for Normal).
  const replaceWords = (block, from, to, html, level) => {
    const words = spanOf(block, from, to);
    if (!words) return;
    select(words);
    document.execCommand("insertHTML", false, html);
    const after = spanOf(block, from, to);
    if (!after) return;
    if (level) {
      const added = elementOf(after.startContainer).closest("span[style]");
      if (added && block.contains(added)) {
        added.className = level;
        after.selectNodeContents(added);
      }
    }
    // WebKit pins the spaces next to the new words as &nbsp;, which would
    // stop the line wrapping there; they go back to ordinary spaces.
    for (const node of [after.startContainer, after.endContainer]) {
      const element = elementOf(node).closest("span[style]") || node;
      for (const side of [element.previousSibling, element.nextSibling]) {
        if (side && side.nodeType === 3 && side.data.includes(" ")) {
          side.data = side.data.replace(/^ /, " ").replace(/ $/, " ");
        }
      }
    }
    select(after);
  };
  const styled = (level, html) => `<span style="${INLINE_HEADING_STYLE[level]}">${html}</span>`;

  body.focus();
  if (!selection || !selection.rangeCount) return whole();
  const range = selection.getRangeAt(0);

  // Inside words that are already an inline heading: change or remove it.
  const current = inlineOf(range.commonAncestorContainer);
  if (current) {
    const block = blockOf(current);
    if (!block) return;
    const before = document.createRange();
    before.selectNode(current);
    const from = offsetIn(block, before.startContainer, before.startOffset);
    const to = from + current.textContent.length;
    const inner = current.innerHTML;
    if (tag === "p") {
      // Unwrapping by hand: insertHTML would carry the old style along.
      current.replaceWith(...current.childNodes);
      const words = spanOf(block, from, to);
      if (words) select(words);
    } else {
      replaceWords(block, from, to, styled(tag, inner), tag);
    }
    return;
  }

  if (selection.isCollapsed || tag === "p") return whole();
  const block = blockOf(range.startContainer);
  // Several lines selected, or text outside any block: format whole lines.
  if (!block || block !== blockOf(range.endContainer)) return whole();

  // Leave out the spaces at either end of the selection.
  const text = block.textContent;
  let start = offsetIn(block, range.startContainer, range.startOffset);
  let end = offsetIn(block, range.endContainer, range.endOffset);
  while (start < end && /\s/.test(text[start])) start++;
  while (end > start && /\s/.test(text[end - 1])) end--;
  if (start === end) return;
  if (!text.slice(0, start).trim() && !text.slice(end).trim()) return whole();

  const holder = document.createElement("div");
  holder.appendChild(spanOf(block, start, end).cloneContents());
  // Inline headings already in there give way to the new one.
  holder.querySelectorAll(INLINE_HEADING).forEach((old) => old.replaceWith(...old.childNodes));
  replaceWords(block, start, end, styled(tag, holder.innerHTML), tag);
}

$("heading-button").addEventListener("click", (event) => {
  const active = currentBlockTag();
  const button = event.currentTarget.getBoundingClientRect();
  const items = [];
  for (const level of [1, 2, 3, 4, 5, 6]) {
    items.push({
      label: `H${level}`,
      check: active === `h${level}`,
      run: () => {
        formatSelectionAsBlock(`h${level}`);
        $("body").focus();
        scheduleSave();
      },
    });
  }
  items.push({ separator: true });
  items.push({
    label: "Normal",
    check: active === "p",
    run: () => {
      formatSelectionAsBlock("p");
      $("body").focus();
      scheduleSave();
    },
  });
  Dialog.menu(button.left, button.bottom + 4, items, { anchor: button });
});

$("undo").addEventListener("click", () => editHistory("undo"));
$("redo").addEventListener("click", () => editHistory("redo"));

$("sync-now").addEventListener("click", async () => {
  const button = $("sync-now");
  if (button.classList.contains("syncing")) return;
  if (!AutoSync.active) {
    activeTab = "sync";
    settingsAtRoot = false;
    openSettings();
    return;
  }
  button.classList.add("syncing");
  try {
    await runSync({ ask: false, popup: true });
  } finally {
    button.classList.remove("syncing");
  }
});

async function runSync({ ask = false, popup = true } = {}) {
  syncStatus(ask ? "Looking at the folder…" : "Syncing…");
  setPill("busy");
  busy(true);
  try {
    const report = await invoke("sync_now", { resolution: null, ask });
    if (report.needs_decision) {
      setPill("ready");
      if (popup) {
        askFirstSync(report.needs_decision);
      } else {
        syncStatus("Sync is paused until you choose what to sync.", "warn");
        setPill("held");
      }
      return;
    }
    await refresh();
    reportSynced(report);
  } catch (error) {
    syncStatus(String(error), "bad");
    setPill("ready");
  } finally {
    busy(false);
  }
}

/** The one button: Connect while there is no folder, Disconnect once there is. */
function paintConnect() {
  const button = $("s-connect");
  const connected = Boolean(savedSync.url);
  button.textContent = connected ? t("Disconnect") : t("Connect");
  button.className = connected ? "ghost" : "primary";
  $("s-connected").textContent = connected ? t("Connected to {url}", { url: savedSync.url }) : "";
}

$("s-connect").addEventListener("click", async () => {
  if (savedSync.url) {
    // Disconnect: the folder and the password are forgotten here, not on the
    // server. Notes stay on this computer.
    busy(true);
    try {
      await invoke("save_sync", {
        form: { backend_on: false, url: "", user: "", password: "" },
      });
      savedSync = { url: "", user: "" };
      $("s-url").value = "";
      $("s-user").value = "";
      $("s-pass").value = "";
      syncStatus("Disconnected. Notes stay on this computer.", "good");
      setPill("ready");
      await loadSyncSetup();
      paintConnect();
    } catch (error) {
      syncStatus(String(error), "bad");
    } finally {
      busy(false);
    }
    return;
  }

  const form = syncForm();
  if (!form.url || !form.user) {
    syncStatus("Fill in the server address and username first.", "bad");
    return;
  }
  busy(true);
  try {
    syncStatus("Connecting…");
    await invoke("test_webdav", { form });
    await invoke("save_sync", { form });
    $("s-pass").value = "";
    savedSync = { url: form.url, user: form.user };
    paintConnect();
    syncStatus("Connected.", "good");
    busy(false);
    // Straight into a sync: if the folder already holds notes, the
    // Merge / Keep this computer / Keep the server window opens now.
    syncHeld = false;
    await runSync();
  } catch (error) {
    syncStatus(String(error), "bad");
  } finally {
    busy(false);
  }
});

$("s-save").addEventListener("click", async () => {
  const form = syncForm();
  const isNew = form.backend_on && (form.url !== savedSync.url || form.user !== savedSync.user || form.password);
  busy(true);
  try {
    if (isNew) {
      // Check before saving, so a typo doesn't leave a broken setup behind.
      syncStatus("Checking the server…");
      await invoke("test_webdav", { form });
    }
    await invoke("save_sync", { form });
    $("s-pass").value = "";
    savedSync = { url: form.url, user: form.user };
    await loadSyncSetup();
    paintConnect();
    if (!form.backend_on) {
      syncStatus("Sync is off. Notes stay on this computer.");
      setPill("ready");
      return;
    }
    syncStatus("Saved.", "good");
  } catch (error) {
    syncStatus(String(error), "bad");
  } finally {
    busy(false);
  }
});

$("s-sync").addEventListener("click", () => runSync({ popup: !syncHeld }));

async function saveSyncOptions() {
  const options = {
    sync_on_change: $("s-on-change").checked,
    auto_sync_minutes: Number($("s-every").value),
  };
  await invoke("save_sync_options", { options });
  AutoSync.options = options;
}
$("s-on-change").addEventListener("change", saveSyncOptions);
$("s-every").addEventListener("change", saveSyncOptions);

let decisionChoice = "merge";
let decisionInfo = null;

const DECISION_DETAIL = {
  merge:
    "Both sides are kept. Where the same note differs, the newer one keeps its name and the other is saved as a “conflict copy”. Nothing is deleted.",
  local:
    "Your notes replace the ones in the folder. Notes in the folder that aren't on this computer are deleted from the server.",
  remote:
    "The folder's notes replace the ones on this computer. Notes here that aren't in the folder are deleted from this computer.",
};

function paintDecision() {
  for (const card of document.querySelectorAll(".choice-card")) {
    card.classList.toggle("selected", card.dataset.choice === decisionChoice);
  }
  $("decision-detail").textContent = t(DECISION_DETAIL[decisionChoice]);
}

function askFirstSync(info) {
  decisionInfo = info;
  decisionChoice = "merge";

  const same = info.shared
    ? plural(info.shared, "and {n} of them shares a name.", "and {n} of them share a name.")
    : t("— none of them are the same note.");
  $("first-sync-text").textContent =
    t("The folder holds {folder} notes and this computer has {here}", {
      folder: info.folder_notes,
      here: info.here_notes,
    }) +
    " " + same + " " +
    t("You can change this later in Settings → Sync.");

  paintDecision();
  // A window of its own on top of Settings (or the notes, if Settings is shut).
  $("first-sync").hidden = false;
}

for (const card of document.querySelectorAll(".choice-card")) {
  card.addEventListener("click", () => {
    decisionChoice = card.dataset.choice;
    paintDecision();
  });
}

$("decision-later").addEventListener("click", () => {
  const paired = decisionInfo && decisionInfo.paired;
  $("first-sync").hidden = true;
  decisionInfo = null;
  // A folder that already syncs carries on as before; a new one waits.
  if (paired) {
    syncStatus("Nothing changed — syncing as before.");
  } else {
    syncHeld = true;
    syncStatus("Sync is paused until you choose what to sync.", "warn");
    setPill("held");
  }
});

$("decision-continue").addEventListener("click", async () => {
  $("first-sync").hidden = true;
  syncHeld = false;
  syncStatus("Syncing…");
  setPill("busy");
  busy(true);
  try {
    const report = await invoke("sync_now", { resolution: decisionChoice, ask: false });
    await refresh();
    reportSynced(report);
  } catch (error) {
    syncStatus(String(error), "bad");
    setPill("ready");
  } finally {
    busy(false);
  }
});

function setPill(kind, detail = "") {
  const pill = $("sync-pill");
  const labels = {
    off: "On this device",
    ready: "WebDAV",
    busy: "Syncing…",
    ok: "Synced",
    error: "Sync problem",
    held: "Choose what to sync",
  };
  pill.className = "pill " + kind;
  pill.querySelector("span").textContent = t(labels[kind] || labels.ready);
  pill.title =
    detail ||
    (kind === "ok" && AutoSync.last
      ? t("Last synced {time} — click to sync now", {
          time: AutoSync.last.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" }),
        })
      : t(
          kind === "off"
            ? "Notes are only on this computer. Set up WebDAV in Settings."
            : kind === "held"
            ? "Pick Merge, Keep local or Keep server to start syncing"
            : "Click to sync now"
        ));
}

// ------------------------------------------------------------ automatic sync
//
// Sync runs on launch, a few seconds after a note changes, every few minutes
// (to pick up changes from other devices), and when the window comes back to
// the front. It never pops up the Merge / Keep local / Keep server question:
// if that still needs an answer, sync waits and the pill says so.

const AutoSync = {
  options: { sync_on_change: true, auto_sync_minutes: 1 },
  active: false,
  running: false,
  last: null,
  lastTry: 0,
  editTimer: null,
};

async function loadSyncSetup() {
  try {
    const [, , active] = await rawInvoke("sync_status");
    AutoSync.active = active;
    AutoSync.options = await rawInvoke("sync_options");
  } catch {
    AutoSync.active = false;
  }
  paintConnect();
  if (!AutoSync.active) setPill("off");
  else if (syncHeld) setPill("held");
  else if (!AutoSync.running) setPill(AutoSync.last ? "ok" : "ready");
}

function notesChanged() {
  if (!AutoSync.active || !AutoSync.options.sync_on_change) return;
  clearTimeout(AutoSync.editTimer);
  AutoSync.editTimer = setTimeout(() => autoSync(), 5000);
}

async function autoSync() {
  if (!AutoSync.active || AutoSync.running || syncHeld) return;
  AutoSync.running = true;
  AutoSync.lastTry = Date.now();
  setPill("busy");
  try {
    await flush();
    const report = await rawInvoke("sync_now", { resolution: null, ask: false });
    if (report.busy) return;
    if (report.needs_decision) {
      syncHeld = true;
      setPill("held");
      return;
    }
    if (report.changed) await refresh();
    if (report.error) {
      setPill("error", report.error);
    } else {
      AutoSync.last = new Date();
      setPill("ok");
    }
    // Something was edited while it ran: go again so it gets uploaded.
    if (report.again) notesChanged();
  } catch (error) {
    setPill("error", String(error));
  } finally {
    AutoSync.running = false;
  }
}

// The timer checks twice a minute whether a sync is due.
setInterval(() => {
  const minutes = AutoSync.options.auto_sync_minutes;
  if (minutes > 0 && Date.now() - AutoSync.lastTry >= minutes * 60_000) autoSync();
}, 30_000);

// Back to the window after a while: catch up with other devices.
window.addEventListener("focus", () => {
  if (Date.now() - AutoSync.lastTry > 60_000) autoSync();
});

$("sync-pill").addEventListener("click", async () => {
  if (!AutoSync.active || syncHeld) {
    await openSettings();
    activeTab = "sync";
    renderSettings();
    return;
  }
  autoSync();
});

// ------------------------------------------------------------ history

$("info").addEventListener("click", async () => {
  const panel = $("info-panel");
  if (!panel.hidden) {
    closeInfoPanel();
    return;
  }
  await showInfo();
});

/** The inspector folds to zero width before it is hidden. */
function closeInfoPanel() {
  const panel = $("info-panel");
  if (panel.hidden || panel.classList.contains("is-closing")) return;
  panel.classList.add("is-closing");
  clearTimeout(infoCloseTimer);
  infoCloseTimer = setTimeout(() => {
    panel.hidden = true;
    panel.classList.remove("is-closing");
  }, 230);
}

async function showInfo() {
  const note = currentNote();
  if (!note) return;
  await flush();
  const details = await invoke("note_details", { id: note.id });
  const body = $("info-body");
  body.innerHTML = "";
  const add = (label, value) => {
    const row = document.createElement("div");
    row.className = "info-row";
    row.innerHTML = "<span></span><b></b>";
    row.querySelector("span").textContent = label;
    row.querySelector("b").textContent = value;
    body.appendChild(row);
  };
  add("Note id", details.id);
  add("File", details.path);
  add("Notebook", details.notebook);
  add("Created", details.created);
  add("Updated", details.updated);
  add("Words", details.words);
  add("Characters", details.characters);
  add("Paragraphs", details.paragraphs);
  const h = document.createElement("h5");
  h.textContent = t("Table of contents");
  body.appendChild(h);
  if (details.headings.length) {
    const list = document.createElement("ol");
    for (const heading of details.headings) {
      const item = document.createElement("li");
      item.textContent = heading;
      list.appendChild(item);
    }
    body.appendChild(list);
  } else {
    const empty = document.createElement("p");
    empty.className = "hint";
    empty.textContent = t("No headings yet");
    body.appendChild(empty);
  }
  const back = document.createElement("h5");
  back.textContent = t("Linked from");
  body.appendChild(back);
  const linking = state.notes.filter(
    (other) =>
      other.id !== note.id &&
      !other.trashed &&
      (state.decrypted.get(other.id) ?? other.body).includes(`ownnotes://note/${note.id}`)
  );
  if (linking.length) {
    const list = document.createElement("ol");
    for (const other of linking) {
      const item = document.createElement("li");
      const link = document.createElement("a");
      link.href = "#";
      link.textContent = other.title.trim() || t("Untitled");
      link.addEventListener("click", (event) => {
        event.preventDefault();
        selectNote(other.id);
      });
      item.appendChild(link);
      list.appendChild(item);
    }
    body.appendChild(list);
  } else {
    const none = document.createElement("p");
    none.className = "hint";
    none.textContent = t("No other note links here yet");
    body.appendChild(none);
  }

  const panel = $("info-panel");
  clearTimeout(infoCloseTimer);
  panel.classList.remove("is-closing");
  panel.hidden = false;
}

$("history").addEventListener("click", async () => {
  const note = currentNote();
  if (!note) return;
  const list = await invoke("versions", { id: note.id });
  const box = $("version-list");
  box.innerHTML = "";
  if (!list.length) {
    const empty = document.createElement("p");
    empty.className = "hint";
    empty.textContent = t("No older versions yet.");
    box.appendChild(empty);
  }
  for (const version of list) {
    const row = document.createElement("button");
    row.className = "version";
    row.textContent = version.label;
    row.addEventListener("click", async () => {
      await flush();
      await invoke("restore_version", { id: note.id, at: version.at });
      $("history-modal").hidden = true;
      await refresh();
      toast("Version restored");
    });
    box.appendChild(row);
  }
  $("history-modal").hidden = false;
  translateDom($("history-modal"));
});

$("h-close").addEventListener("click", () => ($("history-modal").hidden = true));

$("h-clear").addEventListener("click", async () => {
  const note = currentNote();
  if (!note) return;
  const yes = await Dialog.confirm({
    title: "Clear this note's history?",
    label: "Every older version of it goes. The note itself is untouched.",
  });
  if (!yes) return;
  const count = await invoke("clear_versions", { id: note.id });
  $("history-modal").hidden = true;
  toast(plural(count, "Removed {n} version", "Removed {n} versions"));
});

$("g-clear-versions").addEventListener("click", async () => {
  const yes = await Dialog.confirm({
    title: "Delete all version history?",
    label: "Every older version of every note goes. This can't be undone.",
  });
  if (!yes) return;
  try {
    await invoke("clear_all_versions");
    toast("Version history cleared");
  } catch (error) {
    toast(String(error));
  }
});

// ------------------------------------------------------------ focus + sticky

let focus = false;
$("focus").addEventListener("click", () => {
  focus = !focus;
  document.body.classList.toggle("focus", focus);
  $("focus").classList.toggle("on", focus);
});
$("sticky").addEventListener("click", async () => {
  // `currentTarget` is gone once the click handler yields, so keep the
  // element itself.
  const button = $("sticky");
  const on = !button.classList.contains("on");
  try {
    await invoke("always_on_top", { on });
    button.classList.toggle("on", on);
  } catch (error) {
    toast(String(error));
  }
});

// ------------------------------------------------------------ tags

function noteTags(note) {
  // Code and formulas are left out, so "#include" in a code block isn't a tag.
  const prose = note.body
    .replace(/<(pre|code|math)[\s>][\s\S]*?<\/\1>/gi, " ")
    .replace(/<[^>]+>/g, " ");
  const text = note.title + " " + prose;
  const found = text.match(/(^|\s)#([\p{L}\p{N}_-]{2,})/gu) || [];
  return [...new Set(found.map((tag) => tag.trim().slice(1).toLowerCase()))];
}

async function toggleQuick(id) {
  const list = quickList().filter((other) => other !== id);
  if (!quickList().includes(id)) list.unshift(id);
  await setUiPref("quick_access", list);
  renderQuick();
  toast(quickList().includes(id) ? "In Quick Access" : "Removed from Quick Access");
}

function quickList() {
  return pref("quick_access", []);
}

function renderQuick() {
  const pinned = quickList()
    .map((id) => state.notes.find((note) => note.id === id))
    .filter((note) => note && !note.trashed);
  const box = $("quick");
  box.innerHTML = "";
  $("quick-title").hidden = pinned.length === 0;

  const header = $("quick-title");
  header.ondragover = (event) => {
    event.preventDefault();
    header.classList.add("dropping");
  };
  header.ondragleave = () => header.classList.remove("dropping");
  header.ondrop = async (event) => {
    event.preventDefault();
    header.classList.remove("dropping");
    const id = Number(event.dataTransfer.getData("text/ownnotes-note"));
    if (id) await toggleQuick(id);
  };
  for (const note of pinned.slice(0, 6)) {
    box.appendChild(
      row({
        glyph: ICONS.pin,
        label: note.title || t("Untitled"),
        active: state.current === note.id,
        onClick: () => selectNote(note.id),
      })
    );
  }
}

const LOCALES = { en: "en-GB", ar: "ar", tr: "tr", de: "de-DE", pl: "pl-PL", fr: "fr-FR", es: "es-ES" };

/** "Today, 11:30" / "أمس، 9:30" — dates follow the interface language. */
function when(ms) {
  if (!ms) return "";
  const locale = LOCALES[current] || "en-GB";
  const date = new Date(ms);
  const now = new Date();
  const sameDay = date.toDateString() === now.toDateString();
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  const isYesterday = date.toDateString() === yesterday.toDateString();
  const time = new Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit" }).format(date);
  if (sameDay) return `${t("Today")}, ${time}`;
  if (isYesterday) return `${t("Yesterday")}, ${time}`;
  const day = new Intl.DateTimeFormat(locale, { day: "numeric", month: "short" }).format(date);
  return day;
}

function tagColours() {
  return pref("tag_colours", {});
}

function savedSearches() {
  return pref("saved_searches", []);
}

function renderSaved() {
  const box = $("saved");
  const list = savedSearches();
  box.innerHTML = "";
  $("saved-title").hidden = list.length === 0;
  list.forEach((saved, index) => {
    const line = row({
      glyph: '<svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="6.5"/><path d="m20 20-4.2-4.2"/></svg>',
      label: saved.name,
      active: state.savedIndex === index,
      onClick: () => applySaved(index),
    });
    line.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      Dialog.menu(event.clientX, event.clientY, [
        {
          label: "Delete saved search",
          danger: true,
          run: async () => {
            const next = savedSearches().filter((_, i) => i !== index);
            await setUiPref("saved_searches", next);
            renderSaved();
          },
        },
      ]);
    });
    box.appendChild(line);
  });
}

function applySaved(index) {
  const saved = savedSearches()[index];
  if (!saved) return;
  state.savedIndex = index;
  state.search = saved.query || "";
  state.titleOnly = !!saved.titleOnly;
  $("search").value = state.search;
  $("search-scope").classList.toggle("on", state.titleOnly);
  state.view = saved.view || { kind: "all" };
  render();
}

async function saveCurrentSearch() {
  const query = state.search.trim();
  if (!query) return toast("Type something to search for first");
  const name = await Dialog.ask({ title: "Save this search", label: "A name for it", value: query, ok: "Save" });
  if (!name) return;
  const list = savedSearches().slice();
  list.push({ name: name.trim(), query, titleOnly: !!state.titleOnly, view: state.view });
  await setUiPref("saved_searches", list);
  renderSaved();
  toast("Search saved");
}

$("save-search").addEventListener("click", saveCurrentSearch);

function renderTags() {
  const counts = new Map();
  for (const note of state.notes.filter((n) => !n.trashed)) {
    for (const tag of noteTags(note)) counts.set(tag, (counts.get(tag) || 0) + 1);
  }
  const box = $("tags");
  box.innerHTML = "";
  $("tags-title").hidden = counts.size === 0;
  const tagOrder = pref("tag_order", []);
  const entries = [...counts.entries()].sort((a, b) => {
    const ai = tagOrder.indexOf(a[0]);
    const bi = tagOrder.indexOf(b[0]);
    if (ai >= 0 && bi >= 0) return ai - bi;
    if (ai >= 0) return -1;
    if (bi >= 0) return 1;
    return b[1] - a[1];
  });
  const folded = new Set(pref("collapsed_tags", []));
  const hasChild = (tag) => [...counts.keys()].some((other) => other.startsWith(`${tag}/`));

  // A child disappears when any of its parents is folded.
  const hidden = (tag) => {
    const parts = tag.split("/");
    for (let i = 1; i < parts.length; i += 1) {
      if (folded.has(parts.slice(0, i).join("/"))) return true;
    }
    return false;
  };

  for (const [tag, count] of entries) {
    if (hidden(tag)) continue;
    const tint = tagColours()[tag];
    const line = row({
      glyph: tint
        ? `<span class="tag-dot" style="background:${tint}"></span>`
        : '<svg viewBox="0 0 24 24"><path d="M4 4h9l7 7-9 9-7-7z"/><circle cx="8.5" cy="8.5" r="1.3"/></svg>',
      label: "#" + tag,
      count,
      indent: tag.split("/").length - 1,
      active: state.view.kind === "tag" && state.view.tag === tag,
      onClick: () => setView({ kind: "tag", tag }),
      onDrop: (ids, dragging) => {
        if (dragging && dragging.startsWith("tag:")) {
          return reorderTags(dragging.slice(4), tag);
        }
        return dropNotes(ids, { kind: "tag", tag });
      },
    });
    if (hasChild(tag)) {
      const caret = document.createElement("span");
      caret.className = "caret";
      caret.textContent = folded.has(tag) ? "▸" : "▾";
      caret.addEventListener("click", async (event) => {
        event.stopPropagation();
        const next = new Set(pref("collapsed_tags", []));
        if (next.has(tag)) next.delete(tag);
        else next.add(tag);
        await setUiPref("collapsed_tags", [...next]);
        renderTags();
        animateNav("tags");
      });
      line.insertBefore(caret, line.firstChild);
    }
    line.draggable = true;
    line.addEventListener("dragstart", (event) => {
      event.dataTransfer.setData("application/x-ownnotes-tag", tag);
      event.dataTransfer.effectAllowed = "move";
    });
    line.addEventListener("contextmenu", (event) => {
      event.preventDefault();
      Dialog.menu(event.clientX, event.clientY, [
        {
          label: "Rename tag…",
          run: async () => {
            const name = await Dialog.ask({
              title: `Rename #${tag}`,
              label: "The new name, without the #.",
              value: tag,
              ok: "Rename",
            });
            if (!name || name.trim() === tag) return;
            try {
              const changed = await invoke("rename_tag", { from: tag, to: name.trim() });
              await refresh();
              toast(plural(changed, "Renamed in {n} note", "Renamed in {n} notes"));
            } catch (error) {
              toast(String(error));
            }
          },
        },
        {
          label: "Tag colour…",
          run: async () => {
            const chosen = await Dialog.pick({
              title: t("Colour for #{tag}", { tag }),
              placeholder: "Pick a colour",
              items: [
                { label: "No colour", value: "" },
                ...COLOURS.map((colour) => ({ label: colour, value: colour, colour })),
              ],
            });
            if (chosen === null) return;
            const all = { ...tagColours() };
            if (chosen) all[tag] = chosen;
            else delete all[tag];
            await setUiPref("tag_colours", all);
            renderTags();
          },
        },
        {
          label: "Delete tag",
          danger: true,
          run: async () => {
            const yes = await Dialog.confirm({
              title: `Delete #${tag}?`,
              label: "It is removed from every note. This can't be undone.",
            });
            if (!yes) return;
            const changed = await invoke("rename_tag", { from: tag, to: null });
            await refresh();
            toast(plural(changed, "Removed from {n} note", "Removed from {n} notes"));
          },
        },
      ]);
    });
    box.appendChild(line);
  }
}

// ------------------------------------------------------------ templates

let templates = [];

async function renderTemplatesRow() {
  templates = await invoke("templates");
  const box = $("templates-row");
  box.innerHTML = "";
  box.appendChild(
    row({
      glyph: '<svg viewBox="0 0 24 24"><rect x="4" y="3" width="16" height="18" rx="2"/><path d="M8 8h8"/><path d="M8 12h8"/><path d="M8 16h5"/></svg>',
      label: "Templates",
      count: templates.length,
      active: state.view.kind === "templates",
      onClick: () => setView({ kind: "templates" }),
    })
  );
}

async function useTemplate(name) {
  const template = templates.find((t) => t.name === name);
  if (!template) return;
  const notebook = state.notebooks[0]?.id || 1;
  const id = await invoke("create_note", { notebookId: notebook, space: state.space });
  await invoke("save_note", { id, title: template.name, body: await ownCopies(id, template.body) });
  await refresh();
  state.current = id;
  setView({ kind: "all" });
  state.current = id;
  render();
}

function renderTemplatesPane() {
  const box = $("cards");
  box.innerHTML = "";
  $("list-title").textContent = t("Templates");
  $("list-count").textContent = templates.length;

  const add = document.createElement("button");
  add.className = "card";
  add.innerHTML = `<span class="name">+ ${t("Save this note as a template")}</span>`;
  add.addEventListener("click", async () => {
    const note = currentNote();
    if (!note) return toast("Open a note first");
    await invoke("save_template", {
      name: note.title || "Template",
      body: note.body,
    });
    await renderTemplatesRow();
    renderTemplatesPane();
    toast("Template saved");
  });
  box.appendChild(add);

  for (const template of templates) {
    const card = document.createElement("button");
    card.className = "card";
    card.innerHTML = `<span class="name"></span><span class="meta">${t("Click to create a note")}</span>`;
    card.querySelector(".name").textContent = template.name;
    card.addEventListener("click", () => useTemplate(template.name));
    box.appendChild(card);
  }
}

// ------------------------------------------------------------ export + blocks

function openExport() {
  $("export-modal").hidden = false;
  translateDom($("export-modal"));
}

$("tour-later").addEventListener("click", () => {
  $("tour").hidden = true;
  setUiPref("onboarded", true);
});
$("tour-sync").addEventListener("click", () => {
  $("tour").hidden = true;
  setUiPref("onboarded", true);
  activeTab = "sync";
  settingsAtRoot = false;
  openSettings();
});

$("export-close").addEventListener("click", () => ($("export-modal").hidden = true));

for (const button of document.querySelectorAll(".export-go")) {
  button.addEventListener("click", async () => {
    const note = currentNote();
    if (!note) return toast("Open a note first");
    const format = button.dataset.format;
    await flush();
    try {
      if (format === "pdf") {
        const orientation = $("export-pdf-orient").value;
        document.documentElement.dataset.print = orientation;
        const path = await exportPng(null, true);
        if (path) toast(t("Saved to {path}", { path }));
        delete document.documentElement.dataset.print;
      } else if (format === "html") {
        const path = await invoke("export_note", { id: note.id, format: "html" });
        toast(t("Saved to {path}", { path }));
      } else if (format === "png") {
        const path = await exportPng(note);
        if (path) toast(t("Saved to {path}", { path }));
      } else if (format === "md") {
        const path = await invoke("export_note", {
          id: note.id,
          format: "md",
          extras: $("export-extras").checked,
        });
        toast(t("Saved to {path}", { path }));
      } else {
        const path = await invoke("export_note", { id: note.id, format: "txt" });
        toast(t("Saved to {path}", { path }));
      }
    } catch (error) {
      toast(String(error));
    }
  });
}

$("export").addEventListener("click", () => {
  if (!currentNote()) return toast("Open a note first");
  openExport();
});

function insertHTML(html) {
  document.execCommand("insertHTML", false, html);
  $("body").focus();
  scheduleSave();
}

window.addEventListener("keydown", (event) => {
  if (!modKey(event)) return;
  // Behind the lock screen no shortcut works (typing the passcode still does).
  if (locked) return;
  // Shift turns "s" into "S" in every browser; compare the lower-case key.
  const key = event.key.toLowerCase();
  if (key === "s" && !event.shiftKey) {
    event.preventDefault();
    flush().then(() => toast("Saved"));
    return;
  }
  if (event.key === "Backspace") {
    // ⌘⌫ in a text field deletes to the start of the line, as everywhere
    // else on a Mac; it only trashes the note from outside the text.
    const target = event.target;
    if (target.closest?.("input, textarea, select, [contenteditable='true'], .body")) return;
    const note = currentNote();
    if (!note) return;
    event.preventDefault();
    invoke("set_flags", { id: note.id, pinned: null, starred: null, trashed: !note.trashed })
      .then(() => refresh())
      .then(() => toast(note.trashed ? "Restored" : "Moved to Trash"));
    return;
  }
  if (key === "s" && event.shiftKey) {
    event.preventDefault();
    const note = currentNote();
    if (note) toggleQuick(note.id);
    return;
  }
  if (key === "k") {
    event.preventDefault();
    quickSwitch();
    return;
  }
  if (key === "j") {
    event.preventDefault();
    openSpaceMenu();
    return;
  }
  if (key === "p" && event.shiftKey) {
    event.preventDefault();
    const note = currentNote();
    if (note) toggleFlag("pinned");
    return;
  }
  if (key === "f" && !event.shiftKey) {
    event.preventDefault();
    openFind();
    return;
  }
  if (key === "p" && event.altKey) {
    event.preventDefault();
    exportPng()
      .then((path) => {
        if (path) toast(t("Picture saved to {path}", { path }));
      })
      .catch((error) => toast(String(error)));
    return;
  }
  if (key === "p") {
    event.preventDefault();
    $("print").click();
    return;
  }
  if (event.shiftKey && key === "f") {
    event.preventDefault();
    $("focus").click();
  }
  if (key === "e") {
    event.preventDefault();
    $("export").click();
  }
  if (key === "/") {
    event.preventDefault();
    const details = `<details open><summary>${t("Section")}</summary>${t("Write here…")}</details>`;
    insertHTML(details);
  }
});

// ------------------------------------------------------------ quick switch

async function quickSwitch() {
  const notes = state.notes
    .filter((note) => !note.trashed)
    .map((note) => ({
      label: note.title.trim() || t("Untitled"),
      sub: `${when(note.updated_ms) || note.updated} · ${state.notebooks.find((b) => b.id === note.notebook_id)?.name || ""}`,
      value: note.id,
    }));
  if (!notes.length) return toast("No notes yet");
  const id = await Dialog.pick({ title: "Jump to a note", items: notes, placeholder: "Type a title" });
  if (id == null) return;
  const note = state.notes.find((n) => n.id === id);
  if (!note) return;
  if (state.view.kind === "trash" || state.view.kind === "favorites" || state.view.kind === "todo") {
    setView({ kind: "all" });
  }
  selectNote(id);
  $("title").focus();
}

// ------------------------------------------------------------ find in note

const find = { bar: null, input: null, matches: [], index: -1, query: "" };

function buildFind() {
  const bar = document.createElement("div");
  bar.className = "find-bar";
  bar.hidden = true;
  bar.innerHTML = `
    <input class="find-input" placeholder="${t("Find in note")}" />
    <span class="find-count"></span>
    <button class="find-next" title="Next">↓</button>
    <button class="find-close" title="Close">×</button>`;
  $("editor-pane").appendChild(bar);
  translateDom(bar);
  find.bar = bar;
  find.input = bar.querySelector(".find-input");

  find.input.addEventListener("input", () => {
    find.query = find.input.value;
    collectMatches();
    find.index = -1;
    stepMatch(1);
  });
  find.input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") {
      event.preventDefault();
      stepMatch(event.shiftKey ? -1 : 1);
    } else if (event.key === "Escape") {
      event.preventDefault();
      closeFind();
    }
  });
  bar.querySelector(".find-next").addEventListener("click", () => stepMatch(1));
  bar.querySelector(".find-close").addEventListener("click", closeFind);
}

function collectMatches() {
  find.matches = [];
  const query = find.query.trim().toLowerCase();
  if (!query) {
    $("body").scrollTop = 0;
    find.bar.querySelector(".find-count").textContent = "";
    return;
  }
  const walker = document.createTreeWalker($("body"), NodeFilter.SHOW_TEXT);
  let node;
  while ((node = walker.nextNode())) {
    const haystack = node.data.toLowerCase();
    let from = 0;
    let at;
    while ((at = haystack.indexOf(query, from)) >= 0) {
      find.matches.push({ node, at, length: query.length });
      from = at + query.length;
    }
  }
  find.bar.querySelector(".find-count").textContent = find.matches.length
    ? plural(find.matches.length, "{n} match", "{n} matches")
    : t("No matches");
}

function stepMatch(direction) {
  if (!find.matches.length) return;
  find.index = (find.index + direction + find.matches.length) % find.matches.length;
  const hit = find.matches[find.index];
  const range = document.createRange();
  range.setStart(hit.node, hit.at);
  range.setEnd(hit.node, hit.at + hit.length);
  const selection = window.getSelection();
  selection.removeAllRanges();
  selection.addRange(range);
  const parent = hit.node.parentElement;
  if (parent) parent.scrollIntoView({ block: "center", behavior: "smooth" });
  find.bar.querySelector(".find-count").textContent =
    t("{n} of {total}", { n: find.index + 1, total: find.matches.length });
}

function openFind() {
  if (!find.bar) buildFind();
  find.bar.hidden = false;
  find.input.focus();
  find.input.select();
  const selected = window.getSelection().toString().trim();
  if (selected && selected.length < 60) {
    find.input.value = selected;
    find.input.dispatchEvent(new Event("input"));
  }
}

function closeFind() {
  if (!find.bar) return;
  find.bar.hidden = true;
  find.matches = [];
  find.index = -1;
  find.query = "";
  $("body").focus();
}

// ------------------------------------------------------------ note links

// Links between notes: ownnotes://note/<id>
$("body").addEventListener("click", (event) => {
  const link = event.target.closest("a");
  if (!link) return;
  const href = link.getAttribute("href") || "";
  if (!href.startsWith("ownnotes://note/")) return;
  event.preventDefault();
  const id = Number(href.split("/").pop());
  const note = state.notes.find((n) => n.id === id);
  if (!note) return toast("That note is gone");
  if (note.trashed) setView({ kind: "trash" });
  selectNote(id);
});

function insertNoteLink() {
  const books = new Map(state.notebooks.map((b) => [b.id, b.name]));
  Editor.keepingSelection(async (restore) => {
    const id = await Dialog.pick({
      title: "Link to a note",
      placeholder: "Find a note",
      items: state.notes
        .filter((n) => !n.trashed && n.id !== state.current)
        .map((n) => ({ label: n.title || t("Untitled"), sub: books.get(n.notebook_id) || "", value: n.id })),
    });
    restore();
    if (id === null) return;
    const target = state.notes.find((n) => n.id === id);
    const label = (target.title || t("Untitled")).replace(/&/g, "&amp;").replace(/</g, "&lt;");
    insertHTML(`<a href="ownnotes://note/${target.id}">${label}</a>&nbsp;`);
  });
}

// ------------------------------------------------------------ lock

let locked = false;

window.addEventListener("error", (event) => {
  invoke("log_ui_error", { message: String(event.message) + " @ " + String(event.filename) + ":" + event.lineno }).catch(() => {});
});
window.addEventListener("unhandledrejection", (event) => {
  invoke("log_ui_error", { message: "promise: " + String(event.reason) }).catch(() => {});
});

let booted = false;

function windowNote() {
  const wanted = new URLSearchParams(location.search).get("note");
  const id = wanted ? Number(wanted) : null;
  const note = id ? state.notes.find((n) => n.id === id) : null;
  if (!note) return;
  state.current = note.id;
  if (note.space && state.spaces.includes(note.space)) state.space = note.space;
  state.savedIndex = null;
  render();
}

async function boot() {
  await loadUiPrefs();
  applyGlass();
  readPaneWidths();
  applyLanguageState(pref("lang", "en"));

  // A passcode set means the notes stay closed until it is given. Nothing is
  // loaded, rendered or synced before that.
  // First launch: ask about sync before anything else.
  const [, , connected] = await rawInvoke("sync_status");
  // Onboarding is a first-launch event, even for people who never connect.
  showWelcome = !connected && !pref("onboarded", false);

  const hasLock = await invoke("lock_state");
  if (hasLock) {
    locked = true;
    document.body.classList.add("gate");
    $("lock-screen").hidden = false;
    $("lock-input").value = "";
    $("lock-error").textContent = "";
    setTimeout(() => $("lock-input").focus(), 120);
    return;
  }
  await openApp();
}

/** Everything after the gate: load, render, connect. */
async function openApp() {
  $("lock-screen").hidden = true;
  await refresh();
  windowNote();
  applySpelling();
  applyFont();
  applyLanguageState(pref("lang", "en"));
  applyPrefs();
  paintSort();
  await loadSyncSetup();
  if (showWelcome) {
    // Asked once per launch while no folder is connected.
    setTimeout(() => {
      $("tour").hidden = false;
      translateDom($("tour"));
    }, 700);
  }
  if (!booted) {
    booted = true;
    setTimeout(autoSync, 1500);
  }
}


async function tryUnlock() {
  const ok = await invoke("lock_check", { passcode: $("lock-input").value });
  if (!ok) {
    $("lock-error").textContent = t("That passcode doesn't match.");
    $("lock-input").select();
    return;
  }
  locked = false;
  $("lock-error").textContent = "";
  $("lock-input").value = "";
  state.unlocked.clear();
  state.unlockedBooks.clear();
  state.unlockedSpaces.clear();
  await openApp();
}

$("lock-open").addEventListener("click", tryUnlock);
$("lock-input").addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    tryUnlock();
  }
});

$("lock").addEventListener("click", async () => {
  const has = await rawInvoke("lock_state").catch(() => false);
  let current = null;
  if (has) {
    current = await askPasscode("Current passcode");
    if (current === null) return;
  }
  const code = await Dialog.ask({
    title: has ? "New passcode" : "App passcode",
    label: "OwnNotes will ask for it when it opens. Leave it empty to remove the lock.",
    password: true,
    ok: "Save",
  });
  if (code === null) return;
  try {
    await invoke("lock_set", { passcode: code || null, current });
    toast(code ? "Passcode set" : "Lock removed");
  } catch (error) {
    toast(String(error));
  }
});


// ------------------------------------------------------------ pane widths

/** The sidebar, note list and info panel can be dragged wider or narrower. */
const PANE_DEFAULTS = { sidebar: 220, list: 290, info: 250 };
const PANE_LIMITS = { sidebar: [150, 420], list: [200, 560], info: [180, 460] };
const paneWidths = { ...PANE_DEFAULTS };

/** Read the widths back from app.json; called once, before the first paint. */
function readPaneWidths() {
  const saved = pref("pane_widths", {}) || {};
  for (const name of Object.keys(PANE_DEFAULTS)) {
    const value = Number(saved[name]);
    if (Number.isFinite(value)) paneWidths[name] = clampPane(name, value);
  }
  document.body.classList.add("no-pane-transition");
  applyPaneWidths();
  requestAnimationFrame(() => document.body.classList.remove("no-pane-transition"));
}

function clampPane(name, width) {
  const [min, max] = PANE_LIMITS[name];
  return Math.round(Math.max(min, Math.min(max, width)));
}

function applyPaneWidths() {
  const style = document.documentElement.style;
  style.setProperty("--pane-sidebar", `${paneWidths.sidebar}px`);
  style.setProperty("--pane-list", `${paneWidths.list}px`);
  style.setProperty("--pane-info", `${paneWidths.info}px`);
  for (const grip of document.querySelectorAll(".pane-grip")) {
    grip.setAttribute("aria-valuenow", String(paneWidths[grip.dataset.pane]));
    grip.setAttribute("aria-valuemin", String(PANE_LIMITS[grip.dataset.pane][0]));
    grip.setAttribute("aria-valuemax", String(PANE_LIMITS[grip.dataset.pane][1]));
  }
}

/** Keep each grip over the gap between the panes it resizes. */
function positionGrips() {
  if (document.documentElement.dataset.glass !== "on") return;
  if (isMobile()) {
    for (const grip of document.querySelectorAll(".pane-grip")) grip.style.display = "none";
    return;
  }
  const panes = document.querySelector(".panes");
  const sidebar = $("sidebar");
  const list = $("list-pane");
  const editor = $("editor-pane");
  const info = $("info-panel");
  if (!panes || !sidebar || !list || !editor || !info) return;

  const base = panes.getBoundingClientRect();
  const s = sidebar.getBoundingClientRect();
  const l = list.getBoundingClientRect();
  const e = editor.getBoundingClientRect();
  const i = info.getBoundingClientRect();

  const place = (name, x, visible) => {
    const grip = document.querySelector(`.pane-grip[data-pane="${name}"]`);
    if (!grip) return;
    grip.style.display = visible ? "block" : "none";
    if (visible) grip.style.left = `${Math.round(x - base.left)}px`;
  };

  const sidebarShown = !sidebar.classList.contains("is-hidden") && s.width > 2;
  const listShown = !document.body.classList.contains("focus") && l.width > 2;
  place("sidebar", (s.right + l.left) / 2, sidebarShown);
  place("list", (l.right + e.left) / 2, listShown);
  place("info", (e.right + i.left) / 2, !info.hidden && !info.classList.contains("is-closing") && i.width > 2);
}

function savePaneWidths() {
  setUiPref("pane_widths", { ...paneWidths });
}

/** Grow or shrink one pane, without squeezing the editor below 340px. */
function resizePane(name, startWidth, delta, editorWidth) {
  const [min] = PANE_LIMITS[name];
  const room = Math.max(0, editorWidth - 340);
  const max = Math.min(PANE_LIMITS[name][1], startWidth + room);
  paneWidths[name] = Math.round(Math.max(min, Math.min(max, startWidth + delta)));
  applyPaneWidths();
  positionGrips();
}

function bindPaneGrips() {
  for (const grip of document.querySelectorAll(".pane-grip")) {
    const name = grip.dataset.pane;
    grip.addEventListener("pointerdown", (event) => {
      if (event.button !== 0) return;
      event.preventDefault();
      const startX = event.clientX;
      const startWidth = paneWidths[name];
      const editorWidth = $("editor-pane").getBoundingClientRect().width;
      const rtl = document.documentElement.dir === "rtl";
      const move = (e) => {
        const delta = (rtl ? -1 : 1) * (name === "info" ? -1 : 1) * (e.clientX - startX);
        resizePane(name, startWidth, delta, editorWidth);
      };
      const end = () => {
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", end);
        window.removeEventListener("pointercancel", end);
        document.body.classList.remove("resizing");
        grip.classList.remove("dragging");
        savePaneWidths();
      };
      document.body.classList.add("resizing");
      grip.classList.add("dragging");
      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", end);
      window.addEventListener("pointercancel", end);
    });

    grip.addEventListener("keydown", (event) => {
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
      event.preventDefault();
      const direction = event.key === "ArrowRight" ? 1 : -1;
      const rtl = document.documentElement.dir === "rtl";
      const step = 16 * direction * (rtl ? -1 : 1) * (name === "info" ? -1 : 1);
      const editorWidth = $("editor-pane").getBoundingClientRect().width;
      resizePane(name, paneWidths[name], step, editorWidth);
      savePaneWidths();
    });

    grip.addEventListener("dblclick", () => {
      paneWidths[name] = PANE_DEFAULTS[name];
      applyPaneWidths();
      positionGrips();
      savePaneWidths();
    });
  }
}

bindPaneGrips();
const paneResizeObserver = new ResizeObserver(() => positionGrips());
for (const el of [document.querySelector(".panes"), $("sidebar"), $("list-pane"), $("editor-pane"), $("info-panel")]) {
  if (el) paneResizeObserver.observe(el);
}
window.addEventListener("resize", positionGrips);
// A language switch or the glass toggle moves the panes without resizing
// them, so listen for those too.
new MutationObserver(positionGrips).observe(document.documentElement, {
  attributes: true,
  attributeFilter: ["dir", "data-glass"],
});
new MutationObserver(positionGrips).observe(document.body, {
  attributes: true,
  attributeFilter: ["class", "dir"],
});
requestAnimationFrame(positionGrips);


// ------------------------------------------------------- closing overlays

// The app closes modals by setting hidden. Catch that, let the overlay fade
// first, and only then really hide it. Re-opening during the fade cancels it.
const closingOverlays = new Map();
const settledOverlays = new WeakSet();
const reopeningOverlays = new WeakSet();

function hideModalAfterFade(modal) {
  if (closingOverlays.has(modal)) return;
  if (settledOverlays.has(modal)) {
    settledOverlays.delete(modal);
    return;
  }
  reopeningOverlays.add(modal);
  modal.hidden = false;
  modal.classList.add("is-closing");
  const timer = setTimeout(() => {
    closingOverlays.delete(modal);
    modal.classList.remove("is-closing");
    settledOverlays.add(modal);
    modal.hidden = true;
  }, 190);
  closingOverlays.set(modal, timer);
}

const modalHider = new MutationObserver((records) => {
  for (const record of records) {
    const el = record.target;
    if (!(el instanceof HTMLElement) || !el.classList.contains("modal")) continue;
    if (el.hidden) {
      if (settledOverlays.has(el)) {
        settledOverlays.delete(el);
        continue;
      }
      // Setting hidden on an overlay that was never shown must not flash it.
      if (record.oldValue !== null) continue;
      hideModalAfterFade(el);
    } else {
      if (reopeningOverlays.has(el)) {
        reopeningOverlays.delete(el);
        continue;
      }
      const timer = closingOverlays.get(el);
      if (timer) {
        clearTimeout(timer);
        closingOverlays.delete(el);
        el.classList.remove("is-closing");
      }
    }
  }
});
modalHider.observe(document.body, { attributes: true, attributeOldValue: true, attributeFilter: ["hidden"], subtree: true });

applyPlatformLabels();
applyMobileMode();

// ------------------------------------------------------------ start

window.addEventListener("keydown", (event) => {
  if (!modKey(event)) return;
  if (locked) return;
  if (event.key.toLowerCase() === "n") {
    event.preventDefault();
    $("new-note").click();
  }
});

window.addEventListener("beforeunload", () => {
  clearTimeout(state.saveTimer);
  const note = currentNote();
  // The same checks as saveNow: only the note whose text is in the editor,
  // never a locked one, and only when something changed.
  if (!note || state.shownId !== note.id || isHidden(note)) return;
  const title = $("title").value;
  const body = storedBody();
  if (title === note.title && body === note.body) return;
  invoke("save_note", { id: note.id, title, body });
});

boot();
