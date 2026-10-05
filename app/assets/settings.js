// The settings page (docs/settings-page.md), shown in the main window in place of the graph.
//
// A form over the commands the shell answers only for this page (S1). Every change is a CLI call the
// shell makes, and what the CLI answered is what is shown (S3). Everything is built with DOM calls -
// createElement and textContent, never a markup string - because what is shown includes CLI output
// that can quote a server's answer (S5). Icons are Lucide files drawn by CSS (icons/README.md).
"use strict";

const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);

// The graph's address: the shell serves the viewer on its own scheme (viz://localhost), which
// Windows' webview reaches as http://viz.localhost.
const GRAPH = location.protocol === "tauri:" ? "viz://localhost/" : "http://viz.localhost/";

// ---- DOM helpers ---------------------------------------------------------------------------

// h("div", {class: "row", on: {click}}, child, "text", ...): an element, its attributes, children.
function h(tag, props, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v === null || v === undefined) continue;
    // false omits a boolean attribute - except spellcheck, whose false has to be said.
    if (v === false && k !== "spellcheck") continue;
    if (k === "class") el.className = v;
    else if (k === "text") el.textContent = String(v);
    else if (k === "on") for (const [ev, fn] of Object.entries(v)) el.addEventListener(ev, fn);
    else if (k in el && typeof el[k] !== "function" && k !== "list") el[k] = v;
    else el.setAttribute(k, v === true ? "" : String(v));
  }
  for (const c of children.flat()) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

const icon = (name, extra) => h("span", { class: "i i-" + name + (extra ? " " + extra : ""), "aria-hidden": "true" });

// ---- State -----------------------------------------------------------------------------------

const SECTIONS = [
  { id: "server", label: "Server", icon: "server", blurb: "The supragnosis server this Mac's AI apps use. A switch applies from each app's next session." },
  { id: "apps", label: "AI apps", icon: "bot", blurb: "How each AI app reaches supragnosis. Every app goes through the bridge, which keeps no copy of the token and follows the server above." },
  { id: "daemon", label: "This Mac", icon: "laptop", blurb: "The supragnosis daemon on this Mac: the store your AI apps use when This Mac is the server." },
  { id: "sync", label: "Sync", icon: "network", blurb: "How this Mac's knowledge travels between supragnosis nodes: the hubs it syncs to and, when it is a hub, what each peer may read." },
  { id: "about", label: "About", icon: "info", blurb: "Versions and where things live." },
];

let state = null;          // what settings_state last returned
let loadError = null;      // why it could not be read, if it could not
const pending = new Set(); // action keys whose CLI call is running
const appNotes = {};       // app id -> the next step the CLI named for it

function current() {
  const id = location.hash.replace("#", "");
  return SECTIONS.some((s) => s.id === id) ? id : "server";
}

// ---- Status summaries ---------------------------------------------------------------------------

function serverStatus(server) {
  const c = server && server.check;
  if (!c || c.answering === undefined) return { kind: "", text: "" };
  if (c.answering && c.credential === false) return { kind: "warn", text: "Credential refused" };
  if (c.answering) return { kind: "ok", text: "Answering" };
  return { kind: "bad", text: "Not answering" };
}

function daemonStatus(d) {
  if (!d) return { kind: "", title: "", detail: "" };
  if (d.remote) return { kind: "", title: "Not in use", detail: "AI apps use the server \"" + d.remote + "\"" };
  if (d.situation === "conflict") return { kind: "bad", title: "Two managers claim the daemon", detail: d.line };
  if (d.running && d.installed && d.running !== d.installed)
    return { kind: "warn", title: "Restart to update", detail: d.running + " is running, " + d.installed + " is installed" };
  const owed = d.store && d.store.owed_projections;
  if (d.answering === false || !d.running) return { kind: "bad", title: "Not running", detail: d.line };
  if (owed) return { kind: "warn", title: "Running, repairing", detail: owed + " projections owed" };
  return { kind: "ok", title: "Running", detail: [d.running, d.who].filter(Boolean).join(" - ") };
}

function sectionDot(id) {
  if (!state) return null;
  if (id === "server") return serverStatus(state.server).kind;
  if (id === "apps") {
    const connected = state.apps.filter((a) => a.kind === "connected").length;
    return connected ? "ok" : "warn";
  }
  if (id === "daemon") return daemonStatus(state.daemon).kind;
  if (id === "sync") {
    const f = state.federation;
    if (!f || f.configured === false) return null;
    return (f.servers || []).some((h) => !h.healthy) ? "bad" : "ok";
  }
  return null;
}

// ---- Rendering --------------------------------------------------------------------------------

function renderSidebar() {
  const host = document.getElementById("sidebar");
  const now = current();
  host.replaceChildren(
    h("div", { class: "group", text: "General" }),
    ...sectionItems(now));
}

function sectionItems(now) {
  return SECTIONS.map((s) => {
      const dot = sectionDot(s.id);
      let meta = null;
      if (s.id === "apps" && state) meta = h("span", { class: "meta", text: state.apps.filter((a) => a.kind === "connected").length });
      return h("a", { class: "navitem", href: "#" + s.id, "aria-current": s.id === now ? "page" : null },
        h("span", { class: "tile" }, icon(s.icon)),
        h("span", { class: "label", text: s.label }),
        meta,
        dot ? h("span", { class: "dot " + dot, "aria-hidden": "true" }) : null);
  });
}

function sectionHead(id) {
  const s = SECTIONS.find((x) => x.id === id);
  return h("div", { class: "section-head" },
    h("div", { class: "tile" }, icon(s.icon)),
    h("div", null, h("h2", { text: s.label }), h("p", { text: s.blurb })));
}

// A button that runs one CLI call: it shows progress while the call runs, and every other action
// waits for it, since two CLI writes at once could race on the same file.
function actionButton(key, label, iconName, cls, run) {
  const busy = pending.has(key);
  return h("button", {
    class: "btn " + (cls || "") + (busy ? " busy" : ""),
    disabled: pending.size > 0,
    on: { click: run },
  }, busy ? icon("spin", "i-spin") : iconName ? icon(iconName) : null, label);
}

async function act(key, cmd, args, opts) {
  if (pending.size) return;
  pending.add(key);
  render();
  try {
    const out = await invoke(cmd, args);
    toast("ok", (opts && opts.title) || out || "Done", opts && opts.title ? out : null);
    if (opts && opts.after) await opts.after(out);
    return true;
  } catch (e) {
    toast("bad", (opts && opts.failTitle) || "That did not work", String(e));
    return false;
  } finally {
    pending.delete(key);
    await load();
  }
}

function renderServer() {
  const server = state.server;
  if (!server.known) {
    return [h("div", { class: "callout warn" }, icon("warn"),
      h("div", { class: "grow", text: "The supragnosis CLI did not list server profiles. Update supragnosis-server to manage them here." }))];
  }
  const status = serverStatus(server);
  const rows = server.profiles.map((p) => {
    const host = p.remote ? p.url : "this Mac's daemon";
    const end = [];
    if (p.active && status.text) end.push(h("span", { class: "pill " + status.kind }, h("span", { class: "dot" }), status.text));
    if (!p.active) end.push(actionButton("use:" + p.name, "Use", null, "", () => act("use:" + p.name, "server_use", { name: p.name }, { title: "Switched to " + p.label })));
    if (p.remote) end.push(actionButton("remove:" + p.name, null, "trash", "quiet iconbtn danger", () => confirmRemove(p)));
    return h("div", { class: "row" },
      h("span", { class: "radio" + (p.active ? " on" : ""), "aria-hidden": "true" }),
      h("div", { class: "body" },
        h("div", { class: "title" }, p.label, p.active ? h("span", { class: "pill gold", text: "In use" }) : null),
        h("div", { class: "sub mono selectable", text: host })),
      h("div", { class: "end" }, end));
  });
  const trouble = [];
  if (status.kind === "warn") trouble.push(h("div", { class: "callout warn" }, icon("key"),
    h("div", { class: "grow", text: "The server answers but refused this Mac's credential. Ask its operator for a new one, then remove this profile and add it again." })));
  if (status.kind === "bad") trouble.push(h("div", { class: "callout bad" }, icon("alert"),
    h("div", { class: "grow" }, "The server in use does not answer. AI apps cannot reach it until it does.",
      server.check && server.check.detail ? h("div", { class: "sub selectable", text: server.check.detail }) : null)));
  return [
    ...trouble,
    h("div", { class: "group-title", text: "Servers" }),
    h("div", { class: "card" }, rows),
    h("div", { class: "row-actions" },
      h("button", { class: "btn", disabled: pending.size > 0, on: { click: openAddServer } }, icon("plus"), "Add server...")),
  ];
}

// What kind of app each is, for its icon: a desktop app, an editor, or a command-line tool.
const APP_KIND = {
  "claude-desktop": "desktop", "claude-code": "terminal", cursor: "editor",
  vscode: "editor", codex: "terminal", gemini: "terminal",
};

// The line under an app's name: what the pill does not already say.
function appDetail(a) {
  if (a.kind === "connected") return "Through the bridge";
  if (a.kind === "off" || a.state === "not installed") return null;
  return a.state.charAt(0).toUpperCase() + a.state.slice(1);
}

function renderApps() {
  const rows = state.apps.map((a) => {
    const pill = {
      connected: h("span", { class: "pill ok" }, h("span", { class: "dot" }), "Connected"),
      attention: h("span", { class: "pill warn" }, h("span", { class: "dot" }), "Needs switching"),
      off: h("span", { class: "pill", text: "Not connected" }),
      absent: h("span", { class: "pill", text: a.state === "not installed" ? "Not installed" : "Unavailable" }),
    }[a.kind];
    const key = "app:" + a.id;
    let button = null;
    if (a.action) {
      const disconnect = a.action === "Disconnect";
      button = actionButton(key, a.action, disconnect ? "unplug" : "plug", disconnect ? "danger" : "primary",
        () => act(key, "app_toggle", { id: a.id }, {
          after: (out) => { appNotes[a.id] = disconnect ? null : out; },
        }));
    }
    const note = appNotes[a.id] && a.kind === "connected"
      ? h("div", { class: "callout info" }, icon("info"), h("div", { class: "grow", text: appNotes[a.id] }))
      : null;
    const detail = appDetail(a);
    return h("div", { class: "row" + (a.kind === "absent" ? " muted" : "") },
      h("span", { class: "lead" }, icon(APP_KIND[a.id] || "desktop")),
      h("div", { class: "body" },
        h("div", { class: "title" }, a.name, pill),
        detail ? h("div", { class: "sub", text: detail }) : null,
        note),
      h("div", { class: "end" }, button));
  });
  return [
    h("div", { class: "group-title", text: "Apps on this Mac" }),
    h("div", { class: "card" }, rows),
    h("p", { class: "hint", text: "Connecting writes one entry into the app's own settings, backed up first. Quit and reopen an app for it to load the change." }),
  ];
}

function renderDaemon() {
  const d = state.daemon;
  const st = daemonStatus(d);
  const remote = !!d.remote;
  const busy = pending.size > 0;
  const out = [];
  if (remote) out.push(h("div", { class: "callout info" }, icon("info"),
    h("div", { class: "grow", text: "AI apps on this Mac use the server \"" + d.remote + "\", so this Mac's daemon is not what they use. Its controls apply again once This Mac is the server." })));
  if (!remote && st.kind === "warn" && d.running !== d.installed) out.push(h("div", { class: "callout warn" }, icon("warn"),
    h("div", { class: "grow", text: d.running + " is running, but " + d.installed + " is installed. Restart the daemon to load the new version." }),
    actionButton("restart", "Restart", "restart", "", () => act("restart", "daemon_restart", null, { title: "Daemon restarted" }))));

  out.push(h("div", { class: "card" },
    h("div", { class: "row hero" },
      h("span", { class: "lead " + st.kind }, icon("power")),
      h("div", { class: "body" },
        h("div", { class: "title", text: st.title || "Unknown" }),
        h("div", { class: "sub mono", text: st.detail || "" })))));

  const store = d.store || {};
  const owed = store.owed_projections;
  const recovery = store.last_recovery;
  const storeText = owed === undefined || owed === null
    ? "Unknown - the CLI did not report it"
    : owed === 0 ? "Healthy - nothing owed" : owed + " projections owed - being repaired";

  const loginInput = h("input", {
    type: "checkbox", role: "switch", checked: d.login === true,
    disabled: remote || d.login === null || d.login === undefined || busy,
    "aria-label": "Start at Login",
    on: { change: (e) => act("login", "login_set", { on: e.target.checked }, { title: e.target.checked ? "Starts at login" : "No longer starts at login" }) },
  });
  out.push(h("div", { class: "group-title", text: "Startup" }),
    h("div", { class: "card" },
      h("div", { class: "row" },
        h("div", { class: "body" },
          h("div", { class: "title", text: "Start at Login" }),
          h("div", { class: "sub", text: d.login === null || d.login === undefined
            ? "Needs a newer supragnosis-server"
            : "Keeps the daemon running whenever you are logged in, so AI apps always find it." })),
        h("label", { class: "switch" }, loginInput, h("span", { class: "track" }))),
      h("div", { class: "row" },
        h("div", { class: "body" },
          h("div", { class: "title", text: "Restart the daemon" }),
          h("div", { class: "sub", text: "Loads an upgraded binary. AI apps reconnect on their own." })),
        h("div", { class: "end" }, remote ? null :
          actionButton("restart", "Restart", "restart", "", () => act("restart", "daemon_restart", null, { title: "Daemon restarted" }))))),
    h("div", { class: "group-title", text: "Store" }),
    h("div", { class: "card" },
      h("div", { class: "row" },
        h("span", { class: "lead" }, icon("drive")),
        h("div", { class: "body" },
          h("div", { class: "title", text: storeText }),
          recovery ? h("div", { class: "sub", text: "Last recovery re-projected " + (recovery.observations || 0) + " observations" }) : null))));
  return out;
}

// Sync (docs/settings-page.md Section 3.4): what the viewer's Peers tab showed, and its one act -
// narrowing what an admitted peer may read. Read from this Mac's daemon; a remote profile has none.
function renderSync() {
  const f = state.federation;
  if (state.daemon && state.daemon.remote) {
    return [h("div", { class: "callout info" }, icon("info"),
      h("div", { class: "grow", text: "Sync is this Mac's daemon's. It shows here again once This Mac is the server." }))];
  }
  if (!f) {
    return [h("div", { class: "callout warn" }, icon("warn"),
      h("div", { class: "grow", text: "Sync status is unavailable - this Mac's daemon did not answer." }))];
  }
  if (f.configured === false) {
    return [h("div", { class: "card" },
      h("div", { class: "row" },
        h("span", { class: "lead" }, icon("network")),
        h("div", { class: "body" },
          h("div", { class: "title", text: "Sync is not set up on this Mac" }),
          h("div", { class: "sub", text: "This Mac keeps its knowledge to itself. To join a hub or host one, describe it in ~/.supragnosis/supragnosis.toml." }))))];
  }
  const out = [];
  const role = f.role === "hub" ? "Hub" : "Spoke";
  out.push(h("div", { class: "group-title", text: "This node" }),
    h("div", { class: "card" },
      h("div", { class: "row" },
        h("span", { class: "lead" }, icon("network")),
        h("div", { class: "body" },
          h("div", { class: "title" }, role, h("span", { class: "pill gold", text: f.role === "hub" ? "Serves peers" : "Syncs to hubs" })),
          h("div", { class: "sub mono selectable", text: f.node_id || "" })))));

  const hubs = f.servers || [];
  if (hubs.length) {
    out.push(h("div", { class: "group-title", text: "Hubs this Mac syncs to" }),
      h("div", { class: "card" }, hubs.map((hub) => {
        const ws = (hub.workspaces || []).map((w) => {
          const ahead = (w.local_ahead | 0) || (w.hub_ahead | 0);
          return w.workspace + ": " + (ahead ? "this Mac +" + (w.local_ahead | 0) + ", hub +" + (w.hub_ahead | 0) : "in sync");
        });
        return h("div", { class: "row" },
          h("div", { class: "body" },
            h("div", { class: "title" }, String(hub.url || "").replace(/^https?:\/\//, ""),
              hub.healthy
                ? h("span", { class: "pill ok" }, h("span", { class: "dot" }), "Reachable")
                : h("span", { class: "pill bad" }, h("span", { class: "dot" }), "Unreachable")),
            hub.version ? h("div", { class: "sub mono", text: "v" + hub.version }) : null,
            ws.map((line) => h("div", { class: "sub", text: line }))));
      })));
  }

  if (f.role === "hub") {
    const admitted = f.admitted || [];
    out.push(h("div", { class: "group-title", text: "Peers, and what each may read" }));
    if (!admitted.length) {
      out.push(h("div", { class: "card" }, h("div", { class: "row" },
        h("div", { class: "body" }, h("div", { class: "sub", text: "No peer is admitted. Admitting one stays in supragnosis.toml." })))));
    } else {
      out.push(h("div", { class: "card" }, admitted.map((peer) => {
        const shared = peer.shared_workspaces || [];
        const chips = shared.length
          ? shared.map((w) => h("span", { class: "chip" }, w,
              h("button", {
                class: "chip-x", title: "Stop sharing " + w + " with this peer", "aria-label": "Stop sharing " + w,
                disabled: pending.size > 0, on: { click: () => confirmNarrow(peer, w) },
              }, icon("x"))))
          : [h("span", { class: "sub", text: "admitted, may read nothing" })];
        return h("div", { class: "row" },
          h("div", { class: "body" },
            h("div", { class: "title mono selectable", text: peer.node_id }),
            h("div", { class: "chips" }, chips)));
      })));
    }
    out.push(h("p", { class: "hint", text: "Removing a grant takes effect at once and is written to supragnosis.toml. It stops future reads; it does not recall what has already synced. Granting a workspace, and admitting or removing a peer, stay in the file." }));
  }
  return out;
}

function confirmNarrow(peer, ws) {
  const keep = (peer.shared_workspaces || []).filter((w) => w !== ws);
  const stop = h("button", { class: "btn danger solid" }, "Stop sharing");
  stop.addEventListener("click", async () => {
    closeDialog();
    await act("narrow:" + peer.node_id, "peer_narrow", { node: peer.node_id, keep }, { title: "Stopped sharing " + ws });
  });
  openDialog([
    h("div", { class: "d-head" },
      h("h3", { id: "d-title", text: "Stop sharing \"" + ws + "\" with this peer?" }),
      h("p", { text: "It takes effect at once and is written to supragnosis.toml. The peer keeps what it has already synced; it reads nothing new from this workspace." })),
    h("div", { class: "d-foot" }, h("button", { class: "btn", on: { click: closeDialog } }, "Cancel"), stop),
  ], "d-title");
}

function renderAbout() {
  const a = state.about || {};
  const d = state.daemon || {};
  const pairs = [
    ["App", a.app],
    ["CLI", d.installed || (a.cli ? "unknown" : "not found")],
    ["Daemon", d.running || (d.remote ? "not in use" : "not running")],
    ["CLI location", a.cli || "not found"],
    ["Data", a.data],
    ["License", a.license],
    ["Source", a.source],
  ];
  return [
    h("div", { class: "card kv selectable" }, pairs.flatMap(([k, v]) => [h("div", { class: "k", text: k }), h("div", { class: "v", text: v || "-" })])),
    h("p", { class: "hint", text: "Dependency licences are in the manifest and lockfile at the source above, which cannot disagree with the build. Icons by Lucide (ISC)." }),
  ];
}

function renderSkeleton() {
  const line = () => h("div", { class: "skeleton sk-line" });
  const row = () => h("div", { class: "row" }, h("div", { class: "skeleton sk-tile" }),
    h("div", { class: "body" }, line(), line()));
  const card = h("div", { class: "card" }, row(), row(), row());
  return [h("div", { class: "group-title", text: " " }), card];
}

function render() {
  renderSidebar();
  const id = current();
  const pane = document.getElementById("pane");
  let body;
  if (loadError && !state) {
    body = [h("div", { class: "callout bad" }, icon("alert"),
      h("div", { class: "grow" }, "The settings could not be read.", h("div", { class: "sub selectable", text: loadError })))];
  } else if (!state) {
    body = renderSkeleton();
  } else if (!state.cli) {
    body = [h("div", { class: "callout warn" }, icon("warn"),
      h("div", { class: "grow", text: "The supragnosis CLI was not found. Install supragnosis-server (brew install supragnosis-server) and reopen Settings." }))];
  } else {
    // An explicit switch rather than a lookup by name: the id comes from the URL's hash, and a
    // property lookup would also find what every object has (constructor, __proto__).
    switch (id) {
      case "apps": body = renderApps(); break;
      case "daemon": body = renderDaemon(); break;
      case "sync": body = renderSync(); break;
      case "about": body = renderAbout(); break;
      default: body = renderServer();
    }
  }
  pane.replaceChildren(h("section", { class: "section", "aria-labelledby": "h-" + id }, sectionHead(id), body));
  const h2 = pane.querySelector("h2");
  if (h2) h2.id = "h-" + id;
}

// ---- Notices --------------------------------------------------------------------------------

function toast(kind, title, detail) {
  const host = document.getElementById("toasts");
  const close = h("button", { class: "btn quiet iconbtn", "aria-label": "Dismiss" }, icon("x"));
  const el = h("div", { class: "toast " + kind },
    icon(kind === "ok" ? "ok" : "alert"),
    h("div", { class: "grow" }, h("div", { class: "t-title", text: title }), detail ? h("div", { class: "t-detail", text: detail }) : null),
    close);
  close.addEventListener("click", () => el.remove());
  host.prepend(el);
  // A success needs no reading twice; an error stays until it is dismissed.
  if (kind === "ok") setTimeout(() => el.remove(), 4500);
  while (host.children.length > 4) host.lastChild.remove();
}

// ---- Dialogs --------------------------------------------------------------------------------

let lastFocus = null;

function openDialog(content, labelId) {
  closeDialog();
  lastFocus = document.activeElement;
  const dialog = h("div", { class: "dialog", role: "dialog", "aria-modal": "true", "aria-labelledby": labelId }, content);
  const backdrop = h("div", { class: "backdrop", on: { mousedown: (e) => { if (e.target === backdrop) closeDialog(); } } }, dialog);
  document.getElementById("dialogHost").replaceChildren(backdrop);
  const first = dialog.querySelector("input, button.primary, button.solid");
  if (first) first.focus();
}

function closeDialog() {
  document.getElementById("dialogHost").replaceChildren();
  if (lastFocus && document.contains(lastFocus)) lastFocus.focus();
  lastFocus = null;
}

function confirmRemove(p) {
  const remove = h("button", { class: "btn danger solid" }, icon("trash"), "Remove");
  remove.addEventListener("click", async () => {
    closeDialog();
    await act("remove:" + p.name, "server_remove", { name: p.name }, { title: "Removed " + p.label });
  });
  openDialog([
    h("div", { class: "d-head" },
      h("h3", { id: "d-title", text: "Remove \"" + p.label + "\"?" }),
      h("p", { text: "Its credential is deleted from this Mac." + (p.active ? " AI apps go back to This Mac from their next session." : "") })),
    h("div", { class: "d-foot" }, h("button", { class: "btn", on: { click: closeDialog } }, "Cancel"), remove),
  ], "d-title");
}

// The shape of what was typed, checked before the CLI sees it. The CLI still decides: anything it
// refuses comes back as the dialog's error.
function validate(f, names) {
  const errs = {};
  if (!/^[A-Za-z0-9_-]{1,64}$/.test(f.name)) errs.name = "Use letters, digits, - or _ (up to 64).";
  else if (f.name === "local") errs.name = "\"local\" is This Mac.";
  else if (names.includes(f.name)) errs.name = "A server with this name exists.";
  let u = null;
  try { u = new URL(f.url); } catch (_) { errs.url = "Enter the full address, starting with https://"; }
  if (u) {
    const loop = ["localhost", "127.0.0.1", "[::1]"].includes(u.hostname);
    if (u.protocol === "http:" && !loop) errs.url = "Plain http would send the credential unencrypted. Use https://";
    else if (u.protocol !== "https:" && u.protocol !== "http:") errs.url = "Use an https:// address.";
  }
  if (!f.credential.trim()) errs.credential = "Paste the credential the server's operator gave you.";
  return errs;
}

function field(id, label, input, help, optional) {
  return h("div", { class: "field", "data-field": id },
    h("label", { for: id }, label, optional ? h("span", { class: "opt", text: "optional" }) : null),
    h("div", { class: "input" }, input),
    help ? h("div", { class: "help", text: help }) : null,
    h("div", { class: "err", hidden: true }));
}

function openAddServer() {
  const name = h("input", { id: "f-name", placeholder: "home", spellcheck: false, autocomplete: "off" });
  const url = h("input", { id: "f-url", placeholder: "https://hub.example:7420", spellcheck: false, autocomplete: "off" });
  const cred = h("input", { id: "f-cred", type: "password", class: "with-reveal", spellcheck: false, autocomplete: "off" });
  const reveal = h("button", { type: "button", class: "btn quiet iconbtn reveal", "aria-label": "Show credential" }, icon("eye"));
  reveal.addEventListener("click", () => {
    const show = cred.type === "password";
    cred.type = show ? "text" : "password";
    reveal.replaceChildren(icon(show ? "eye-off" : "eye"));
    reveal.setAttribute("aria-label", show ? "Hide credential" : "Show credential");
  });
  const ca = h("input", { id: "f-ca", placeholder: "/path/to/ca.pem", spellcheck: false, autocomplete: "off" });
  const useNow = h("input", { type: "checkbox", checked: true });
  const error = h("div", { class: "callout bad", hidden: true });
  const submit = h("button", { class: "btn primary", type: "submit" }, "Add server");
  const credField = field("f-cred", "Credential", cred, "Stored in a file only you can read. It is not shown again.");
  credField.querySelector(".input").append(reveal);

  const form = h("form", { novalidate: true },
    h("div", { class: "d-head" },
      h("h3", { id: "d-title", text: "Add a server" }),
      h("p", { text: "A supragnosis server another machine runs. Its operator gives you the address and a credential." })),
    h("div", { class: "d-body" },
      error,
      field("f-name", "Name", name, "What this Mac calls it."),
      field("f-url", "Address", url, "The server's https:// address. /mcp is added if you leave the path out."),
      credField,
      field("f-ca", "CA bundle", ca, "Only for a server whose certificate a private CA issued.", true),
      h("label", { class: "check" }, useNow, "Use this server now")),
    h("div", { class: "d-foot" }, h("button", { type: "button", class: "btn", on: { click: closeDialog } }, "Cancel"), submit));

  form.addEventListener("submit", async (ev) => {
    ev.preventDefault();
    const f = { name: name.value.trim(), url: url.value.trim(), credential: cred.value, ca: ca.value.trim() || null };
    const names = state.server.profiles.map((p) => p.name);
    const errs = validate(f, names);
    for (const el of form.querySelectorAll(".field")) {
      const key = { "f-name": "name", "f-url": "url", "f-cred": "credential" }[el.dataset.field];
      const msg = key && errs[key];
      el.classList.toggle("invalid", !!msg);
      const err = el.querySelector(".err");
      err.hidden = !msg;
      err.textContent = msg || "";
    }
    if (Object.keys(errs).length) return;
    // The credential leaves the field the moment it is handed over, whatever the CLI answers (S2).
    cred.value = "";
    submit.disabled = true;
    submit.replaceChildren(icon("spin", "i-spin"), "Adding...");
    submit.classList.add("busy");
    error.hidden = true;
    try {
      await invoke("server_add", { name: f.name, url: f.url, credential: f.credential, ca: f.ca });
      closeDialog();
      if (useNow.checked) {
        await act("use:" + f.name, "server_use", { name: f.name }, { title: "Added and switched to " + f.name });
      } else {
        toast("ok", "Added " + f.name, "Switch to it from the list when you are ready.");
        await load();
      }
    } catch (e) {
      error.replaceChildren(icon("alert"), h("div", { class: "grow selectable", text: String(e) }));
      error.hidden = false;
      submit.disabled = false;
      submit.classList.remove("busy");
      submit.replaceChildren("Add server");
      cred.focus();
    }
  });
  openDialog(form, "d-title");
}

// ---- Loading --------------------------------------------------------------------------------

async function load() {
  try {
    state = await invoke("settings_state");
    loadError = null;
  } catch (e) {
    loadError = String(e);
  }
  render();
}

window.addEventListener("hashchange", () => { render(); document.getElementById("pane").scrollTop = 0; });
window.addEventListener("focus", () => { if (!pending.size && !document.getElementById("dialogHost").firstChild) load(); });
// Esc closes a dialog if one is open, and otherwise closes Settings back to the graph.
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (document.getElementById("dialogHost").firstChild) closeDialog();
  else location.href = GRAPH;
});
document.getElementById("close").href = GRAPH;

render();
load();
