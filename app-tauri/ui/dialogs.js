// In-app dialogs, used instead of the browser's prompt(): a text question
// (optionally with a live preview), a searchable list to pick from, and a
// small menu at the pointer. Each returns a promise; null means cancelled.

const Dialog = (() => {
  const escapeHTML = (text) =>
    String(text).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

  function sheet(inner) {
    const modal = document.createElement("div");
    modal.className = "modal dialog";
    modal.innerHTML = `<div class="sheet dialog-sheet">${inner}</div>`;
    document.body.appendChild(modal);
    return modal;
  }

  /** Remove a sheet after the close animation (see .modal.is-closing). */
  function closeSheet(modal) {
    if (!modal.isConnected || modal.classList.contains("is-closing")) return;
    modal.classList.add("is-closing");
    setTimeout(() => modal.remove(), 190);
  }

  /**
   * Ask for a line of text.
   * options: title, label, value, placeholder, hint, ok, password, multiline,
   *          preview(value) → Promise<html> (shown under the field; throwing
   *          shows the message instead).
   */
  function ask(options) {
    const o = { ok: "OK", value: "", ...options };
    const field = o.multiline
      ? `<textarea class="dialog-input" rows="3" placeholder="${escapeHTML(t(o.placeholder || ""))}"></textarea>`
      : `<input class="dialog-input" type="${o.password ? "password" : "text"}" placeholder="${escapeHTML(t(o.placeholder || ""))}" />`;
    const modal = sheet(`
      <h3>${escapeHTML(t(o.title || ""))}</h3>
      ${o.label ? `<p class="hint">${escapeHTML(t(o.label))}</p>` : ""}
      ${field}
      ${o.preview ? '<div class="dialog-preview"></div>' : ""}
      ${o.hint ? `<p class="hint dialog-hint">${escapeHTML(t(o.hint))}</p>` : ""}
      <div class="modal-row">
        <span class="spacer"></span>
        <button class="ghost dialog-cancel">${escapeHTML(t("Cancel"))}</button>
        <button class="primary dialog-ok">${escapeHTML(t(o.ok))}</button>
      </div>`);
    const input = modal.querySelector(".dialog-input");
    const preview = modal.querySelector(".dialog-preview");
    input.value = o.value;

    let ticket = 0;
    async function paint() {
      if (!preview) return;
      const mine = ++ticket;
      const value = input.value.trim();
      if (!value) {
        preview.innerHTML = "";
        preview.classList.remove("error");
        return;
      }
      try {
        const html = await o.preview(value);
        if (mine !== ticket) return;
        preview.classList.remove("error");
        preview.innerHTML = html;
      } catch (error) {
        if (mine !== ticket) return;
        preview.classList.add("error");
        preview.textContent = String(error);
      }
    }
    input.addEventListener("input", paint);
    paint();

    return new Promise((resolve) => {
      const done = (value) => {
        closeSheet(modal);
        resolve(value);
      };
      modal.querySelector(".dialog-ok").addEventListener("click", () => done(input.value));
      modal.querySelector(".dialog-cancel").addEventListener("click", () => done(null));
      modal.addEventListener("mousedown", (event) => {
        if (event.target === modal) done(null);
      });
      input.addEventListener("keydown", (event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          done(null);
        } else if (event.key === "Enter" && (!o.multiline || event.metaKey || event.ctrlKey)) {
          event.preventDefault();
          done(input.value);
        }
      });
      setTimeout(() => {
        input.focus();
        input.select();
      });
    });
  }

  /**
   * Pick one item from a list you can type to filter.
   * items: [{ label, sub?, value, colour? }]
   */
  function pick({ title, items, placeholder = "Search", empty = "Nothing matches" }) {
    const modal = sheet(`
      <h3>${escapeHTML(t(title))}</h3>
      <input class="dialog-input" placeholder="${escapeHTML(t(placeholder))}" />
      <div class="pick-list"></div>
      <div class="modal-row"><span class="spacer"></span><button class="ghost dialog-cancel">${escapeHTML(t("Cancel"))}</button></div>`);
    const input = modal.querySelector(".dialog-input");
    const list = modal.querySelector(".pick-list");
    let shown = items;
    let index = 0;

    return new Promise((resolve) => {
      const done = (value) => {
        closeSheet(modal);
        resolve(value);
      };
      function paint() {
        const q = input.value.trim().toLowerCase();
        shown = q ? items.filter((i) => (i.label + " " + (i.sub || "")).toLowerCase().includes(q)) : items;
        index = Math.min(index, Math.max(0, shown.length - 1));
        list.innerHTML = "";
        if (!shown.length) {
          list.innerHTML = `<p class="hint">${escapeHTML(t(empty))}</p>`;
          return;
        }
        shown.forEach((item, i) => {
          const row = document.createElement("button");
          row.className = "pick-item" + (i === index ? " active" : "");
          row.innerHTML = `${item.colour ? '<span class="pick-dot"></span>' : ""}<span class="pick-text"><b></b><i></i></span>`;
          if (item.colour) row.querySelector(".pick-dot").style.background = item.colour;
          row.querySelector("b").textContent = item.label;
          row.querySelector("i").textContent = item.sub || "";
          row.addEventListener("click", () => done(item.value));
          list.appendChild(row);
        });
        list.children[index]?.scrollIntoView({ block: "nearest" });
      }
      input.addEventListener("input", () => {
        index = 0;
        paint();
      });
      input.addEventListener("keydown", (event) => {
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          if (!shown.length) return;
          index = (index + (event.key === "ArrowDown" ? 1 : shown.length - 1)) % shown.length;
          paint();
        } else if (event.key === "Enter") {
          event.preventDefault();
          if (shown[index]) done(shown[index].value);
        } else if (event.key === "Escape") {
          event.preventDefault();
          done(null);
        }
      });
      modal.querySelector(".dialog-cancel").addEventListener("click", () => done(null));
      modal.addEventListener("mousedown", (event) => {
        if (event.target === modal) done(null);
      });
      paint();
      setTimeout(() => input.focus());
    });
  }

  /**
   * Put a floating box (menu, palette, popover) next to what opened it.
   * `anchor` is the opener's rect, or a point { x, y } such as the pointer.
   * align "start": the box's leading edge lines up with the anchor's (left
   * edges in a left-to-right language, right edges in Arabic); "end": the
   * trailing edges do. So in a right-to-left language everything opens
   * mirrored. Below the anchor when it fits (or `above: true` asks for
   * above), otherwise on the other side, always kept inside the window.
   */
  function place(box, anchor, { align = "start", above = false, gap = 4 } = {}) {
    const rect = "x" in anchor && !("width" in anchor)
      ? { left: anchor.x, right: anchor.x, top: anchor.y, bottom: anchor.y }
      : anchor;
    const rtl = document.documentElement.dir === "rtl";
    const width = box.offsetWidth;
    const height = box.offsetHeight;
    const alignRight = rtl ? align === "start" : align === "end";
    let left = alignRight ? rect.right - width : rect.left;
    left = Math.max(8, Math.min(left, window.innerWidth - width - 8));
    const below = rect.bottom + gap;
    const over = rect.top - height - gap;
    const fitsBelow = below + height <= window.innerHeight - 8;
    const fitsAbove = over >= 8;
    let top = above ? (fitsAbove || !fitsBelow ? over : below) : fitsBelow || !fitsAbove ? below : over;
    top = Math.max(8, Math.min(top, window.innerHeight - height - 8));
    box.style.left = left + "px";
    box.style.top = top + "px";
  }

  /** A native action menu: a bottom sheet on phones, a popover on desktop. */
  function menu(x, y, items, options = {}) {
    document.querySelector(".context-menu")?.remove();
    document.querySelector(".menu-scrim")?.remove();
    // Only phones have sheet styles; a narrow desktop window keeps its popover.
    const mobile = ["ios", "android"].includes(document.documentElement.dataset.platform);
    const box = document.createElement("div");
    box.className = "context-menu" + (mobile ? " menu-sheet" : "");
    let scrim = null;
    const close = (event) => {
      if (event && box.contains(event.target)) return;
      if (mobile) {
        // Phones slide the sheet away; the desktop menu just goes.
        box.classList.add("is-leaving");
        scrim?.classList.add("is-leaving");
        setTimeout(() => {
          box.remove();
          scrim?.remove();
        }, 220);
      } else {
        box.remove();
      }
      document.removeEventListener("mousedown", close, true);
      document.removeEventListener("keydown", esc);
    };
    function esc(event) {
      if (event.key === "Escape") close();
    }
    for (const item of items) {
      if (item.separator) {
        const rule = document.createElement("div");
        rule.className = "menu-sep";
        box.appendChild(rule);
        continue;
      }
      const row = document.createElement("button");
      if (item.icon) {
        const glyph = document.createElement("span");
        glyph.className = "menu-icon";
        glyph.innerHTML = item.icon;
        row.appendChild(glyph);
      }
      if (item.check !== undefined) {
        const tick = document.createElement("span");
        tick.className = "menu-check";
        tick.textContent = item.check ? "✓" : "";
        row.appendChild(tick);
      }
      const label = document.createElement("span");
      label.className = "menu-label";
      label.textContent = t(item.label);
      row.appendChild(label);
      if (item.hint) {
        const hint = document.createElement("span");
        hint.className = "menu-hint";
        hint.textContent = t(item.hint);
        row.appendChild(hint);
      }
      if (item.danger) row.classList.add("danger");
      if (item.checked || item.check) row.classList.add("checked");
      row.addEventListener("click", () => {
        close();
        item.run();
      });
      box.appendChild(row);
    }
    if (mobile) {
      const cancel = document.createElement("button");
      cancel.className = "menu-cancel";
      cancel.textContent = t("Cancel");
      cancel.addEventListener("click", () => close());
      box.appendChild(cancel);
      scrim = document.createElement("div");
      scrim.className = "menu-scrim";
      scrim.addEventListener("click", () => close());
      document.body.appendChild(scrim);
    }
    document.body.appendChild(box);
    if (typeof window.applyPlatformLabels === "function") window.applyPlatformLabels(box);
    if (!mobile) {
      place(box, options.anchor || { x, y }, { align: options.align || "start", gap: options.anchor ? 4 : 0 });
      document.addEventListener("mousedown", close, true);
    }
    document.addEventListener("keydown", esc);
  }

  /** A yes/no question. `confirm()` from the browser does nothing here. */
  function confirm(options) {
    const o = { title: "", label: "", ok: "Delete", cancel: "Cancel", danger: true, ...options };
    return new Promise((resolve) => {
      const modal = sheet(`
        <h3>${escapeHTML(t(o.title))}</h3>
        ${o.label ? `<p class="hint">${escapeHTML(t(o.label))}</p>` : ""}
        <div class="modal-row">
          <span class="spacer"></span>
          <button class="ghost dialog-cancel">${escapeHTML(t(o.cancel))}</button>
          <button class="primary dialog-ok${o.danger ? " danger" : ""}">${escapeHTML(t(o.ok))}</button>
        </div>`);
      let done = false;
      const onKey = (event) => {
        if (event.key === "Escape") close(false);
        if (event.key === "Enter") close(true);
      };
      const close = (value) => {
        if (done) return;
        done = true;
        document.removeEventListener("keydown", onKey, true);
        closeSheet(modal);
        resolve(value);
      };
      modal.querySelector(".dialog-cancel").addEventListener("click", () => close(false));
      modal.querySelector(".dialog-ok").addEventListener("click", () => close(true));
      modal.addEventListener("mousedown", (event) => {
        if (event.target === modal) close(false);
      });
      document.addEventListener("keydown", onKey, true);
      modal.querySelector(".dialog-ok").focus();
    });
  }

  /**
   * The formula editor: a large field, a live preview, Cancel and Save in the
   * header. Resolves to the LaTeX, or null when cancelled.
   */
  function formula({ value = "", render, hint = "" }) {
    const modal = document.createElement("div");
    modal.className = "modal dialog formula-modal";
    modal.innerHTML = `
      <div class="formula-sheet">
        <div class="formula-head">
          <button class="formula-cancel">${escapeHTML(t("Cancel"))}</button>
          <span class="formula-title">${escapeHTML(t("TeX Formula"))}</span>
          <button class="formula-save">${escapeHTML(t("Save"))}</button>
        </div>
        <div class="formula-body">
          <textarea class="formula-input" spellcheck="false" placeholder="x=\\frac{-b\\pm\\sqrt{b^2-4ac}}{2a}"></textarea>
          ${hint ? `<p class="hint formula-hint"></p>` : ""}
          <div class="formula-preview">
            <button class="formula-copy" title="Copy the formula">
              <svg viewBox="0 0 24 24"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5h10"/></svg>
            </button>
            <div class="formula-output"></div>
          </div>
        </div>
      </div>`;
    document.body.appendChild(modal);
    translateDom(modal);

    const input = modal.querySelector(".formula-input");
    const output = modal.querySelector(".formula-output");
    const save = modal.querySelector(".formula-save");
    const hintEl = modal.querySelector(".formula-hint");
    if (hintEl) hintEl.textContent = t(hint);
    input.value = value;

    let drawn = "";
    let ticket = 0;
    async function paint() {
      const tex = input.value.trim();
      const mine = ++ticket;
      save.disabled = false;
      output.classList.remove("bad");
      if (!tex) {
        output.textContent = t("Nothing to show yet");
        output.classList.add("empty");
        return;
      }
      output.classList.remove("empty");
      try {
        const html = await render(tex);
        if (mine !== ticket) return;
        drawn = html;
        output.innerHTML = html;
      } catch (error) {
        if (mine !== ticket) return;
        drawn = "";
        output.textContent = String(error);
        output.classList.add("bad");
        save.disabled = true;
      }
    }

    return new Promise((resolve) => {
      let done = false;
      const close = (result) => {
        if (done) return;
        done = true;
        closeSheet(modal);
        document.removeEventListener("keydown", onKey, true);
        resolve(result);
      };
      const onKey = (event) => {
        if (event.key === "Escape") close(null);
        if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) close(input.value.trim());
      };
      input.addEventListener("input", paint);
      document.addEventListener("keydown", onKey, true);
      modal.querySelector(".formula-cancel").addEventListener("click", () => close(null));
      save.addEventListener("click", () => close(input.value.trim()));
      modal.addEventListener("mousedown", (event) => {
        if (event.target === modal) close(null);
      });
      modal.querySelector(".formula-copy").addEventListener("click", async () => {
        try {
          await navigator.clipboard.writeText(input.value.trim());
        } catch {
          /* the app has a clipboard command, but this is a nicety */
        }
      });
      paint();
      input.focus();
      input.setSelectionRange(input.value.length, input.value.length);
    });
  }

  /**
   * Pick any number of items from a list you can type to filter.
   * items: [{ label, sub?, value, colour?, checked? }]
   * Returns the chosen values in list order, or null when cancelled.
   */
  function multiPick({
    title,
    items,
    placeholder = "Search",
    empty = "Nothing matches",
    hint,
    hintWhenEmpty,
    minimum = 0,
  }) {
    const modal = sheet(`
      <h3>${escapeHTML(t(title))}</h3>
      ${hint ? `<p class="hint">${escapeHTML(t(hint))}</p>` : ""}
      <input class="dialog-input" placeholder="${escapeHTML(t(placeholder))}" />
      <div class="pick-list multi-list"></div>
      ${hintWhenEmpty ? `<p class="hint dialog-count" hidden>${escapeHTML(t(hintWhenEmpty))}</p>` : ""}
      <div class="modal-row">
        <span class="spacer"></span>
        <button class="ghost dialog-cancel">${escapeHTML(t("Cancel"))}</button>
        <button class="primary dialog-ok">${escapeHTML(t("OK"))}</button>
      </div>`);
    const input = modal.querySelector(".dialog-input");
    const list = modal.querySelector(".multi-list");
    const ok = modal.querySelector(".dialog-ok");
    const count = modal.querySelector(".dialog-count");
    const chosen = new Set(items.filter((item) => item.checked).map((item) => item.value));

    const selected = () => items.filter((item) => chosen.has(item.value)).map((item) => item.value);

    function sync() {
      ok.disabled = chosen.size < minimum;
      if (count) count.hidden = chosen.size > 0;
    }

    function paint() {
      const q = input.value.trim().toLowerCase();
      const shown = q
        ? items.filter((item) => (item.label + " " + (item.sub || "")).toLowerCase().includes(q))
        : items;
      list.innerHTML = "";
      if (!shown.length) {
        const none = document.createElement("p");
        none.className = "hint";
        none.textContent = t(empty);
        list.appendChild(none);
        return;
      }
      for (const item of shown) {
        const row = document.createElement("button");
        row.type = "button";
        row.className = "pick-item multi-item" + (chosen.has(item.value) ? " on" : "");
        row.innerHTML = `<span class="pick-check"></span>${item.colour ? '<span class="pick-dot"></span>' : ""}<span class="pick-text"><b></b><i></i></span>`;
        if (item.colour) row.querySelector(".pick-dot").style.background = item.colour;
        row.querySelector("b").textContent = item.label;
        row.querySelector("i").textContent = item.sub || "";
        row.addEventListener("click", () => {
          if (chosen.has(item.value)) chosen.delete(item.value);
          else chosen.add(item.value);
          row.classList.toggle("on", chosen.has(item.value));
          sync();
        });
        list.appendChild(row);
      }
    }

    return new Promise((resolve) => {
      const done = (value) => {
        closeSheet(modal);
        resolve(value);
      };
      sync();
      paint();
      input.addEventListener("input", paint);
      input.addEventListener("keydown", (event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          done(null);
        } else if (event.key === "Enter" && !ok.disabled) {
          event.preventDefault();
          done(selected());
        }
      });
      ok.addEventListener("click", () => {
        if (!ok.disabled) done(selected());
      });
      modal.querySelector(".dialog-cancel").addEventListener("click", () => done(null));
      modal.addEventListener("mousedown", (event) => {
        if (event.target === modal) done(null);
      });
      modal.addEventListener("keydown", (event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          done(null);
        }
      });
      setTimeout(() => input.focus());
    });
  }

  return { ask, pick, multiPick, menu, confirm, formula, place };
})();
