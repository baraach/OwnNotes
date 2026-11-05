// Decide once, before the first paint, which platform layer applies.
(() => {
  const ua = navigator.userAgent || "";
  const p = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
  const ios = /iPhone|iPad|iPod/i.test(ua) || (/Macintosh/i.test(ua) && navigator.maxTouchPoints > 1);
  const android = /Android/i.test(ua);
  const win = !ios && !android && (/Windows NT|Win32|Win64|Windows/i.test(ua) || /Win/i.test(p));
  const linux = !ios && !android && !win && (/Linux/i.test(ua) || /Linux/i.test(p));
  const mac = !ios && !android && !win && !linux && (/Macintosh|Mac OS X/i.test(ua) || /Mac/i.test(p));
  document.documentElement.dataset.platform = ios ? "ios" : android ? "android" : mac ? "mac" : win ? "windows" : "linux";
  document.documentElement.dataset.mobile = ios || android ? "true" : "false";
  // Linux always uses solid surfaces; the glass treatment is an Apple look.
  document.documentElement.dataset.glass =
    document.documentElement.dataset.platform === "linux" ? "off" : "on";
  // Only the main desktop window has the clear strip around it. Note windows
  // keep a normal system frame.
  if ((mac || win || linux) && !/[?&]note=/.test(location.search)) {
    document.documentElement.dataset.window = "main";
  }

  // Desktop windows quieten their chrome when they lose focus, as native ones do.
  if (!ios && !android) {
    const root = document.documentElement;
    window.addEventListener("blur", () => root.classList.add("window-inactive"));
    window.addEventListener("focus", () => root.classList.remove("window-inactive"));
  }

  // Android draws the app behind its system bars and does not resize the
  // webview for the keyboard, so the activity reports the real sizes here.
  if (android) {
    const root = document.documentElement;
    window.__applyAndroidInsets = (top, bottom, keyboard) => {
      root.style.setProperty("--android-top", top + "px");
      root.style.setProperty("--android-bottom", bottom + "px");
      root.style.setProperty("--keyboard-inset", keyboard + "px");
      root.classList.toggle("keyboard-open", keyboard > 0);
    };
    try {
      const known = window.OwnNotesAndroid?.insets();
      if (known) window.__applyAndroidInsets(...known.split(",").map(Number));
    } catch {}
  }
})();
