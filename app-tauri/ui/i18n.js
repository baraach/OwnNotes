// The interface in more than one language.
//
// English is the source: every string here is the English one, and the
// dictionaries translate from it. Anything not translated falls back to
// English, so a missing line is never a blank space. Arabic switches the
// whole interface to right-to-left.

// One dictionary per language, each in its own file (i18n.ar.js,
// i18n.de.js, …), loaded after this one. Adding a language: a new
// i18n.xx.js that sets STRINGS.xx, a line in LANGUAGES, and a <script>.
const STRINGS = {};

const LANGUAGES = [
  { code: "en", name: "English", rtl: false },
  { code: "ar", name: "العربية", rtl: true },
  { code: "tr", name: "Türkçe", rtl: false },
  { code: "de", name: "Deutsch", rtl: false },
  { code: "pl", name: "Polski", rtl: false },
  { code: "fr", name: "Français", rtl: false },
  { code: "es", name: "Español", rtl: false },
];

let current = "en";

/** Translate one string; unknown text comes back as it went in.
 *  `{name}` in the text is filled from `vars`, after translating, so the
 *  key stays the English sentence: t("Moved to {name}", { name }). */
/** Typographic quotes and dashes are the same word as the plain ones, so a
 *  label written with “curly quotes” still finds its translation. */
const QUOTES = /[\u2018\u2019\u201a\u201b\u2032]/g;
const DQUOTES = /[\u201c\u201d\u201e\u201f\u2033]/g;
const DASHES = /[\u2010\u2011\u2012\u2013\u2014]/g;
function plain(text) {
  return text.replace(QUOTES, "'").replace(DQUOTES, '"').replace(DASHES, "-");
}

function t(text, vars) {
  let out = text;
  if (text && current !== "en") {
    const table = STRINGS[current];
    // Look the sentence up as written, then once more with plain punctuation.
    if (table && table[text] === undefined) {
      const wanted = plain(text);
      for (const [key, value] of Object.entries(table)) {
        if (plain(key) === wanted) {
          table[text] = value;
          break;
        }
      }
    }
    const found = table && table[text];
    if (typeof found === "string") out = found;
  }
  if (vars && typeof out === "string") {
    out = out.replace(/\{(\w+)\}/g, (all, name) => (name in vars ? String(vars[name]) : all));
  }
  return out;
}

/** A count with the right plural form: plural(3, "{n} note", "{n} notes").
 *  The English plural is the key. A language with more than two forms gives
 *  an object keyed by Intl.PluralRules categories (one, two, few, many,
 *  other, zero) instead of a string; `other` is the fallback. */
function plural(count, one, many, vars = {}) {
  const all = { n: count, ...vars };
  if (current === "en") return t(count === 1 ? one : many, all);
  const table = STRINGS[current] || {};
  const found = table[many];
  let form;
  if (found && typeof found === "object") {
    const category = new Intl.PluralRules(current).select(count);
    // A form set without "one" leaves 1 to the singular key's translation.
    form =
      found[category] ??
      (count === 1 && typeof table[one] === "string" ? table[one] : found.other ?? many);
  } else if (count === 1 && typeof table[one] === "string") {
    form = table[one];
  } else {
    form = typeof found === "string" ? found : count === 1 ? one : many;
  }
  return form.replace(/\{(\w+)\}/g, (whole, name) => (name in all ? String(all[name]) : whole));
}

function language(code) {
  return LANGUAGES.find((entry) => entry.code === code) || LANGUAGES[0];
}

function isRtl() {
  return language(current).rtl;
}

/** Translate the fixed parts of the page, remembering the English original. */
function translateDom(root = document.body) {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const nodes = [];
  while (walker.nextNode()) nodes.push(walker.currentNode);
  for (const node of nodes) {
    const text = node.nodeValue;
    if (!text || !text.trim()) continue;
    if (node.parentElement && node.parentElement.closest("[data-no-i18n]")) continue;
    if (node.__en === undefined) node.__en = text;
    const key = node.__en.trim();
    const translated = t(key);
    node.nodeValue = translated === key ? node.__en : node.__en.replace(key, translated);
  }

  for (const el of root.querySelectorAll("[placeholder], [title]")) {
    if (el.closest("[data-no-i18n]")) continue;
    if (el.hasAttribute("placeholder")) {
      if (el.dataset.enPlaceholder === undefined) el.dataset.enPlaceholder = el.placeholder;
      el.placeholder = t(el.dataset.enPlaceholder);
    }
    if (el.hasAttribute("title")) {
      if (el.dataset.enTitle === undefined) el.dataset.enTitle = el.title;
      el.title = t(el.dataset.enTitle);
    }
  }
}

/** Apply a language: direction, translation, and a fresh render. */
function applyLanguageState(code) {
  current = STRINGS[code] ? code : "en";
  const info = language(current);
  const root = document.documentElement;
  root.lang = current;
  root.dir = info.rtl ? "rtl" : "ltr";
  root.dataset.rtl = info.rtl ? "true" : "false";
  document.body.dir = root.dir;
  document.body.classList.toggle("rtl", info.rtl);
  root.classList.toggle("rtl", info.rtl);
  translateDom();
}
