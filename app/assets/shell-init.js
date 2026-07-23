// Injected by the shell before the viewer loads (initialization_script). Three duties, all of
// them desktop-shell concerns kept OUT of the viewer assets (which stay browser-neutral):
//
// 1. SSE facade - the webview's custom protocol cannot stream responses, so
//    EventSource("/api/events") cannot ride the viz:// proxy. The Rust core holds the SSE
//    connection to the daemon's unix socket and re-emits each data frame as a "viz-event" Tauri
//    event; the facade hands those frames to the viewer through the EventSource surface it
//    already uses (viewer.js sets .onmessage only). Any other URL falls through natively.
// 2. Window-chrome integration - the macOS title bar is a transparent overlay (see show_viewer),
//    so the viewer header doubles as the title bar: it becomes the drag region and its left edge
//    is padded clear of the traffic lights.
// 3. Startup health signal - report which page actually loaded (the daemon-served viewer vs the
//    shell's starting splash); the shell's only observable for "the proxy + webview path works".
(function () {
  window.addEventListener("DOMContentLoaded", function () {
    var header = document.querySelector("header");
    if (window.__TAURI__)
      window.__TAURI__.event.emit("shell-page-loaded", {
        title: document.title,
        url: String(location.href),
        // Rendered header height - the ground truth for the traffic-light y in show_viewer
        // (lights center on headerHeight/2; keep the two in agreement when styling shifts it).
        headerHeight: header ? header.offsetHeight : null,
      });

    // App feel: UI chrome is not selectable text (this is an app, not a document). Selection
    // stays enabled only where copying matters - text inputs, the detail inspector, the event
    // log. The header also gets its left padding here, clear of the overlaid traffic lights.
    var style = document.createElement("style");
    style.textContent =
      "html { -webkit-user-select: none; user-select: none; cursor: default; }" +
      " input { -webkit-user-select: auto; user-select: auto; cursor: text; }" +
      " #detail, #log { -webkit-user-select: text; user-select: text; }" +
      // The left padding clears the overlaid traffic lights - except in fullscreen, where
      // macOS hides them (the shell emits shell-fullscreen on transitions) and the title
      // slides back to the header's own 14px padding.
      " header { transition: padding-left 0.15s ease; }" +
      " html:not(.shell-fullscreen) header { padding-left: 84px; }";
    document.head.appendChild(style);
    if (window.__TAURI__)
      window.__TAURI__.event.listen("shell-fullscreen", function (e) {
        document.documentElement.classList.toggle("shell-fullscreen", !!e.payload);
      });

    // Title bar unification: the header is the window drag handle (and double-click zooms, like
    // a real title bar - both need the window permissions in capabilities/default.json). The
    // drag handler only fires when the mousedown target itself carries the attribute, so the
    // header's inputs/buttons stay interactive; the title text is chrome, so it drags too.
    if (header) {
      header.setAttribute("data-tauri-drag-region", "");
      var h1 = header.querySelector("h1");
      if (h1) h1.setAttribute("data-tauri-drag-region", "");
    }
  });

  const Native = window.EventSource;
  window.EventSource = function (url) {
    if (!String(url).includes("/api/events") && Native) return new Native(url);
    const es = {
      onmessage: null,
      _closed: false,
      _unlisten: null,
      close() {
        this._closed = true;
        if (this._unlisten) this._unlisten();
      },
    };
    window.__TAURI__.event
      .listen("viz-event", (e) => {
        if (!es._closed && typeof es.onmessage === "function") es.onmessage({ data: e.payload });
      })
      .then((unlisten) => {
        es._unlisten = unlisten;
        if (es._closed) unlisten();
      });
    return es;
  };
})();
