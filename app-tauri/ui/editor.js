// Editor features on top of the basic rich-text box in app.js: the "/" menu,
// code blocks, tables, highlight colours, fonts, attachments (images, PDFs,
// other files), embedded videos and LaTeX maths. Loaded after app.js and uses
// its helpers (state, invoke, insertHTML, scheduleSave, toast, $).

const Editor = (() => {
  const body = $("body");

  // ------------------------------------------------------------ helpers

  const escapeHTML = (text) =>
    String(text)
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;")
      .replace(/"/g, "&quot;");

  /** Run something that opens a prompt, then put the caret back where it was. */
  async function keepingSelection(work) {
    const selection = window.getSelection();
    const saved = selection.rangeCount && body.contains(selection.anchorNode)
      ? selection.getRangeAt(0).cloneRange()
      : null;
    const restore = () => {
      body.focus();
      if (saved) {
        selection.removeAllRanges();
        selection.addRange(saved);
      }
    };
    return work(restore);
  }

  function caretElement() {
    const selection = window.getSelection();
    if (!selection.rangeCount) return null;
    let node = selection.anchorNode;
    if (!node || !body.contains(node)) return null;
    return node.nodeType === 1 ? node : node.parentElement;
  }

  function formatSize(bytes) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
    return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  }

  // ------------------------------------------------------------ safety
  // Notes can arrive from the server, so anything that could run code is
  // removed before a note is shown: scripts, on… handlers, javascript: links
  // and frames other than the video sites and PDF previews we create.

  const STORED = "attachment://localhost/";
  const IS_WINDOWS = navigator.userAgent.includes("Windows");
  const SHOWN = IS_WINDOWS ? "http://attachment.localhost/" : STORED;

  const VIDEO_FRAMES = [
    /^https:\/\/www\.youtube-nocookie\.com\/embed\/[\w-]+/,
    /^https:\/\/www\.youtube\.com\/embed\/[\w-]+/,
    /^https:\/\/player\.vimeo\.com\/video\/\d+/,
    /^https:\/\/www\.dailymotion\.com\/embed\/video\/[\w]+/,
  ];

  function isAttachmentUrl(url) {
    return url.startsWith(STORED) || url.startsWith("http://attachment.localhost/");
  }

  function clean(html) {
    const doc = new DOMParser().parseFromString(`<body>${html}</body>`, "text/html");
    doc.body
      .querySelectorAll("script, style, link, meta, base, object, embed, form, frame, frameset, applet, noscript, template")
      .forEach((node) => node.remove());
    // SVG animation can rewrite a link into javascript: after cleaning.
    const ANIMATION = new Set(["animate", "set", "animatemotion", "animatetransform", "discard"]);
    for (const el of [...doc.body.querySelectorAll("*")]) {
      if (ANIMATION.has(el.localName.toLowerCase())) el.remove();
    }
    for (const el of [...doc.body.querySelectorAll("*")]) {
      for (const attr of [...el.attributes]) {
        const name = attr.name.toLowerCase();
        // Browsers ignore control characters and spaces inside a URL's
        // scheme ("\x01java script:"), so they go before checking it.
        const value = attr.value.toLowerCase().replace(/[\u0000-\u0020]+/g, "");
        const isUrl = ["href", "src", "xlink:href", "action", "formaction", "poster", "background"].includes(name);
        if (
          name.startsWith("on") ||
          name === "srcdoc" ||
          // An id or name inside a note could take over one of the app's
          // own elements (say "lock-screen"), so notes don't keep them.
          name === "id" ||
          name === "name" ||
          name === "form" ||
          (isUrl && (value.startsWith("javascript:") || value.startsWith("vbscript:"))) ||
          (isUrl && value.startsWith("data:") && !value.startsWith("data:image/"))
        ) {
          el.removeAttribute(attr.name);
        }
      }
      if (el.tagName === "IFRAME") {
        const src = el.getAttribute("src") || "";
        const video = VIDEO_FRAMES.some((pattern) => pattern.test(src));
        const pdf = isAttachmentUrl(src) && /\.pdf$/i.test(decodeURIComponent(src));
        if (!video && !pdf) el.remove();
      }
    }
    return doc.body.innerHTML;
  }

  /** Stored HTML → what the editor shows. */
  function load(html) {
    const safe = clean(html || "");
    return IS_WINDOWS ? safe.split(STORED).join(SHOWN) : safe;
  }

  /** What the editor shows → stored HTML. */
  function store(html) {
    return IS_WINDOWS ? html.split(SHOWN).join(STORED) : html;
  }

  // ------------------------------------------------------------ blocks

  function block(tag) {
    formatSelectionAsBlock(tag);
    body.focus();
    scheduleSave();
  }

  /** Insert HTML whose first element carries `data-new`, then put the caret
   *  inside that element (at `selector` within it, if given). */
  function insertAndEnter(html, selector) {
    insertHTML(html);
    const fresh = body.querySelector("[data-new]");
    if (!fresh) return;
    fresh.removeAttribute("data-new");
    placeCaret(selector ? fresh.querySelector(selector) || fresh : fresh);
    scheduleSave();
  }

  function insertCodeBlock() {
    const text = window.getSelection().toString();
    const content = text ? escapeHTML(text) : "<br>";
    insertAndEnter(`<pre class="code" data-new><code>${content}</code></pre><p><br></p>`, "code");
  }

  function inlineCode() {
    const text = window.getSelection().toString();
    if (!text) {
      insertHTML("<code>​</code>");
      return;
    }
    insertHTML(`<code>${escapeHTML(text)}</code>`);
  }

  function insertDivider() {
    insertHTML("<hr><p><br></p>");
  }

  function insertCollapsible() {
    insertHTML(`<details open><summary>${escapeHTML(t("Section"))}</summary><p>${escapeHTML(t("Write here…"))}</p></details><p><br></p>`);
  }

  function insertDate() {
    const text = new Date().toLocaleDateString(undefined, {
      weekday: "long",
      year: "numeric",
      month: "long",
      day: "numeric",
    });
    insertHTML(escapeHTML(text) + "&nbsp;");
  }

  // ------------------------------------------------------------ checklists

  // A ticked box only changes the "checked" property; the saved HTML holds
  // the attribute, so it is copied over or the tick would be lost.
  body.addEventListener("click", (event) => {
    const box = event.target;
    if (!(box instanceof HTMLInputElement) || box.type !== "checkbox") return;
    if (box.checked) box.setAttribute("checked", "");
    else box.removeAttribute("checked");
    box.closest("li")?.classList.toggle("done", box.checked);
    scheduleSave();
  });

  // ------------------------------------------------------------ code blocks

  body.addEventListener("keydown", (event) => {
    const el = caretElement();
    const pre = el && el.closest("pre");
    if (!pre || slash.open) return;
    if (event.key === "Tab") {
      event.preventDefault();
      document.execCommand("insertText", false, "  ");
    } else if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      // Leave the code block.
      event.preventDefault();
      let next = pre.nextElementSibling;
      if (!next) {
        next = document.createElement("p");
        next.innerHTML = "<br>";
        pre.after(next);
      }
      const range = document.createRange();
      range.setStart(next, 0);
      range.collapse(true);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    } else if (event.key === "Enter" && !event.shiftKey) {
      // A new line inside the block, not a new paragraph.
      event.preventDefault();
      const selection = window.getSelection();
      const range = selection.getRangeAt(0);
      range.deleteContents();
      const atEnd = (() => {
        const probe = range.cloneRange();
        probe.selectNodeContents(pre);
        probe.setStart(range.endContainer, range.endOffset);
        return probe.toString().length === 0;
      })();
      // At the very end a single newline isn't drawn, so add two.
      const text = document.createTextNode(atEnd ? "\n\n" : "\n");
      range.insertNode(text);
      range.setStart(text, 1);
      range.collapse(true);
      selection.removeAllRanges();
      selection.addRange(range);
      scheduleSave();
    }
  });

  // ------------------------------------------------------------ tables

  function insertTable(rows = 3, cols = 3) {
    let html = '<table class="note-table" data-new><tbody>';
    for (let r = 0; r < rows; r++) {
      html += "<tr>" + "<td><br></td>".repeat(cols) + "</tr>";
    }
    html += "</tbody></table><p><br></p>";
    insertAndEnter(html, "td");
  }

  function currentCell() {
    const el = caretElement();
    const cell = el && el.closest("td, th");
    return cell && body.contains(cell) ? cell : null;
  }

  function placeCaret(node, atEnd = false) {
    const range = document.createRange();
    range.selectNodeContents(node);
    range.collapse(!atEnd);
    const selection = window.getSelection();
    selection.removeAllRanges();
    selection.addRange(range);
  }

  function emptyCell(tag = "td") {
    const cell = document.createElement(tag);
    cell.innerHTML = "<br>";
    return cell;
  }

  const tableActions = {
    rowAbove(cell) {
      const row = cell.parentElement;
      const fresh = document.createElement("tr");
      for (let i = 0; i < row.children.length; i++) fresh.appendChild(emptyCell());
      row.before(fresh);
      placeCaret(fresh.children[cell.cellIndex]);
    },
    rowBelow(cell) {
      const row = cell.parentElement;
      const fresh = document.createElement("tr");
      for (let i = 0; i < row.children.length; i++) fresh.appendChild(emptyCell());
      row.after(fresh);
      placeCaret(fresh.children[cell.cellIndex]);
    },
    columnLeft(cell) {
      const index = cell.cellIndex;
      for (const row of cell.closest("table").rows) row.cells[index]?.before(emptyCell(row.cells[index]?.tagName.toLowerCase()));
      placeCaret(cell.parentElement.cells[index]);
    },
    columnRight(cell) {
      const index = cell.cellIndex;
      for (const row of cell.closest("table").rows) row.cells[index]?.after(emptyCell(row.cells[index]?.tagName.toLowerCase()));
      placeCaret(cell.parentElement.cells[index + 1]);
    },
    deleteRow(cell) {
      const table = cell.closest("table");
      const row = cell.parentElement;
      if (table.rows.length <= 1) return tableActions.deleteTable(cell);
      const target = row.nextElementSibling || row.previousElementSibling;
      row.remove();
      placeCaret(target.cells[Math.min(cell.cellIndex, target.cells.length - 1)]);
    },
    deleteColumn(cell) {
      const table = cell.closest("table");
      const index = cell.cellIndex;
      if (cell.parentElement.cells.length <= 1) return tableActions.deleteTable(cell);
      const row = cell.parentElement;
      for (const r of table.rows) r.cells[index]?.remove();
      placeCaret(row.cells[Math.max(0, index - 1)]);
    },
    headerRow(cell) {
      const first = cell.closest("table").rows[0];
      const toTag = first.cells[0].tagName === "TH" ? "td" : "th";
      for (const old of [...first.cells]) {
        const fresh = document.createElement(toTag);
        fresh.innerHTML = old.innerHTML;
        old.replaceWith(fresh);
      }
    },
    deleteTable(cell) {
      const table = cell.closest("table");
      const after = table.nextElementSibling;
      table.remove();
      if (after) placeCaret(after);
    },
  };

  const tableBar = document.createElement("div");
  tableBar.className = "table-tools";
  tableBar.hidden = true;
  tableBar.innerHTML = `
    <button data-act="rowAbove" title="Add a row above">↥ Row</button>
    <button data-act="rowBelow" title="Add a row below">↧ Row</button>
    <button data-act="columnLeft" title="Add a column to the left">↤ Col</button>
    <button data-act="columnRight" title="Add a column to the right">↦ Col</button>
    <button data-act="headerRow" title="Make the first row a header">Header</button>
    <span class="sep"></span>
    <button data-act="deleteRow" title="Delete this row">− Row</button>
    <button data-act="deleteColumn" title="Delete this column">− Col</button>
    <button data-act="deleteTable" class="danger" title="Delete the table">Delete table</button>`;
  document.body.appendChild(tableBar);
  translateDom(tableBar);
  let tableCell = null;
  tableBar.addEventListener("mousedown", (event) => event.preventDefault());
  tableBar.addEventListener("click", (event) => {
    const act = event.target.closest("button")?.dataset.act;
    if (!act || !tableCell) return;
    tableActions[act](tableCell);
    scheduleSave();
    updateTableBar();
  });

  function updateTableBar() {
    const cell = currentCell();
    tableCell = cell;
    if (!cell) {
      tableBar.hidden = true;
      return;
    }
    const rect = cell.closest("table").getBoundingClientRect();
    tableBar.hidden = false;
    // Above the table, lined up with its start edge (its right edge in a
    // right-to-left language); under it when there is no room above.
    Dialog.place(tableBar, rect, { above: true, gap: 6 });
  }

  document.addEventListener("selectionchange", updateTableBar);
  body.addEventListener("scroll", updateTableBar);

  body.addEventListener("keydown", (event) => {
    if (event.key !== "Tab" || slash.open) return;
    const cell = currentCell();
    if (!cell) return;
    event.preventDefault();
    const cells = [...cell.closest("table").querySelectorAll("td, th")];
    const index = cells.indexOf(cell);
    if (event.shiftKey) {
      if (index > 0) placeCaret(cells[index - 1], true);
      return;
    }
    if (index < cells.length - 1) {
      placeCaret(cells[index + 1], true);
    } else {
      tableActions.rowBelow(cell);
      placeCaret(cell.closest("table").rows[cell.closest("table").rows.length - 1].cells[0]);
      scheduleSave();
    }
  });

  // ------------------------------------------------------------ highlight + fonts

  const HIGHLIGHTS = [
    ["Yellow", "rgba(255, 214, 0, 0.38)"],
    ["Green", "rgba(52, 199, 89, 0.30)"],
    ["Blue", "rgba(10, 132, 255, 0.26)"],
    ["Red", "rgba(255, 69, 58, 0.24)"],
    ["Orange", "rgba(255, 149, 0, 0.32)"],
    ["Purple", "rgba(175, 82, 222, 0.26)"],
  ];
  const TEXT_COLOURS = [
    ["Red", "#e5484d"],
    ["Orange", "#e5700f"],
    ["Yellow", "#c49000"],
    ["Green", "#2f9e5f"],
    ["Blue", "#1f6fd1"],
    ["Purple", "#8e4ec6"],
    ["Grey", "#8b8d98"],
  ];

  /** A row of colour dots under a toolbar button. `pick(colour|null)`. */
  function colourPopover(button, colours, pick, noneLabel) {
    const box = document.createElement("div");
    box.className = "palette";
    box.hidden = true;
    for (const [name, colour] of colours) {
      const dot = document.createElement("button");
      dot.title = t(name);
      dot.style.setProperty("--c", colour);
      dot.addEventListener("click", () => {
        box.hidden = true;
        pick(colour);
      });
      box.appendChild(dot);
    }
    const none = document.createElement("button");
    none.className = "none";
    none.title = t(noneLabel);
    none.textContent = "⌀";
    none.addEventListener("click", () => {
      box.hidden = true;
      pick(null);
    });
    box.appendChild(none);
    box.addEventListener("mousedown", (event) => event.preventDefault());
    document.body.appendChild(box);
    button.addEventListener("click", () => {
      if (!box.hidden) return (box.hidden = true);
      const rect = button.getBoundingClientRect();
      box.hidden = false;
      Dialog.place(box, rect, { above: true, gap: 6 });
    });
    document.addEventListener("mousedown", (event) => {
      if (!box.hidden && !box.contains(event.target) && !button.contains(event.target)) box.hidden = true;
    });
    return box;
  }

  function highlight(colour) {
    document.execCommand("styleWithCSS", false, true);
    document.execCommand("hiliteColor", false, colour || "transparent");
    body.focus();
    scheduleSave();
  }

  function textColour(colour) {
    if (colour) {
      document.execCommand("styleWithCSS", false, false);
      document.execCommand("foreColor", false, colour);
    } else {
      // Back to the normal text colour: colour with a marker, then unwrap it,
      // so the text follows light and dark mode again.
      document.execCommand("styleWithCSS", false, false);
      document.execCommand("foreColor", false, "#010203");
      for (const font of body.querySelectorAll('font[color="#010203"]')) {
        font.replaceWith(...font.childNodes);
      }
      for (const span of body.querySelectorAll('[style*="rgb(1, 2, 3)"]')) span.style.color = "";
    }
    body.focus();
    scheduleSave();
  }

  // The format bar's highlight button opens colours instead of always using
  // yellow. Replacing the node drops app.js's old listener.
  const oldHl = document.querySelector(".format-bar button[data-hl]");
  const hlButton = oldHl.cloneNode(true);
  oldHl.replaceWith(hlButton);
  colourPopover(hlButton, HIGHLIGHTS, highlight, "No highlight");
  colourPopover($("text-colour"), TEXT_COLOURS, textColour, "Normal colour");
  for (const button of [hlButton, $("text-colour")]) {
    button.addEventListener("mousedown", (event) => event.preventDefault());
  }

  const FONTS = {
    sans: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif',
    serif: '"New York", ui-serif, Georgia, "Times New Roman", serif',
    mono: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace',
    rounded: 'ui-rounded, "SF Pro Rounded", "Nunito", system-ui, sans-serif',
  };

  function fontStack(name) {
    if (!name) return FONTS.sans;
    if (FONTS[name]) return FONTS[name];
    return `"${name.replace(/"/g, "")}", system-ui, sans-serif`;
  }

  // ------------------------------------------------------------ attachments

  function attachmentUrl(reference) {
    const [id, ...rest] = reference.split("/");
    return SHOWN + id + "/" + encodeURIComponent(rest.join("/"));
  }

  function referenceOf(url) {
    const tail = url.startsWith(SHOWN) ? url.slice(SHOWN.length) : url.slice(STORED.length);
    const [id, ...rest] = tail.split("/");
    return id + "/" + decodeURIComponent(rest.join("/"));
  }

  function attachmentHTML(reference, file) {
    const url = attachmentUrl(reference);
    const name = escapeHTML(file.name || reference.split("/").pop());
    const size = formatSize(file.size || 0);
    if ((file.type || "").startsWith("image/")) {
      const width = typeof pref === "function" ? pref("image_size", 100) : 100;
      const style = width && width < 100 ? ` style="width:${width}%"` : "";
      // In a paragraph of its own, so turning the next line into a heading
      // (or a list) doesn't pull the picture into it.
      return `<p><img src="${url}" alt="${name}"${style}></p><p><br></p>`;
    }
    if (file.type === "application/pdf" || /\.pdf$/i.test(file.name || "")) {
      return (
        `<figure class="pdf" contenteditable="false" data-file="${escapeHTML(reference)}">` +
        `<iframe src="${url}" title="${name}"></iframe>` +
        `<figcaption><span class="file-name">📄 ${name}</span><span class="file-size">${size}</span>` +
        `<button class="pdf-toggle" data-pdf="toggle">${escapeHTML(t("Hide preview"))}</button>` +
        `<button data-pdf="open">${escapeHTML(t("Open"))}</button></figcaption></figure><p><br></p>`
      );
    }
    return (
      `<span class="attachment" contenteditable="false" data-file="${escapeHTML(reference)}" title="${escapeHTML(t("Open {name}", { name }))}">` +
      `📎 ${name} <small>${size}</small></span>&nbsp;`
    );
  }

  async function addFiles(files) {
    const note = state.current;
    if (!note) return toast("Open a note first");
    for (const file of files) {
      if (file.size > 200 * 1024 * 1024) {
        toast(t("{name} is larger than 200 MB", { name: file.name }));
        continue;
      }
      try {
        const bytes = new Uint8Array(await file.arrayBuffer());
        const reference = await invoke("save_attachment", bytes, {
          headers: { "x-name": encodeURIComponent(file.name || "file"), "x-note": String(note) },
        });
        // Another note was opened while it uploaded: the file belongs to the
        // first one, so it isn't put into this one.
        if (state.current !== note) {
          toast(t("{name} was added to the note you were in", { name: file.name }));
          continue;
        }
        insertHTML(attachmentHTML(reference, file));
      } catch (error) {
        toast(String(error));
      }
    }
  }

  function pickFiles(accept) {
    keepingSelection((restore) => {
      const input = document.createElement("input");
      input.type = "file";
      input.multiple = true;
      if (accept) input.accept = accept;
      input.addEventListener("change", () => {
        restore();
        addFiles([...input.files]);
      });
      input.click();
    });
  }

  body.addEventListener("paste", (event) => {
    const files = [...(event.clipboardData?.files || [])];
    if (!files.length) return;
    event.preventDefault();
    addFiles(files);
  });

  body.addEventListener("dragover", (event) => {
    if ([...(event.dataTransfer?.types || [])].includes("Files")) event.preventDefault();
  });

  body.addEventListener("drop", (event) => {
    const files = [...(event.dataTransfer?.files || [])];
    if (!files.length) return;
    event.preventDefault();
    const range = document.caretRangeFromPoint?.(event.clientX, event.clientY);
    if (range) {
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
    }
    addFiles(files);
  });

  body.addEventListener("click", (event) => {
    const chip = event.target.closest(".attachment");
    const action = event.target.closest("[data-pdf]");
    if (action) {
      event.preventDefault();
      const figure = action.closest("figure.pdf");
      if (action.dataset.pdf === "open") {
        invoke("open_attachment", { reference: figure.dataset.file }).catch((e) => toast(String(e)));
      } else {
        const hidden = figure.classList.toggle("collapsed");
        action.textContent = t(hidden ? "Show preview" : "Hide preview");
        scheduleSave();
      }
      return;
    }
    if (chip) {
      event.preventDefault();
      invoke("open_attachment", { reference: chip.dataset.file }).catch((e) => toast(String(e)));
      return;
    }
    const link = event.target.closest("a");
    const href = link?.getAttribute("href") || "";
    // Web links open in the browser: with ⌘ (or Ctrl) while writing, or a
    // plain click on a video's caption.
    if (link && /^https?:\/\//i.test(href) && (event.metaKey || event.ctrlKey || link.closest("figcaption"))) {
      event.preventDefault();
      invoke("open_url", { url: href }).catch((e) => toast(String(e)));
      return;
    }
    if (link && isAttachmentUrl(href)) {
      event.preventDefault();
      invoke("open_attachment", { reference: referenceOf(link.getAttribute("href")) }).catch((e) =>
        toast(String(e))
      );
    }
  });

  // ------------------------------------------------------------ videos

  function videoEmbed(text) {
    let url;
    try {
      url = new URL(text.trim());
    } catch {
      return null;
    }
    const host = url.hostname.replace(/^(www\.|m\.|music\.)/, "");
    let id;
    if (host === "youtu.be") id = url.pathname.slice(1);
    else if (host === "youtube.com" || host === "youtube-nocookie.com") {
      id = url.searchParams.get("v") || (url.pathname.match(/^\/(embed|shorts|live)\/([\w-]+)/) || [])[2];
    }
    if (id && /^[\w-]{6,}$/.test(id)) return `https://www.youtube-nocookie.com/embed/${id}`;
    if (host === "vimeo.com" || host === "player.vimeo.com") {
      const vid = (url.pathname.match(/(\d{5,})/) || [])[1];
      if (vid) return `https://player.vimeo.com/video/${vid}`;
    }
    if (host === "dailymotion.com" || host === "dai.ly") {
      const vid = host === "dai.ly"
        ? url.pathname.slice(1)
        : (url.pathname.match(/\/(?:embed\/)?video\/([\w]+)/) || [])[1];
      if (vid && /^\w+$/.test(vid)) return `https://www.dailymotion.com/embed/video/${vid}`;
    }
    return null;
  }

  function insertVideo() {
    keepingSelection(async (restore) => {
      const text = await Dialog.ask({
        title: "Add a video",
        label: "Paste a link to a YouTube, Vimeo or Dailymotion video.",
        placeholder: "https://www.youtube.com/watch?v=…",
        ok: "Add video",
      });
      restore();
      if (!text || !text.trim()) return;
      const embed = videoEmbed(text);
      if (!embed) return toast("That isn't a YouTube, Vimeo or Dailymotion link");
      insertHTML(
        `<figure class="video" contenteditable="false">` +
          `<iframe src="${embed}" allow="fullscreen; picture-in-picture; encrypted-media" allowfullscreen referrerpolicy="strict-origin-when-cross-origin"></iframe>` +
          `<figcaption><a href="${escapeHTML(text.trim())}">${escapeHTML(text.trim())}</a></figcaption>` +
          `</figure><p><br></p>`
      );
    });
  }

  // ------------------------------------------------------------ maths

  async function mathHTML(tex, isBlock) {
    const mathml = await invoke("render_math", { tex, block: isBlock });
    const tag = isBlock ? "div" : "span";
    return `<${tag} class="math${isBlock ? " block" : ""}" contenteditable="false" data-tex="${escapeHTML(tex)}" title="${escapeHTML(t("Click to edit"))}">${mathml}</${tag}>`;
  }

  function askFormula(isBlock, value = "") {
    return Dialog.formula({
      value,
      hint: value
        ? ""
        : "This formula was written into the file without its LaTeX. Paste it here to edit it.",
      render: (tex) => invoke("render_math", { tex, block: isBlock }),
    });
  }

  function insertMath(isBlock) {
    keepingSelection(async (restore) => {
      const tex = await askFormula(isBlock);
      restore();
      if (!tex || !tex.trim()) return;
      try {
        const html = await mathHTML(tex, isBlock);
        restore();
        insertHTML(isBlock ? html + "<p><br></p>" : html + "&nbsp;");
      } catch (error) {
        toast(String(error));
      }
    });
  }

  // Right-click a picture to choose how wide it is.
  body.addEventListener("contextmenu", (event) => {
    const image = event.target.closest("img");
    if (!image) return;
    event.preventDefault();
    const set = (percent) => {
      if (percent === null) image.removeAttribute("style");
      else image.style.width = percent + "%";
      scheduleSave();
    };
    const align = (where) => {
      image.style.float = "";
      image.style.display = "";
      image.style.margin = "";
      if (where === "left") image.style.float = "left";
      if (where === "right") image.style.float = "right";
      if (where === "center") {
        image.style.display = "block";
        image.style.margin = "10px auto";
      }
      if (where === "left" || where === "right") image.style.margin = "4px 12px 8px 0";
      scheduleSave();
    };
    Dialog.menu(event.clientX, event.clientY, [
      { label: "Full width", run: () => set(100) },
      { label: "Half width", run: () => set(50) },
      { label: "Quarter width", run: () => set(25) },
      { label: "Original size", run: () => set(null) },
      { separator: true },
      { label: "Align left", run: () => align("left") },
      { label: "Centre", run: () => align("center") },
      { label: "Align right", run: () => align("right") },
    ]);
  });

  body.addEventListener("click", async (event) => {
    // Formulas made here carry their LaTeX; a bare <math> from a file or an
    // import has none, so it opens with an empty field instead of doing
    // nothing.
    const math = event.target.closest(".math") || event.target.closest("math");
    if (!math) return;
    event.preventDefault();
    const stored = (math.dataset.tex || (math.closest(".math")?.dataset.tex ?? "")).trim();
    const tex = await askFormula(math.classList.contains("block"), stored);
    if (tex === null) return;
    if (!tex.trim()) {
      math.remove();
      scheduleSave();
      return;
    }
    try {
      const html = await mathHTML(tex, math.classList.contains("block"));
      const holder = document.createElement("div");
      holder.innerHTML = html;
      math.replaceWith(holder.firstElementChild);
      scheduleSave();
    } catch (error) {
      toast(String(error));
    }
  });

  // ------------------------------------------------------------ "/" menu

  const COMMANDS = [
    { label: "Heading 1", hint: "Large heading", icon: "H1", words: "title", run: () => block("h1") },
    { label: "Heading 2", hint: "Medium heading", icon: "H2", words: "subtitle", run: () => block("h2") },
    { label: "Heading 3", hint: "Small heading", icon: "H3", run: () => block("h3") },
    { label: "Bulleted list", icon: "•", words: "bullet unordered", run: () => document.execCommand("insertUnorderedList") },
    { label: "Numbered list", icon: "1.", words: "ordered", run: () => document.execCommand("insertOrderedList") },
    { label: "Checklist", icon: "☑", words: "todo task", run: () => $("checklist").click() },
    { label: "Quote", icon: "❝", words: "blockquote", run: () => block("blockquote") },
    { label: "Code block", icon: "{ }", words: "pre snippet", run: insertCodeBlock },
    { label: "Table", icon: "▦", words: "grid", run: () => insertTable() },
    { label: "Divider", icon: "—", words: "line rule hr", run: insertDivider },
    { label: "Collapsible section", icon: "▾", words: "toggle details fold", run: insertCollapsible },
    { label: "Image", hint: "From this computer", icon: "🖼", words: "picture photo", run: () => pickFiles("image/*") },
    { label: "File", hint: "Any file; PDFs show a preview", icon: "📎", words: "attachment pdf upload", run: () => pickFiles("") },
    { label: "Video", hint: "YouTube, Vimeo, Dailymotion", icon: "▶", words: "youtube vimeo embed", run: insertVideo },
    { label: "Math", hint: "LaTeX in the line", icon: "∑", words: "latex formula equation", run: () => insertMath(false) },
    { label: "Math block", hint: "LaTeX on its own line", icon: "∫", words: "latex formula equation", run: () => insertMath(true) },
    { label: "Date", hint: "Today's date", icon: "📅", words: "today time", run: insertDate },
    { label: "Link to note", icon: "↗", words: "internal reference", run: () => insertNoteLink() },
  ];

  const slash = { open: false, node: null, offset: 0, items: [], index: 0 };
  const menu = document.createElement("div");
  menu.className = "slash-menu";
  menu.hidden = true;
  document.body.appendChild(menu);
  menu.addEventListener("mousedown", (event) => event.preventDefault());

  // ------------------------------------------------- markdown while typing
  // With the setting on, a line that starts "# ", "- ", "1. " or "> " turns
  // into that block as soon as the space is typed.

  const MARKDOWN_STARTS = [
    [/^###\s$/, "h3"],
    [/^##\s$/, "h2"],
    [/^#\s$/, "h1"],
    [/^[-*]\s\[\s?\]\s$/, "checklist"],
    [/^[-*]\s$/, "bullet"],
    [/^\d+\.\s$/, "number"],
    [/^>\s$/, "quote"],
  ];

  function markdownOnSpace() {
    if (!pref("markdown_mode", false)) return false;
    const selection = window.getSelection();
    if (!selection || !selection.rangeCount || !selection.isCollapsed) return false;
    const node = selection.anchorNode;
    if (!node || node.nodeType !== 3 || !body.contains(node)) return false;

    // The space hasn't been typed yet (this runs on keydown), so it is added
    // here for the patterns, which all end with it.
    const before = node.textContent.slice(0, selection.anchorOffset) + " ";
    if (!before.trim()) return false;
    // Only at the start of a line: nothing before this text in its block.
    const block = node.parentElement?.closest("p, div, li, h1, h2, h3, blockquote") || body;
    const lead = document.createRange();
    lead.setStart(block === body ? body : block, 0);
    lead.setEnd(node, 0);
    if (lead.toString().trim()) return false;

    for (const [pattern, kind] of MARKDOWN_STARTS) {
      if (!pattern.test(before)) continue;
      const range = document.createRange();
      range.setStart(node, 0);
      range.setEnd(node, selection.anchorOffset);
      range.deleteContents();

      if (kind === "bullet") document.execCommand("insertUnorderedList");
      else if (kind === "number") document.execCommand("insertOrderedList");
      else if (kind === "checklist") $("checklist").click();
      else document.execCommand("formatBlock", false, kind);
      scheduleSave();
      return true;
    }
    return false;
  }

  body.addEventListener("keydown", (event) => {
    if (event.key !== " " || event.metaKey || event.ctrlKey) return;
    if (markdownOnSpace()) event.preventDefault();
  }, true);

  function slashEnabled() {
    return state.prefs.slash_menu !== false;
  }

  function closeMenu() {
    slash.open = false;
    menu.hidden = true;
  }

  function query() {
    const selection = window.getSelection();
    if (!selection.rangeCount) return null;
    const range = selection.getRangeAt(0);
    if (range.startContainer !== slash.node || range.startOffset <= slash.offset) return null;
    if (slash.node.data[slash.offset] !== "/") return null;
    return slash.node.data.slice(slash.offset + 1, range.startOffset);
  }

  function matches(text) {
    const q = text.toLowerCase().trim();
    if (!q) return COMMANDS;
    // The English name and the translated one both find a command.
    return COMMANDS.filter((c) => {
      const names = c.label + " " + t(c.label);
      return (names + " " + (c.words || "")).toLowerCase().split(/\s+/).some((w) => w.startsWith(q)) ||
        names.toLowerCase().includes(q);
    });
  }

  function paintMenu() {
    menu.innerHTML = "";
    slash.items.forEach((command, i) => {
      const item = document.createElement("button");
      item.className = "slash-item" + (i === slash.index ? " active" : "");
      item.innerHTML = '<span class="slash-icon"></span><span class="slash-text"><b></b><i></i></span>';
      item.querySelector(".slash-icon").textContent = command.icon;
      item.querySelector("b").textContent = t(command.label);
      item.querySelector("i").textContent = t(command.hint || "");
      item.addEventListener("mouseenter", () => {
        slash.index = i;
        for (const [j, el] of [...menu.children].entries()) el.classList.toggle("active", j === i);
      });
      item.addEventListener("click", () => choose(i));
      menu.appendChild(item);
    });
    menu.children[slash.index]?.scrollIntoView({ block: "nearest" });
  }

  function placeMenu() {
    const selection = window.getSelection();
    let rect = selection.rangeCount ? selection.getRangeAt(0).getBoundingClientRect() : null;
    if (!rect || (!rect.top && !rect.left)) rect = caretElement()?.getBoundingClientRect() || body.getBoundingClientRect();
    menu.hidden = false;
    // Opens from the caret: to the right of it in English, to the left in
    // Arabic, below the line when it fits.
    const caret = { left: rect.left, right: rect.left, top: rect.top, bottom: rect.bottom, width: 0 };
    if (document.documentElement.dir === "rtl") caret.left = caret.right = rect.right;
    Dialog.place(menu, caret, { gap: 6 });
  }

  function openMenu(node, offset) {
    slash.open = true;
    slash.node = node;
    slash.offset = offset;
    slash.items = COMMANDS;
    slash.index = 0;
    paintMenu();
    placeMenu();
  }

  function choose(i) {
    const command = slash.items[i];
    if (!command) return;
    // Remove the "/query" that opened the menu, then run the command there.
    const selection = window.getSelection();
    const range = document.createRange();
    const end = selection.rangeCount && selection.getRangeAt(0).startContainer === slash.node
      ? selection.getRangeAt(0).startOffset
      : slash.offset + 1;
    range.setStart(slash.node, slash.offset);
    range.setEnd(slash.node, Math.max(end, slash.offset + 1));
    range.deleteContents();
    selection.removeAllRanges();
    selection.addRange(range);
    closeMenu();
    body.focus();
    command.run();
    scheduleSave();
  }

  body.addEventListener("input", (event) => {
    if (slash.open) {
      const q = query();
      if (q === null || q.length > 30 || /\s\s/.test(q)) return closeMenu();
      slash.items = matches(q);
      if (!slash.items.length) {
        if (q.length > 2) return closeMenu();
      }
      slash.index = 0;
      paintMenu();
      placeMenu();
      return;
    }
    if (event.inputType !== "insertText" || event.data !== "/" || !slashEnabled()) return;
    const selection = window.getSelection();
    if (!selection.rangeCount) return;
    const range = selection.getRangeAt(0);
    const node = range.startContainer;
    if (node.nodeType !== 3 || range.startOffset < 1) return;
    if (caretElement()?.closest("pre, code")) return;
    const before = node.data[range.startOffset - 2];
    if (before !== undefined && !/\s/.test(before)) return;
    openMenu(node, range.startOffset - 1);
  });

  body.addEventListener(
    "keydown",
    (event) => {
      if (!slash.open) return;
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const n = slash.items.length;
        if (!n) return;
        slash.index = (slash.index + (event.key === "ArrowDown" ? 1 : n - 1)) % n;
        paintMenu();
      } else if (event.key === "Enter" || event.key === "Tab") {
        if (!slash.items.length) return closeMenu();
        event.preventDefault();
        event.stopImmediatePropagation();
        choose(slash.index);
      } else if (event.key === "Escape") {
        event.preventDefault();
        closeMenu();
      }
    },
    true
  );

  body.addEventListener("blur", () => setTimeout(closeMenu, 120));
  document.addEventListener("mousedown", (event) => {
    if (slash.open && !menu.contains(event.target)) closeMenu();
  });

  /** The format bar's "+" opens the same menu where the caret is. */
  function openInsertMenu() {
    body.focus();
    const selection = window.getSelection();
    if (!selection.rangeCount || !body.contains(selection.anchorNode)) {
      placeCaret(body, true);
    }
    const range = window.getSelection().getRangeAt(0);
    range.deleteContents();
    const text = document.createTextNode("/");
    range.insertNode(text);
    range.setStart(text, 1);
    range.collapse(true);
    window.getSelection().removeAllRanges();
    window.getSelection().addRange(range);
    openMenu(text, 0);
  }

  // ------------------------------------------------------------ format bar

  const actions = {
    code: inlineCode,
    codeblock: insertCodeBlock,
    divider: insertDivider,
    table: () => insertTable(),
    file: () => pickFiles(""),
    math: () => insertMath(false),
    insert: openInsertMenu,
  };
  for (const button of document.querySelectorAll(".format-bar [data-x]")) {
    button.addEventListener("mousedown", (event) => event.preventDefault());
    button.addEventListener("click", () => actions[button.dataset.x]());
  }

  // The bar scrolls instead of hiding tools behind a "…" menu. Vertical
  // wheel over it moves it sideways, for mice without a horizontal gesture.
  const bar = document.querySelector(".format-bar");

  /** Fade whichever edge still has tools beyond it. */
  function paintBarEdges() {
    const room = bar.scrollWidth - bar.clientWidth;
    bar.classList.toggle("is-scrollable", room > 1);
    bar.classList.toggle("at-start", bar.scrollLeft <= 1);
    bar.classList.toggle("at-end", bar.scrollLeft >= room - 1);
  }

  bar.addEventListener("scroll", paintBarEdges, { passive: true });
  bar.addEventListener(
    "wheel",
    (event) => {
      if (event.shiftKey || Math.abs(event.deltaX) > Math.abs(event.deltaY)) return;
      const room = bar.scrollWidth - bar.clientWidth;
      if (room <= 1) return;
      event.preventDefault();
      bar.scrollLeft = Math.max(0, Math.min(room, bar.scrollLeft + event.deltaY));
    },
    { passive: false },
  );
  new ResizeObserver(paintBarEdges).observe(bar);
  paintBarEdges();
  // Start at the first tools, whatever the engine restored for the bar.
  requestAnimationFrame(() => {
    bar.scrollLeft = 0;
    paintBarEdges();
  });

  // ------------------------------------------------------------ "Aa": font and size

  const textStyle = document.createElement("div");
  textStyle.className = "text-style-pop";
  textStyle.hidden = true;
  textStyle.innerHTML = `
    <div class="pop-title">Font</div>
    <div class="font-options">
      <button data-font="sans"><b style="font-family:${FONTS.sans.replace(/"/g, "'")}">Aa</b>Sans</button>
      <button data-font="serif"><b style="font-family:${FONTS.serif.replace(/"/g, "'")}">Aa</b>Serif</button>
      <button data-font="rounded"><b style="font-family:${FONTS.rounded.replace(/"/g, "'")}">Aa</b>Rounded</button>
      <button data-font="mono"><b style="font-family:${FONTS.mono.replace(/"/g, "'")}">Aa</b>Mono</button>
    </div>
    <div class="pop-title">Text size</div>
    <div class="size-row">
      <button data-size="-1" title="Smaller">A−</button>
      <span class="size-label"></span>
      <button data-size="1" title="Larger">A+</button>
    </div>
    <p class="pop-hint">Any font on this computer can be chosen in Settings → Editor.</p>`;
  document.body.appendChild(textStyle);
  translateDom(textStyle);

  function paintTextStyle() {
    const font = pref("editor_font", "") || "sans";
    for (const b of textStyle.querySelectorAll("[data-font]")) b.classList.toggle("on", b.dataset.font === font);
    textStyle.querySelector(".size-label").textContent = `${state.prefs.editor_font_size || 15}px`;
  }

  textStyle.addEventListener("click", (event) => {
    const fontButton = event.target.closest("[data-font]");
    const sizeButton = event.target.closest("[data-size]");
    if (fontButton) {
      // The same setting as Settings → Editor → Typeface ("" = system).
      const value = fontButton.dataset.font === "sans" ? "" : fontButton.dataset.font;
      setUiPref("editor_font", value);
      if (value) document.documentElement.dataset.font = value;
      else delete document.documentElement.dataset.font;
      const picker = document.getElementById("e-font");
      if (picker) picker.value = value;
    }
    if (sizeButton) {
      const next = (state.prefs.editor_font_size || 15) + Number(sizeButton.dataset.size);
      state.prefs.editor_font_size = Math.min(22, Math.max(12, next));
    }
    if (fontButton || sizeButton) {
      applyPrefs();
      document.documentElement.style.setProperty("--editor-size", state.prefs.editor_font_size + "px");
      savePrefs();
      paintTextStyle();
    }
  });

  $("text-style").addEventListener("click", () => {
    if (!textStyle.hidden) return (textStyle.hidden = true);
    paintTextStyle();
    const rect = $("text-style").getBoundingClientRect();
    textStyle.hidden = false;
    Dialog.place(textStyle, rect, { align: "end", above: true, gap: 8 });
  });
  document.addEventListener("mousedown", (event) => {
    if (!textStyle.hidden && !textStyle.contains(event.target) && event.target !== $("text-style")) {
      textStyle.hidden = true;
    }
  });

  // ------------------------------------------------------------ settings

  function applyPrefs() {
    // Typeface comes from data-font (Settings → Editor); nothing to set here.
  }

  // The typeface is chosen with the "Typeface" picker in Settings → Editor
  // (and "Aa"); this adds only the "/" menu switch next to it.
  const editorPage = document.querySelector('[data-page="editor"]');
  const settingsBox = document.createElement("div");
  settingsBox.innerHTML = `<label class="check"><input type="checkbox" id="e-slash" /> Type “/” to open the insert menu</label>`;
  editorPage.appendChild(settingsBox);

  function renderSettings() {
    settingsBox.querySelector("#e-slash").checked = state.prefs.slash_menu !== false;
  }

  settingsBox.querySelector("#e-slash").addEventListener("change", (event) => {
    state.prefs.slash_menu = event.target.checked;
    savePrefs();
  });

  /** A stored image address as the editor shows it (only differs on Windows). */
  function shown(url) {
    return IS_WINDOWS && url.startsWith(STORED) ? SHOWN + url.slice(STORED.length) : url;
  }

  return { keepingSelection, load, store, clean, shown, applyPrefs, renderSettings, videoEmbed, addFiles };
})();
