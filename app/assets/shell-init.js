// Injected by the shell before every page the main window loads (initialization_script) - the
// daemon's viewer, the shell's own splash, and the app's settings page. Five duties, all of them
// desktop-shell concerns kept OUT of the viewer assets (which stay browser-neutral):
//
// 1. SSE facade - the webview's custom protocol cannot stream responses, so
//    EventSource("/api/events") cannot ride the viz:// proxy. The Rust core holds the SSE
//    connection to the daemon's unix socket and re-emits each data frame as a "viz-event" Tauri
//    event; the facade hands those frames to the viewer through the EventSource surface it
//    already uses (viewer.js sets .onmessage only). Any other URL falls through natively.
// 2. Window-chrome integration - the macOS title bar is a transparent overlay (see show_viewer),
//    so a page's header doubles as the title bar: it becomes the drag region, and the shared
//    chrome stylesheet (shell.css) pads it clear of the traffic lights.
// 3. Navigation (docs/settings-page.md Section 3.0) - on the graph, the status bar's gear opens the
//    app's Settings page, which holds what the viewer's own dialog did (its build, a hub's peers).
// 4. Startup health signal - report which page actually loaded (the daemon-served viewer vs the
//    shell's starting splash); the shell's only observable for "the proxy + webview path works".
// 5. The machine's name (docs/design-system.md Section 9, D2) - the viewer calls the machine it runs
//    on "this machine"; in the app on a Mac the shell says "this Mac", the word the app's pages use.
//
// Everything is built with DOM calls - no markup strings - like the settings page (S5).
(function () {
  if (/Mac/.test(navigator.platform || "")) window.supragnosisHere = "this Mac";

  // The two pages' addresses. macOS and Linux reach the app's schemes as scheme://localhost;
  // Windows' webview reaches them as http://<scheme>.localhost.
  var custom = location.protocol === "viz:" || location.protocol === "tauri:";
  var GRAPH = custom ? "viz://localhost/" : "http://viz.localhost/";
  var SETTINGS = custom ? "tauri://localhost/settings.html" : "http://tauri.localhost/settings.html";
  var onSettings = /settings\.html$/.test(location.pathname) &&
    (location.protocol === "tauri:" || location.hostname === "tauri.localhost");

  // The graph's gear - the viewer's settings button, in its status bar - is where settings have
  // always been opened. In the app it opens the app's Settings page, which holds what the viewer's
  // own dialog did. A capturing listener on the document runs before the viewer's own handler on the
  // button, so the viewer needs to know nothing of this.
  function takeOverGear() {
    var gear = document.getElementById("settingsBtn");
    if (!gear) {
      // A daemon whose page no longer has the gear (docs/compatibility.md Section 8): its settings
      // stay the viewer's own, and the page is left as the daemon drew it.
      console.warn("shell: the viewer has no #settingsBtn - Settings opens from the menu (Cmd+,) only");
      return;
    }
    gear.title = "Settings (\u2318,)";
    gear.setAttribute("aria-label", "Settings");
    document.addEventListener("click", function (e) {
      if (!e.target.closest) return;
      // The status bar's sync pill opens the same page at its Sync section - the place sync lives in
      // the app (docs/design-system.md Section 6).
      var to = e.target.closest("#settingsBtn") ? "" : e.target.closest("#syncPill") ? "#sync" : null;
      if (to === null) return;
      // The way back carries the viewer's address, its #review dropped (design-system.md Section
      // 29.4); the app's pages follow it only when it is the viewer's own.
      var here = String(location.href).split("#")[0];
      to = SETTINGS + "?from=" + encodeURIComponent(here) + to;
      e.preventDefault();
      e.stopPropagation();
      location.href = to;
    }, true);
  }

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
    // The splash and the remote notice have no header and no chrome to add.
    if (!header) return;

    // The shared chrome. The settings page links it itself, from the app's origin; the viewer is
    // the daemon's page, so the shell serves the same file on the viewer's origin.
    if (!document.querySelector("link[data-shell]")) {
      var link = document.createElement("link");
      link.rel = "stylesheet";
      link.href = "/__shell/shell.css";
      link.setAttribute("data-shell", "");
      document.head.appendChild(link);
    }
    // macOS hides the traffic lights in full screen, and the header drops the room it keeps for
    // them (shell.css). The shell announces each transition; a page loaded while the window is
    // already full screen - Settings or Setup opened from it, the graph come back to - saw none, so
    // it asks the window too. The first answer lands without the padding's animation, so the bar
    // does not slide on load (.shell-asking).
    var root = document.documentElement;
    var fullscreen = function (on) { root.classList.toggle("shell-fullscreen", !!on); };
    if (window.__TAURI__) {
      window.__TAURI__.event.listen("shell-fullscreen", function (e) { fullscreen(e.payload); });
      root.classList.add("shell-asking");
      var settled = function () {
        requestAnimationFrame(function () { root.classList.remove("shell-asking"); });
      };
      window.__TAURI__.window.getCurrentWindow().isFullscreen()
        .then(function (on) { fullscreen(on); settled(); }, settled);
    }

    // Title bar unification: the header is the window drag handle (and double-click zooms, like
    // a real title bar - both need the window permissions in capabilities/default.json). "deep"
    // drags from anywhere in the header: the bare attribute counts only a press on the header
    // itself, and since the title bar became a Mac toolbar its two groups (.tlead, .ttrail) cover
    // all of it but the padding. Tauri's handler still leaves buttons, links, inputs and
    // interactive roles their own clicks. The search's results open inside the header, so their
    // list opts out.
    header.setAttribute("data-tauri-drag-region", "deep");
    var results = header.querySelector("#searchPop");
    if (results) results.setAttribute("data-tauri-drag-region", "false");
    if (!onSettings) takeOverGear();
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
