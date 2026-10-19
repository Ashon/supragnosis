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

// A section's dot appears only when the section needs attention (design-system.md Section 6a): a dot
// on every section said nothing. "ok" is no dot.
function sectionDot(id) {
  const kind = sectionKind(id);
  return kind === "warn" || kind === "bad" ? kind : null;
}
function sectionKind(id) {
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
        dot ? h("span", { class: "dot " + dot }, h("span", { class: "sr-only", text: dot === "bad" ? "has a problem" : "needs attention" })) : null);
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
function actionButton(key, label, iconName, cls, run, name) {
  const busy = pending.has(key);
  return h("button", {
    class: "btn " + (cls || "") + (busy ? " busy" : ""),
    disabled: pending.size > 0,
    "aria-label": name || null,
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
    if (p.remote) end.push(actionButton("remove:" + p.name, null, "trash", "quiet iconbtn danger", () => confirmRemove(p), "Remove " + p.label));
    // A list, not a radio group: the active row says Active, every other row has Use, so arrowing down
    // the list cannot switch every AI app's server (design-system.md Section 6a).
    return h("div", { class: "row" },
      h("div", { class: "body" },
        h("div", { class: "title" }, p.label, p.active ? h("span", { class: "pill gold", text: "Active" }) : null),
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
  // Version drift is one card at the top, holding the section's only Restart while it shows
  // (design-system.md Section 6a): it was said three times, with a Restart each.
  const drift = !remote && d.running && d.installed && d.running !== d.installed;
  out.push(h("div", { class: "card" },
    h("div", { class: "row hero" },
      h("span", { class: "lead " + st.kind }, icon("power")),
      h("div", { class: "body" },
        h("div", { class: "title", text: drift ? "Update ready - restart to load " + d.installed : st.title || "Unknown" }),
        h("div", { class: "sub mono", text: drift ? d.running + " is running" : st.detail || "" })),
      drift ? h("div", { class: "end" },
        actionButton("restart", "Restart", "restart", "primary", () => act("restart", "daemon_restart", null, { title: "Daemon restarted" }))) : null)));

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
        h("div", { class: "end" }, remote || drift ? null :
          actionButton("restart", "Restart", "restart", "", () => act("restart", "daemon_restart", null, { title: "Daemon restarted" }))))),
    h("div", { class: "group-title", text: "Search" }),
    h("div", { class: "card" }, searchRow(d, remote)),
    h("div", { class: "group-title", text: "Store" }),
    h("div", { class: "card" },
      h("div", { class: "row" },
        h("span", { class: "lead" }, icon("drive")),
        h("div", { class: "body" },
          h("div", { class: "title", text: storeText }),
          recovery ? h("div", { class: "sub", text: "Last recovery re-projected " + (recovery.observations || 0) + " observations" }) : null))));
  return out;
}

// Search by meaning (docs/settings-page.md Section 3.3): the daemon's report, and the download when
// the model is not here. The download is setup's model step, its progress the lines it streams. It
// runs outside the page's one-act-at-a-time lock - 224 MB takes minutes, and nothing else it would
// race with - so the rest of the page stays usable, and it can be stopped from its row.
let modelProgress = null, modelPct = null, downloading = false, stopAsked = false;

async function downloadModel() {
  if (downloading) return;
  downloading = true; stopAsked = false; modelProgress = null; modelPct = null;
  render();
  try {
    const out = await invoke("setup_run", { step: "model" });
    if (out && out.ok) toast("ok", "Search by meaning is on");
    else if (!stopAsked) toast("bad", "The model did not download", (out && out.why) || "See the details in Setup.");
  } catch (e) {
    toast("bad", "The model did not download", String(e));
  } finally {
    downloading = false;
    await load();
  }
}

// A download started on an earlier visit, or from Setup, is still running: show it as running here
// and wait for it, so a second one is not offered.
async function adoptDownload() {
  if (downloading) return;
  let keys = [];
  try { keys = await invoke("setup_running"); } catch (_) { return; }
  if (!keys.includes("model")) return;
  downloading = true; stopAsked = false;
  render();
  const wait = async () => {
    let still = [];
    try { still = await invoke("setup_running"); } catch (_) { still = []; }
    if (still.includes("model")) { setTimeout(wait, 2000); return; }
    downloading = false;
    await load();
  };
  setTimeout(wait, 2000);
}

async function stopDownload() {
  stopAsked = true;
  try { toast("ok", await invoke("setup_stop", { step: "model" }), "The next download fetches what is missing."); }
  catch (e) { toast("bad", "The download did not stop", String(e)); }
}

function searchRow(d, remote) {
  const s = d.semantic;
  let title, sub, kind;
  if (remote) { title = "Not in use"; sub = "The server in use searches with its own model."; kind = ""; }
  else if (!s) { title = "Keyword only"; sub = d.answering === false ? "Known once the daemon answers." : "This daemon runs without a search model, so search matches words."; kind = ""; }
  else if (s.state === "on") { title = "Search by meaning"; sub = "Multilingual, on this Mac (" + s.model + ")"; kind = "ok"; }
  else { title = "Keyword only - the model is not downloaded"; sub = s.note || "Search matches words until the model (224 MB) is here."; kind = "warn"; }
  let end = null, bar = null;
  if (!remote && s && s.state !== "on") {
    if (downloading) {
      bar = h("div", { class: "bar" + (modelPct === null ? " busy" : ""), role: "progressbar", "aria-label": "Model download",
        "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": modelPct === null ? null : String(modelPct) }, h("span"));
      if (modelPct !== null) bar.firstChild.style.width = modelPct + "%";
      end = [h("span", { class: "sub", text: modelProgress || "Starting..." }),
        h("button", { class: "btn quiet", on: { click: stopDownload } }, "Stop")];
    } else {
      end = h("button", { class: "btn", on: { click: downloadModel } }, icon("download"), "Download");
    }
  }
  return h("div", { class: "row" },
    h("span", { class: "lead " + kind }, icon("search")),
    h("div", { class: "body" }, h("div", { class: "title", text: title }), h("div", { class: "sub", text: sub }), bar),
    h("div", { class: "end" }, end));
}

// The download's progress, from the lines setup streams for it.
if (window.__TAURI__) window.__TAURI__.event.listen("setup-line", (e) => {
  const p = e.payload || {};
  if (p.key !== "model") return;
  let ev = null;
  try { ev = JSON.parse(p.line); } catch (_) { return; }
  const m = ev && /^(\d+) of (\d+) MB/.exec(ev.say || "");
  if (m) {
    modelProgress = m[1] + " of " + m[2] + " MB";
    modelPct = Math.min(100, Math.round((100 * +m[1]) / Math.max(1, +m[2])));
    render();
  }
});

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
        // The rows the viewer's Peers tab had (docs/design-system.md Section 6): a workspace this Mac
        // shares that the hub does not admit, one the hub admits that this Mac does not share, and
        // whether the hub has answered what it shares - unknown is not "nothing".
        const extra = [
          ...(hub.local_only || []).map((w) => w + ": not admitted by the hub"),
          ...(hub.peer_only || []).map((w) => w + ": admitted, not shared from this Mac"),
          !hub.negotiated_at ? "What it shares: unknown - no answer yet"
            : state.federation.updated_ms ? "What it shares: agreed " + Math.max(0, Math.round((state.federation.updated_ms - hub.negotiated_at) / 1000)) + "s ago" : null,
        ].filter(Boolean);
        return h("div", { class: "row" },
          h("div", { class: "body" },
            h("div", { class: "title" }, String(hub.url || "").replace(/^https?:\/\//, ""),
              hub.healthy
                ? h("span", { class: "pill ok" }, h("span", { class: "dot" }), "Reachable")
                : h("span", { class: "pill bad" }, h("span", { class: "dot" }), "Unreachable")),
            hub.version ? h("div", { class: "sub mono", text: "v" + hub.version }) : null,
            ws.map((line) => h("div", { class: "sub", text: line })),
            extra.map((line) => h("div", { class: "sub", text: line }))));
      })));
  }

  // One row per peer (design-system.md Section 6a): who checked in and when, and on a hub what it
  // may read - the two lists named the same peers twice.
  const known = f.known_peers || [];
  const admitted = f.role === "hub" ? (f.admitted || []) : [];
  const ids = [...new Set([...admitted.map((p) => p.node_id), ...known.map((p) => p.node_id)])];
  if (ids.length || f.role === "hub") {
    out.push(h("div", { class: "group-title", text: f.role === "hub" ? "Peers, and what each may read" : "Peers that have checked in" }));
    out.push(h("div", { class: "card" }, ids.length ? ids.map((id) => {
      const seen = known.find((p) => p.node_id === id);
      const peer = admitted.find((p) => p.node_id === id);
      const ago = seen && f.updated_ms && seen.last_seen_ms ? Math.max(0, Math.round((f.updated_ms - seen.last_seen_ms) / 1000)) : null;
      const checkedIn = seen
        ? seen.last_action + (ago !== null ? " " + ago + "s ago" : "") + " (" + (seen.hits | 0) + "), "
          + (seen.release ? "v" + seen.release : "release not known") + ", sync " + (seen.protocol | 0)
        : "has not checked in yet";
      let chips = null;
      if (peer) {
        const shared = peer.shared_workspaces || [];
        chips = h("div", { class: "chips" }, shared.length
          ? shared.map((w) => h("span", { class: "chip" }, w,
              h("button", {
                class: "chip-x", title: "Stop sharing " + w + " with this peer", "aria-label": "Stop sharing " + w,
                disabled: pending.size > 0, on: { click: () => confirmNarrow(peer, w) },
              }, icon("x"))))
          : [h("span", { class: "sub", text: "admitted, may read nothing" })]);
      } else if (f.role === "hub") {
        chips = h("div", { class: "sub", text: "not admitted" });
      }
      return h("div", { class: "row" },
        h("div", { class: "body" },
          h("div", { class: "title mono selectable", text: id }),
          h("div", { class: "sub", text: checkedIn }),
          chips));
    }) : [h("div", { class: "row" }, h("div", { class: "body" }, h("div", { class: "sub", text: f.role === "hub"
      ? "No peer is admitted or has checked in. Admitting one stays in supragnosis.toml." : "No peer has checked in yet." })))]));
    if (f.role === "hub") out.push(h("p", { class: "hint", text: "Removing a grant takes effect at once and is written to supragnosis.toml. It stops future reads; it does not take back what has already synced. Granting a workspace, and admitting or removing a peer, stay in the file." }));
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
    // The first run's page, again (docs/onboarding.md Section 6): it shows only what is not done.
    h("div", { class: "group-title", text: "Setup" }),
    h("div", { class: "card" },
      h("div", { class: "row" },
        h("div", { class: "body" },
          h("div", { class: "title", text: "Setup" }),
          h("div", { class: "sub", text: "The four steps of the first run: what is done, and what is left." })),
        h("div", { class: "end" }, h("a", { class: "btn", href: "welcome.html" }, "Open Setup")))),
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
  } else if (state.cli_newer && (id === "server" || id === "apps" || id === "daemon")) {
    // docs/compatibility.md Section 6: the CLI answered in a schema this app does not read, so these
    // sections are not drawn from a payload whose meaning moved.
    body = [h("div", { class: "callout warn" }, icon("warn"),
      h("div", { class: "grow", text: "This supragnosis-server is newer than the app. Update the app (brew upgrade --cask supragnosis) and reopen Settings." }))];
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

// ---- Dialogs --------------------------------------------------------------------------------

let lastFocus = null, releaseFocus = null;

function openDialog(content, labelId) {
  closeDialog();
  lastFocus = document.activeElement;
  const dialog = h("div", { class: "dialog", role: "dialog", "aria-modal": "true", "aria-labelledby": labelId }, content);
  const backdrop = h("div", { class: "backdrop", on: { mousedown: (e) => { if (e.target === backdrop) closeDialog(); } } }, dialog);
  document.getElementById("dialogHost").replaceChildren(backdrop);
  releaseFocus = holdFocus(dialog);
  const first = dialog.querySelector("input, button.primary, button.solid");
  if (first) first.focus();
}

function closeDialog() {
  if (releaseFocus) releaseFocus();
  releaseFocus = null;
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
    h("div", { class: "err", id: id + "-err", hidden: true }));
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
      const input = el.querySelector("input");
      if (msg) {
        input.setAttribute("aria-invalid", "true");
        input.setAttribute("aria-describedby", el.dataset.field + "-err");
      } else {
        input.removeAttribute("aria-invalid");
        input.removeAttribute("aria-describedby");
      }
    }
    if (Object.keys(errs).length) {
      const first = form.querySelector(".field.invalid input");
      if (first) first.focus();
      return;
    }
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
      error.replaceChildren(icon("alert"), h("div", { class: "grow selectable" },
        h("div", { text: String(e) }),
        h("div", { text: "For safety the credential was cleared. Paste it again." })));
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
load().then(adoptDownload);
