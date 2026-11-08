// The settings page (docs/settings-page.md), shown in the main window in place of the graph.
//
// A form over the commands the shell answers only for this page (S1). Every change is a CLI call the
// shell makes, and what the CLI answered is what is shown (S3). Everything is built with DOM calls -
// createElement and textContent, never a markup string - because what is shown includes CLI output
// that can quote a server's answer (S5). Icons are Lucide files drawn by CSS (icons/README.md).
"use strict";

const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);


// ---- State -----------------------------------------------------------------------------------

const SECTIONS = [
  { id: "apps", label: t("sec.apps"), icon: "bot", blurb: t("sec.appsBlurb") },
  { id: "daemon", label: t("sec.daemon"), icon: "laptop", blurb: t("sec.daemonBlurb") },
  { id: "sync", label: t("sec.sync"), icon: "network", blurb: t("sec.syncBlurb") },
  { id: "usage", label: t("sec.usage"), icon: "chart", blurb: t("sec.usageBlurb") },
  { id: "about", label: t("sec.about"), icon: "info", blurb: t("sec.aboutBlurb") },
];

let state = null;          // what settings_state last returned
let loadError = null;      // why it could not be read, if it could not
const pending = new Set(); // action keys whose CLI call is running
const appNotes = {};       // app id -> the next step the CLI named for it

// The pane on screen: the one the address names, or the one last opened, which the page keeps in its
// own storage so Cmd+, and the gear reopen it (design-system.md Section 29.14); AI apps otherwise.
const LAST_PANE = "supra.settings.pane";
function current() {
  const id = location.hash.replace("#", "");
  if (SECTIONS.some((s) => s.id === id)) return id;
  let last = null;
  try { last = localStorage.getItem(LAST_PANE); } catch (_) { /* no storage - AI apps */ }
  return SECTIONS.some((s) => s.id === last) ? last : "apps";
}
function keepPane() {
  try { localStorage.setItem(LAST_PANE, current()); } catch (_) { /* lost, the page opens on AI apps */ }
}

// ---- Status summaries ---------------------------------------------------------------------------

function daemonStatus(d) {
  if (!d) return { kind: "", title: "", detail: "" };
  if (d.situation === "conflict") return { kind: "bad", title: t("daemon.conflict"), detail: t("daemon.conflictDetail") };
  // A store indexed by another model keeps the daemon from starting, and a restart fails the same
  // way (docs/settings-page.md Section 3.3): the card says why and holds Rebuild index instead.
  const c = d.embedder_conflict;
  if (c) return { kind: "bad", title: t("daemon.indexConflict"),
    detail: t("daemon.indexConflictDetail", { recorded: c.recorded, daemon: c.daemon }) };
  if (d.running && d.installed && d.running !== d.installed)
    return { kind: "warn", title: t("daemon.drift"), detail: t("daemon.driftDetail", { running: d.running, installed: d.installed }) };
  const owed = d.store && d.store.owed_projections;
  if (d.answering === false || !d.running) {
    if (d.shell === "starting") return { kind: "", title: t("daemon.starting"), detail: "" };
    if (d.shell === "failed") return { kind: "bad", title: t("daemon.couldNotStart"), detail: d.cause || "" };
    return { kind: "bad", title: t("daemon.stopped"), detail: t("daemon.stoppedDetail") };
  }
  if (owed) return { kind: "warn", title: t("daemon.repairing", { n: owed }), detail: [d.running, d.who].filter(Boolean).join(" - ") };
  return { kind: "ok", title: t("daemon.running"), detail: [d.running, d.who].filter(Boolean).join(" - ") };
}

// The AI apps the command line reported, and how many have an entry - on the bridge or not, as the
// tray counts them (design-system.md Section 29.14).
function appsRead() { return !!(state && state.cli !== false && !state.cli_newer); }
function appsWithEntry() { return state.apps.filter((a) => a.kind === "connected" || a.kind === "attention").length; }

// A section's dot appears only when the section needs attention (design-system.md Section 6a): a dot
// on every section said nothing. "ok" is no dot.
function sectionDot(id) {
  const kind = sectionKind(id);
  return kind === "warn" || kind === "bad" ? kind : null;
}
function sectionKind(id) {
  if (!state) return null;
  if (id === "apps") {
    // The dot shows while no app has an entry, a row needs the person, or the apps could not be read
    // (Section 29.14).
    if (!appsRead()) return "warn";
    return appsWithEntry() && !state.apps.some((a) => a.kind === "attention") ? "ok" : "warn";
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
  // One group, so no heading: "General" named the whole list (Section 29.14).
  host.replaceChildren(...sectionItems(now));
}

function sectionItems(now) {
  return SECTIONS.map((s) => {
      const dot = sectionDot(s.id);
      let meta = null;
      // The count says what it counts, the tray's count: every app with an entry; while the apps
      // could not be read there is none (Section 29.14, P5).
      if (s.id === "apps" && state && appsRead()) meta = h("span", { class: "meta", text: t("sec.connectedCount", { n: appsWithEntry() }) });
      return h("a", { class: "navitem", href: "#" + s.id, "aria-current": s.id === now ? "page" : null },
        h("span", { class: "tile" }, icon(s.icon)),
        h("span", { class: "label", text: s.label }),
        meta,
        dot ? h("span", { class: "dot " + dot }, h("span", { class: "sr-only", text: t(dot === "bad" ? "sec.hasProblem" : "sec.needsAttention") })) : null);
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

// Restart, from either of This Mac's buttons. However it went, the page then follows the daemon
// until it answers (watchDaemon): a restart reported as failed can still come back under launchd.
function restartDaemon() {
  return act("restart", "daemon_restart", null, { title: t("daemon.restarted"), failTitle: t("daemon.restartUnfinished") })
    .then(watchDaemon);
}

// Rebuild index (docs/settings-page.md Section 3.3): the app stops the daemon, the CLI resets the
// index, and the daemon that comes back embeds the store again. The page then follows the daemon and
// the re-embedding until both settle (watchDaemon).
function rebuildIndex() {
  return act("rebuild", "search_rebuild", null, { title: t("daemon.rebuilding"), failTitle: t("daemon.notRebuilt") })
    .then(watchDaemon);
}

// Whether the daemon is embedding what has no vector yet - after a rebuild, or what was written
// before the model.
function indexing() {
  const s = state && state.daemon && state.daemon.semantic;
  return !!(s && s.backfill && s.backfill.state === "running");
}

async function act(key, cmd, args, opts) {
  if (pending.size) return;
  pending.add(key);
  render();
  try {
    const out = await invoke(cmd, args);
    const detail = opts && "detail" in opts ? opts.detail : opts && opts.title ? out : null;
    toast("ok", (opts && opts.title) || out || t("page.done"), detail);
    if (opts && opts.after) await opts.after(out);
    return true;
  } catch (e) {
    toast("bad", (opts && opts.failTitle) || t("page.failed"), String(e));
    return false;
  } finally {
    pending.delete(key);
    await load();
  }
}

// The line under an app's name: what the pill does not already say - or, once this visit connected
// it, the next step the CLI named, said once here and not again in the notice (Section 29.6). The
// app's state comes as its entry's kind, and the page says it (docs/i18n.md Section 5).
const APP_STATE = { newer: "app.stateNewer", http: "app.stateHttp", stdio: "app.stateStdio", other: "app.stateOther", unread: "app.stateUnread" };
function appDetail(a) {
  if (appNotes[a.id] && a.kind === "connected") return appNotes[a.id];
  if (a.kind === "connected") return t("app.byBridge");
  return APP_STATE[a.state] ? t(APP_STATE[a.state]) : null;
}
// A Settings row's kind, in the AI app row's words (kit.js).
function appState(a) {
  if (a.kind === "off") return "todo";
  if (a.kind === "absent") return a.state === "absent" ? "absent" : "unavailable";
  return a.kind;
}
const APP_ACTION = { connect: "app.connect", disconnect: "app.disconnect", switch: "app.switch" };

function renderApps() {
  // flatMap: Claude Code's row is two rows, its own and the capture switch's.
  const rows = state.apps.flatMap((a) => {
    const key = "app:" + a.id;
    const end = [];
    if (a.action) {
      const disconnect = a.action === "disconnect";
      end.push(actionButton(key, t(APP_ACTION[a.action]), disconnect ? "unplug" : "plug", disconnect ? "danger" : "primary",
        () => act(key, "app_toggle", { id: a.id }, {
          title: disconnect ? null : t("app.didConnect", { app: APPS[a.id] }), detail: null,
          // The row keeps only the next step, the CLI's, on the answer's second line: the first
          // says "connected", which its pill says already.
          after: (out) => { appNotes[a.id] = disconnect ? null : (String(out).split("\n")[1] || null); },
          failTitle: t(disconnect ? "app.notDisconnected" : "app.notConnected", { app: APPS[a.id] }),
        })));
    }
    // The helper that finishes a connection, after this visit made it (onboarding.md D9).
    if (HELPER[a.id] && appNotes[a.id] && a.kind === "connected") {
      end.unshift(actionButton("helper:" + a.id, HELPER[a.id], a.id === "cursor" ? "open" : "restart", "",
        () => act("helper:" + a.id, "app_helper", { id: a.id }, { failTitle: t("app.byHand") })));
    }
    const detail = appDetail(a);
    const row = appRow({ id: a.id, state: appState(a), sub: detail ? h("div", { class: "sub", text: detail }) : null, end });
    // Claude Code's capture hook, a switch while it is connected through the bridge and the listing
    // says (design-system.md Section 6a); any other row has none, so the switch never connects or
    // replaces anything.
    if (a.id !== "claude-code" || typeof a.capture !== "boolean") return row;
    const input = h("input", {
      type: "checkbox", role: "switch", checked: a.capture, disabled: pending.size > 0,
      "aria-label": CAPTURE.label,
      on: { change: (e) => act("capture", "capture_set", { on: e.target.checked }, { failTitle: t("app.captureNotChanged") }) },
    });
    return [row, h("div", { class: "row sub-row", "data-k": "capture:" + a.id },
      h("div", { class: "body" },
        h("div", { class: "title", text: CAPTURE.label }),
        h("div", { class: "sub", text: CAPTURE.sub })),
      h("label", { class: "switch" }, input, h("span", { class: "track" })))];
  });
  return [
    h("div", { class: "group-title", text: t("app.onThisMac") }),
    h("div", { class: "card" }, rows),
    h("p", { class: "hint", text: t("app.hint") }),
  ];
}

function renderDaemon() {
  const d = state.daemon;
  const st = daemonStatus(d);
  const busy = pending.size > 0;
  const out = [];
  // Version drift is one card at the top, holding the section's only Restart while it shows
  // (design-system.md Section 6a): it was said three times, with a Restart each.
  const drift = d.running && d.installed && d.running !== d.installed;
  const conflict = !!d.embedder_conflict;
  out.push(h("div", { class: "card" },
    h("div", { class: "row hero", "data-k": "daemon" },
      h("span", { class: "lead " + st.kind }, icon("power")),
      h("div", { class: "body" },
        h("div", { class: "title", text: drift ? t("daemon.driftLoad", { installed: d.installed }) : st.title || t("page.unknown") }),
        h("div", { class: "sub mono", text: drift ? t("daemon.runningVersion", { running: d.running }) : st.detail || "" })),
      drift ? h("div", { class: "end" },
        actionButton("restart", t("daemon.restart"), "restart", "primary", () => restartDaemon()))
        : conflict ? h("div", { class: "end" },
          actionButton("rebuild", t("daemon.rebuild"), "restart", "primary", () => rebuildIndex())) : null)));

  const store = d.store || {};
  const owed = store.owed_projections;
  const recovery = store.last_recovery;
  const storeText = owed === undefined || owed === null
    ? t("daemon.storeUnknown")
    : owed === 0 ? t("daemon.upToDate") : t("daemon.repairing", { n: owed });

  const loginInput = h("input", {
    type: "checkbox", role: "switch", checked: d.login === true,
    disabled: d.login === null || d.login === undefined || busy,
    "aria-label": t("daemon.login"),
    on: { change: (e) => act("login", "login_set", { on: e.target.checked }, { title: t(e.target.checked ? "daemon.loginOn" : "daemon.loginOff"), failTitle: t(e.target.checked ? "daemon.loginNotOn" : "daemon.loginNotOff") }) },
  });
  out.push(h("div", { class: "group-title", text: t("daemon.startup") }),
    h("div", { class: "card" },
      h("div", { class: "row" },
        h("div", { class: "body" },
          h("div", { class: "title", text: t("daemon.login") }),
          h("div", { class: "sub", text: d.login === null || d.login === undefined
            ? t("daemon.loginNeedsNewer")
            : t("daemon.loginSub") })),
        h("label", { class: "switch" }, loginInput, h("span", { class: "track" }))),
      // Not drawn while the card above holds the section's act: a row
      // with nothing to press said nothing (design-system.md Section 29.6).
      drift || conflict ? null : h("div", { class: "row" },
        h("div", { class: "body" },
          h("div", { class: "title", text: t("daemon.restartTitle") }),
          h("div", { class: "sub", text: t("daemon.restartSub") })),
        h("div", { class: "end" }, actionButton("restart", t("daemon.restart"), "restart", "", () => restartDaemon())))),
    // What a report needs, and where the daemon writes (design-system.md Section 29.5).
    h("div", { class: "group-title", text: t("daemon.troubleshooting") }),
    h("div", { class: "card" },
      h("div", { class: "row", "data-k": "diagnostics" },
        h("div", { class: "body" },
          h("div", { class: "title", text: t("daemon.diagnostics") }),
          h("div", { class: "sub", text: t("daemon.diagnosticsSub") })),
        h("div", { class: "end" }, h("button", { class: "btn", id: "copyDiagnostics", on: { click: copyDiagnostics } }, t("daemon.copyDiagnostics")))),
      h("div", { class: "row", "data-k": "log" },
        h("div", { class: "body" },
          h("div", { class: "title", text: t("daemon.log") }),
          h("div", { class: "sub mono selectable", text: d.log || "~/.supragnosis/log" })),
        h("div", { class: "end" }, h("button", { class: "btn quiet", on: { click: () => revealFile("log") } }, t("page.showInFinder"))))),
    h("div", { class: "group-title", text: t("daemon.search") }),
    h("div", { class: "card" }, searchRow(d)),
    h("div", { class: "group-title", text: t("daemon.store") }),
    h("div", { class: "card" },
      h("div", { class: "row" },
        h("span", { class: "lead" }, icon("drive")),
        h("div", { class: "body" },
          h("div", { class: "title", text: storeText }),
          recovery ? h("div", { class: "sub", text: t("daemon.recovery", { n: recovery.observations || 0 }) }) : null))));
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
    if (out && out.ok) toast("ok", t("search.on"));
    else if (!stopAsked) toast("bad", t("search.notDownloadedNow"), (out && out.why) || t("search.seeSetup"));
  } catch (e) {
    toast("bad", t("search.notDownloadedNow"), String(e));
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
  try { await invoke("setup_stop", { step: "model" }); toast("ok", t("search.stopped"), t("search.fetchesMissing")); }
  catch (e) { toast("bad", t("search.didNotStop"), String(e)); }
}

function searchRow(d) {
  const s = d.semantic;
  let title, sub, kind;
  if (!s) { title = t("search.keywordOnly"); sub = t(d.answering === false ? "search.knownLater" : "search.noModel"); kind = ""; }
  else if (s.state === "on" && indexing()) {
    const b = s.backfill;
    title = t("search.indexing"); sub = t("search.embedding", { done: b.embedded, all: b.found }); kind = "ok";
  }
  else if (s.state === "on") { title = t("search.byMeaning"); sub = t("search.multilingual", { model: s.model }); kind = "ok"; }
  // The page's own sentence; the CLI's note is Copy Diagnostics' (design-system.md Section 29.6).
  else { title = t("search.notDownloaded"); sub = t("search.untilDownloaded"); kind = "warn"; }
  let end = null, bar = null;
  if (s && s.state === "on" && indexing()) {
    const b = s.backfill, pct = b.found ? Math.round(100 * b.embedded / b.found) : 0;
    bar = h("div", { class: "bar", role: "progressbar", "aria-label": t("search.indexing"),
      "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": String(pct) }, h("span"));
    bar.firstChild.style.width = pct + "%";
  } else if (s && s.state === "on" && d.answering) {
    // Rebuild index, whenever the daemon runs with a model (docs/settings-page.md Section 3.3).
    end = actionButton("rebuild", t("daemon.rebuild"), "restart", "", () => rebuildIndex());
  }
  if (s && s.state !== "on") {
    if (downloading) {
      bar = h("div", { class: "bar" + (modelPct === null ? " busy" : ""), role: "progressbar", "aria-label": t("search.modelDownload"),
        "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": modelPct === null ? null : String(modelPct) }, h("span"));
      if (modelPct !== null) bar.firstChild.style.width = modelPct + "%";
      end = [h("span", { class: "sub", text: modelProgress || t("daemon.starting") }),
        h("button", { class: "btn quiet", on: { click: stopDownload } }, t("search.stop"))];
    } else {
      end = h("button", { class: "btn", on: { click: downloadModel } }, icon("download"), t("search.download"));
    }
  }
  return h("div", { class: "row", "data-k": "model" },
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
    modelProgress = t("search.progress", { done: +m[1], all: +m[2] });
    modelPct = Math.min(100, Math.round((100 * +m[1]) / Math.max(1, +m[2])));
    render();
  }
});

// Sync (docs/settings-page.md Section 3.4): what the viewer's Peers tab showed, and its one act -
// narrowing what an admitted peer may read. Read from this Mac's daemon.
function renderSync() {
  const f = state.federation;
  if (!f) {
    return [h("div", { class: "callout warn" }, icon("warn"),
      h("div", { class: "grow", text: t("sync.unavailable") }))];
  }
  if (f.configured === false) {
    return [h("div", { class: "card" },
      h("div", { class: "row" },
        h("span", { class: "lead" }, icon("network")),
        h("div", { class: "body" },
          h("div", { class: "title", text: t("sync.notSetUp") }),
          h("div", { class: "sub" }, tParts("sync.describeIn", { file: h("code", { text: "~/.supragnosis/supragnosis.toml" }) }))),
        h("div", { class: "end" }, h("button", { class: "btn quiet", on: { click: () => revealFile("config") } }, t("page.showInFinder")))))];
  }
  const out = [];
  const role = t(f.role === "hub" ? "sync.hub" : "sync.spoke");
  out.push(h("div", { class: "group-title", text: t("sec.daemon") }),
    h("div", { class: "card" },
      h("div", { class: "row" },
        h("span", { class: "lead" }, icon("network")),
        h("div", { class: "body" },
          h("div", { class: "title" }, role, h("span", { class: "pill gold", text: t(f.role === "hub" ? "sync.servesPeers" : "sync.syncsToHubs") })),
          h("div", { class: "sub mono selectable", text: f.node_id || "" })))));

  const hubs = f.servers || [];
  if (hubs.length) {
    out.push(h("div", { class: "group-title", text: t("sync.hubs") }),
      h("div", { class: "card" }, hubs.map((hub) => {
        const ws = (hub.workspaces || []).map((w) => {
          const ahead = (w.local_ahead | 0) || (w.hub_ahead | 0);
          return ahead ? t("sync.ahead", { ws: w.workspace, local: w.local_ahead | 0, hub: w.hub_ahead | 0 }) : t("sync.inSync", { ws: w.workspace });
        });
        // The rows the viewer's Peers tab had (docs/design-system.md Section 6): a workspace this Mac
        // shares that the hub does not admit, one the hub admits that this Mac does not share, and
        // whether the hub has answered what it shares - unknown is not "nothing".
        const extra = [
          ...(hub.local_only || []).map((w) => t("sync.notAdmitted", { ws: w })),
          ...(hub.peer_only || []).map((w) => t("sync.notShared", { ws: w })),
          !hub.negotiated_at ? t("sync.sharesUnknown")
            : state.federation.updated_ms ? t("sync.sharesAgreed", { ago: t("page.secondsAgo", { s: Math.max(0, Math.round((state.federation.updated_ms - hub.negotiated_at) / 1000)) }) }) : null,
        ].filter(Boolean);
        return h("div", { class: "row", "data-k": "hub:" + hub.url },
          h("div", { class: "body" },
            h("div", { class: "title" }, String(hub.url || "").replace(/^https?:\/\//, ""),
              hub.healthy
                ? h("span", { class: "pill ok" }, h("span", { class: "dot" }), t("sync.reachable"))
                : h("span", { class: "pill bad" }, h("span", { class: "dot" }), t("sync.unreachable"))),
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
    out.push(h("div", { class: "group-title", text: t(f.role === "hub" ? "sync.peersHub" : "sync.peersSpoke") }));
    out.push(h("div", { class: "card" }, ids.length ? ids.map((id) => {
      const seen = known.find((p) => p.node_id === id);
      const peer = admitted.find((p) => p.node_id === id);
      const ago = seen && f.updated_ms && seen.last_seen_ms ? Math.max(0, Math.round((f.updated_ms - seen.last_seen_ms) / 1000)) : null;
      const checkedIn = seen
        ? t("sync.checkedIn", { action: seen.last_action, ago: ago !== null ? t("page.secondsAgo", { s: ago }) : "", hits: seen.hits | 0,
          release: seen.release ? t("sync.release", { release: seen.release }) : t("sync.releaseUnknown"), protocol: seen.protocol | 0 }).replace("  ", " ")
        : t("sync.notCheckedIn");
      let chips = null;
      if (peer) {
        const shared = peer.shared_workspaces || [];
        chips = h("div", { class: "chips" }, shared.length
          ? shared.map((w) => h("span", { class: "chip" }, w,
              h("button", {
                class: "chip-x", title: tip(t("sync.stopSharingWs", { ws: w })), "aria-label": t("sync.stopSharingWs", { ws: w }),
                disabled: pending.size > 0, on: { click: () => confirmNarrow(peer, w) },
              }, icon("x"))))
          : [h("span", { class: "sub", text: t("sync.readsNothing") })]);
      } else if (f.role === "hub") {
        chips = h("div", { class: "sub", text: t("sync.peerNotAdmitted") });
      }
      return h("div", { class: "row", "data-k": "peer:" + id },
        h("div", { class: "body" },
          h("div", { class: "title mono selectable", text: id }),
          h("div", { class: "sub", text: checkedIn }),
          chips));
    }) : [h("div", { class: "row" }, h("div", { class: "body" }, h("div", { class: "sub", text: t(f.role === "hub" ? "sync.noPeerHub" : "sync.noPeer") })))]));
    if (f.role === "hub") out.push(h("p", { class: "hint", text: t("sync.hint") }));
  }
  return out;
}

function confirmNarrow(peer, ws) {
  const keep = (peer.shared_workspaces || []).filter((w) => w !== ws);
  const stop = h("button", { class: "btn danger solid" }, t("sync.stopSharing"));
  stop.addEventListener("click", async () => {
    closeDialog();
    await act("narrow:" + peer.node_id, "peer_narrow", { node: peer.node_id, keep }, { title: t("sync.stoppedSharing", { ws }) });
  });
  openDialog([
    h("div", { class: "d-head" },
      h("h3", { id: "d-title", text: t("sync.confirmTitle", { ws }) }),
      h("p", { text: t("sync.confirmBody") })),
    h("div", { class: "d-foot" }, h("button", { class: "btn", on: { click: closeDialog } }, t("page.cancel")), stop),
  ], "d-title");
}

function renderAbout() {
  const a = state.about || {};
  const d = state.daemon || {};
  const pairs = [
    [t("about.app"), a.app],
    [t("about.cli"), d.installed || t(a.cli ? "about.unknown" : "about.notFound")],
    [t("about.daemon"), d.running || daemonStatus(d).title],
    [t("about.cliLocation"), a.cli || t("about.notFound")],
    [t("about.data"), a.data],
    [t("about.license"), a.license],
    [t("about.source"), a.source],
  ];
  return [
    h("div", { class: "card kv selectable" }, pairs.flatMap(([k, v]) => [h("div", { class: "k", text: k }), h("div", { class: "v", text: v || "-" })])),
    // The first run's page, again (docs/onboarding.md Section 6): it shows only what is not done.
    h("div", { class: "group-title", text: t("about.setup") }),
    h("div", { class: "card" },
      h("div", { class: "row" },
        h("div", { class: "body" },
          h("div", { class: "title", text: t("about.setup") }),
          h("div", { class: "sub", text: t("about.setupSub") })),
        h("div", { class: "end" }, h("a", { class: "btn", href: withBack("welcome.html") }, t("about.openSetup"))))),
    h("p", { class: "hint", text: t("about.hint") }),
  ];
}

function renderSkeleton() {
  const line = () => h("div", { class: "skeleton sk-line" });
  const row = () => h("div", { class: "row" }, h("div", { class: "skeleton sk-tile" }),
    h("div", { class: "body" }, line(), line()));
  const card = h("div", { class: "card" }, row(), row(), row());
  return [h("div", { class: "group-title", text: " " }), card];
}

// Every draw keeps the person's focus (design-system.md Section 29.5).
function render() { keepFocus(document.body, draw); }
function draw() {
  renderSidebar();
  const id = current();
  // The title bar names the pane on screen (design-system.md Section 6a, U26).
  document.getElementById("pageTitle").textContent = SECTIONS.find((s) => s.id === id).label;
  const pane = document.getElementById("pane");
  // The Usage pane reads again each time it is opened (docs/settings-page.md Section 3.6).
  if (id !== "usage") usage.shown = false;
  let body;
  if (loadError && !state) {
    body = [h("div", { class: "callout bad" }, icon("alert"),
      h("div", { class: "grow" }, t("page.unread"), h("div", { class: "sub selectable", text: loadError })))];
  } else if (!state) {
    body = renderSkeleton();
  } else if (!state.cli) {
    body = [h("div", { class: "callout warn" }, icon("warn"),
      h("div", { class: "grow", text: t("page.noCli") }))];
  } else if (state.cli_newer && (id === "apps" || id === "daemon")) {
    // docs/compatibility.md Section 6: the CLI answered in a schema this app does not read, so these
    // sections are not drawn from a payload whose meaning moved.
    body = [h("div", { class: "callout warn" }, icon("warn"),
      h("div", { class: "grow", text: t("page.cliNewer") }))];
  } else {
    // An explicit switch rather than a lookup by name: the id comes from the URL's hash, and a
    // property lookup would also find what every object has (constructor, __proto__).
    switch (id) {
      case "daemon": body = renderDaemon(); break;
      case "sync": body = renderSync(); break;
      case "usage": body = renderUsage(); break;
      case "about": body = renderAbout(); break;
      default: body = renderApps();
    }
  }
  pane.replaceChildren(h("section", { class: "section", "aria-labelledby": "h-" + id }, sectionHead(id), body));
  const h2 = pane.querySelector("h2");
  if (h2) h2.id = "h-" + id;
}

// ---- Usage (docs/settings-page.md Section 3.6) ------------------------------------------------

// The pane's windows and the ways the calls chart splits (docs/usage.md Section 4.1). The counts are
// the CLI's: the page adds its hours into local days and keeps the five largest series apart.
const USAGE_WINDOWS = [["7d", t("usage.days", { n: 7 })], ["30d", t("usage.days", { n: 30 })], ["90d", t("usage.days", { n: 90 })]];
const USAGE_SPLITS = [["name", t("usage.function")], ["workspace", t("usage.workspace")], ["app", t("usage.app")], ["rw", t("usage.readOrWrite")]];
// `of` is the window the data on screen was read for and `at` when, so the days end where the read
// did and the words name the window the numbers are of.
const usage = { since: "7d", split: "name", data: null, of: null, at: 0, error: null, reading: false, shown: false };

// Reads the window chosen. The last numbers stay on screen, dimmed, until the next arrive; a read
// that fails takes the numbers of another window off the screen.
async function readUsage() {
  const since = usage.since;
  usage.reading = true;
  render();
  try {
    const data = await invoke("usage_read", { since });
    if (since !== usage.since) return;
    Object.assign(usage, { data, of: since, at: Date.now(), error: null });
  } catch (e) {
    if (since !== usage.since) return;
    usage.error = String(e);
    if (usage.of !== since) usage.data = null;
  }
  usage.reading = false;
  render();
}

const DAY = 86400000;
const dayStart = (t) => { const d = new Date(t); d.setHours(0, 0, 0, 0); return d.getTime(); };
const total = (hours, f) => hours.reduce((n, hr) => n + f(hr), 0);
// Counts and days in the page's language (docs/i18n.md Section 4): 1.2K, 12K; 1.2천, 1.2만.
const COMPACT = new Intl.NumberFormat(LANG, { notation: "compact" });
const counted = (n) => COMPACT.format(n);
const DAY_NAME = new Intl.DateTimeFormat(LANG, { month: "short", day: "numeric" });
const dayName = (t) => DAY_NAME.format(t);

// One column per local day from the window's start to the read, oldest first, each with the hours in
// it. The first day starts where the window does, so it can be partial. An hour that starts before
// the first day or after the last (a half-hour time zone) joins the nearest.
function usageDays(data, until) {
  const days = [];
  for (let t = dayStart(data.since); t <= until; t = dayStart(t + DAY * 1.5)) days.push({ t, hours: [] });
  for (const hr of data.by_hour) {
    const day = days.find((d) => d.t === dayStart(hr.t)) || (hr.t < days[0].t ? days[0] : days[days.length - 1]);
    day.hours.push(hr);
  }
  return days;
}

const shownName = (split, n) => n !== "-" ? n : t(split === "workspace" ? "usage.noWorkspace" : "usage.unnamedApp");

// The calls chart's series for a split: the five largest in the window, the rest as Other.
function callSeries(days, split) {
  if (split === "rw") return [
    { name: t("usage.reads"), get: (d) => total(d.hours, (hr) => hr.calls - hr.writes) },
    { name: t("usage.writes"), get: (d) => total(d.hours, (hr) => hr.writes) }];
  const key = { name: "by_name", workspace: "by_workspace", app: "by_app" }[split];
  const sums = new Map();
  for (const d of days) for (const hr of d.hours) for (const [n, c] of hr[key]) sums.set(n, (sums.get(n) || 0) + c);
  const ranked = [...sums].sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1)).map(([n]) => n);
  const top = ranked.slice(0, 5);
  const count = (d, pick) => total(d.hours, (hr) => hr[key].reduce((n, [k, c]) => n + (pick(k) ? c : 0), 0));
  const series = top.map((n) => ({ name: shownName(split, n), get: (d) => count(d, (k) => k === n) }));
  if (ranked.length > top.length) series.push({ name: t("usage.other"), other: true, get: (d) => count(d, (k) => !top.includes(k)) });
  return series;
}

const SVG_NS = "http://www.w3.org/2000/svg";
function svgEl(tag, attrs, ...kids) {
  const el = document.createElementNS(SVG_NS, tag);
  for (const [k, v] of Object.entries(attrs || {})) el.setAttribute(k, String(v));
  el.append(...kids);
  return el;
}
const niceMax = (v) => { const p = 10 ** Math.floor(Math.log10(v)); return [1, 2, 5, 10].map((m) => m * p).find((m) => m >= v); };

// A stacked column per day: columns at most 24 pixels wide with a 2-pixel gap between segments and a
// rounded top, hairline gridlines, a legend, the day's numbers on hover or with the arrow keys, and
// every number again in the table under the chart (the dataviz rules this page follows).
function columnChart(label, days, series) {
  if (days.every((d) => series.every((se) => !se.get(d)))) {
    return h("div", { class: "ucard" }, h("p", { class: "hint", text: t("usage.none") }));
  }
  const W = 640, H = 170, L = 36, T = 8, B = 22, pw = W - L - 6, ph = H - T - B;
  const slots = series.map((se, i) => se.other ? "so" : "s" + (i + 1));
  const sums = days.map((d) => series.reduce((n, se) => n + se.get(d), 0));
  const max = niceMax(Math.max(1, ...sums));
  const slot = pw / days.length, cw = Math.max(2, Math.min(24, slot * 0.66));
  const y = (v) => T + ph - (v / max) * ph;
  const chart = svgEl("svg", { viewBox: `0 0 ${W} ${H}`, class: "uchart", role: "img", tabindex: 0,
    "aria-label": t("usage.chartLabel", { label }) });
  // Counts take whole-number ticks: the middle one only when it is whole.
  for (const v of Number.isInteger(max / 2) ? [0, max / 2, max] : [0, max]) {
    chart.append(svgEl("line", { x1: L, x2: W - 6, y1: y(v), y2: y(v), class: v ? "grid" : "base" }),
      svgEl("text", { x: L - 6, y: y(v) + 4, class: "tick", "text-anchor": "end" }, counted(v)));
  }
  const every = days.length <= 8 ? 1 : days.length <= 32 ? 7 : 14;
  const cols = days.map((d, i) => {
    const x = L + slot * i + (slot - cw) / 2, col = svgEl("g", { class: "col" });
    const parts = series.map((se, k) => ({ v: se.get(d), slot: slots[k] })).filter((p) => p.v > 0);
    let acc = 0, base = y(0);
    parts.forEach((p, j) => {
      acc += p.v;
      const top = y(acc), bottom = j ? base - 2 : base, h = bottom - top;
      if (h > 0.5) {
        const r = j === parts.length - 1 ? Math.min(4, cw / 2, h) : 0;
        col.append(svgEl("path", { class: p.slot, d: `M${x},${bottom}V${top + r}Q${x},${top} ${x + r},${top}H${x + cw - r}Q${x + cw},${top} ${x + cw},${top + r}V${bottom}Z` }));
      }
      base = top;
    });
    if ((days.length - 1 - i) % every === 0) chart.append(svgEl("text", { x: L + slot * (i + 0.5), y: H - 6, class: "tick", "text-anchor": "middle" }, dayName(d.t)));
    chart.append(col);
    return col;
  });
  const tip = h("div", { class: "utip", hidden: true });
  let at = -1;
  const show = (i) => {
    if (at >= 0) cols[at].classList.remove("on");
    at = i;
    cols[i].classList.add("on");
    const d = days[i];
    tip.replaceChildren(h("div", { class: "utip-h" }, h("span", { text: dayName(d.t) }), h("strong", { text: counted(sums[i]) })),
      ...series.map((se, k) => h("div", { class: "utip-r" }, h("span", { class: "key " + slots[k] }), h("strong", { text: counted(se.get(d)) }), h("span", { text: se.name }))));
    const centre = (L + slot * (i + 0.5)) / W;
    tip.style.left = (centre * 100) + "%";
    tip.className = "utip" + (centre < 0.2 ? " from-left" : centre > 0.8 ? " from-right" : "");
    tip.hidden = false;
  };
  const hide = () => { if (at >= 0) cols[at].classList.remove("on"); at = -1; tip.hidden = true; };
  days.forEach((_, i) => {
    const hit = svgEl("rect", { x: L + slot * i, y: T, width: slot, height: ph, class: "hit" });
    hit.addEventListener("pointerenter", () => show(i));
    chart.append(hit);
  });
  chart.addEventListener("pointerleave", hide);
  chart.addEventListener("blur", hide);
  chart.addEventListener("focus", () => show(at >= 0 ? at : days.length - 1));
  chart.addEventListener("keydown", (e) => {
    const step = { ArrowLeft: -1, ArrowRight: 1 }[e.key];
    if (!step) return;
    e.preventDefault();
    show(Math.max(0, Math.min(days.length - 1, (at >= 0 ? at : days.length - 1) + step)));
  });
  const legend = h("div", { class: "ulegend" }, series.map((se, k) => h("span", null, h("span", { class: "key " + slots[k] }), se.name)));
  const table = h("details", { class: "unumbers" }, h("summary", { text: t("usage.numbers") }),
    usageTable([t("usage.day"), ...series.map((se) => se.name), t("usage.total")],
      days.map((d, i) => [dayName(d.t), ...series.map((se) => se.get(d)), sums[i]]).reverse()));
  return h("div", { class: "ucard" }, legend, h("div", { class: "uplot" }, chart, tip), table);
}

// A table whose first `names` columns name and the rest count.
function usageTable(head, rows, names = 1) {
  const cls = (i) => i >= names ? "n" : null;
  return h("table", { class: "utable" },
    h("thead", null, h("tr", null, head.map((c, i) => h("th", { class: cls(i), text: c })))),
    h("tbody", null, rows.map((r) => h("tr", null, r.map((c, i) => h("td", { class: cls(i), text: typeof c === "number" ? counted(c) : c }))))));
}

function renderUsage() {
  if (!usage.shown) { usage.shown = true; queueMicrotask(readUsage); }
  const windows = h("div", { class: "useg", role: "group", "aria-label": t("usage.window") },
    USAGE_WINDOWS.map(([id, label]) => h("button", {
      type: "button", class: "btn" + (usage.since === id ? " on" : ""), "aria-pressed": String(usage.since === id),
      on: { click: () => { if (usage.since !== id) { usage.since = id; readUsage(); } } },
    }, label)));
  const out = [h("div", { class: "ubar" }, windows)];
  const failed = (text) => h("div", { class: "callout bad" }, icon("alert"),
    h("div", { class: "grow" }, t("usage.unread"), h("div", { class: "sub selectable", text })));
  if (!usage.data) return out.concat(usage.error ? [failed(usage.error)] : renderSkeleton());
  if (usage.error) out.push(failed(usage.error));
  const dim = usage.reading || usage.of !== usage.since;
  return out.concat(h("div", { class: "ubody" + (dim ? " reading" : ""), "aria-busy": String(usage.reading) }, usageBody(usage.data)));
}

function usageBody(data) {
  const days = usageDays(data, usage.at), hours = days.flatMap((d) => d.hours), hk = data.hook;
  const since = USAGE_WINDOWS.find(([id]) => id === usage.of)[1];
  if (!data.calls && !hours.length) {
    return h("div", { class: "callout info" }, icon("info"), h("div", { class: "grow",
      text: t("usage.nothing", { since }) }));
  }
  const writes = total(hours, (hr) => hr.writes), missed = total(hours, (hr) => hr.empty + hr.not_found + hr.errors);
  const tile = (label, value, sub) => h("div", { class: "utile" },
    h("div", { class: "ul", text: label }), h("div", { class: "uv", text: value }), h("div", { class: "us", text: sub }));
  const split = h("div", { class: "useg", role: "group", "aria-label": t("usage.splitBy") },
    USAGE_SPLITS.map(([id, label]) => h("button", {
      type: "button", class: "btn" + (usage.split === id ? " on" : ""), "aria-pressed": String(usage.split === id),
      on: { click: () => { usage.split = id; render(); } },
    }, label)));
  const series = (pairs) => pairs.map(([name, get, other]) => ({ name, get: (d) => total(d.hours, get), other }));
  // Recall as the log sees it (docs/recall.md Section 7.1); a CLI from before it says nothing, so
  // its figures read "-" and its charts are not drawn.
  const rc = data.recall || { opens: 0, index_opens: 0, observes: 0, cited: 0, reach: [] };
  return [
    h("div", { class: "utiles" },
      tile(t("usage.calls"), counted(data.calls), t("usage.readsWrites", { reads: counted(data.calls - writes), writes: counted(writes) })),
      tile(t("usage.missed"), data.calls ? Math.round((missed / data.calls) * 100) + "%" : "-", t("usage.missedOf", { missed: counted(missed), calls: counted(data.calls) })),
      tile(t("usage.indexesGiven"), counted(hk.session_indexes + hk.subagent_indexes), t("usage.indexesTo", { sessions: hk.session_indexes, subagents: hk.subagent_indexes })),
      tile(t("usage.commitAnswered"), hk.commit_questions ? t("page.kOfN", { k: hk.answered_with_a_record, n: hk.commit_questions }) : "-", t("usage.commitAnsweredSub")),
      tile(t("usage.fromIndex"), rc.opens ? t("page.kOfN", { k: rc.index_opens, n: rc.opens }) : "-", t("usage.fromIndexSub")),
      tile(t("usage.citing"), rc.observes ? t("page.kOfN", { k: rc.cited, n: rc.observes }) : "-", t("usage.citingSub"))),
    h("div", { class: "group-title uhead" }, h("span", { text: t("usage.calls") }), split),
    columnChart(t("usage.calls"), days, callSeries(days, usage.split)),
    h("div", { class: "group-title", text: t("usage.missed") }),
    columnChart(t("usage.missedCalls"), days, series([
      [t("usage.foundNothing"), (hr) => hr.empty], [t("usage.namedNothing"), (hr) => hr.not_found], [t("usage.failed"), (hr) => hr.errors]])),
    h("div", { class: "group-title", text: t("usage.hookIndexes") }),
    columnChart(t("usage.indexes"), days, series([
      [t("usage.toSessions"), (hr) => hr.session_indexes], [t("usage.toSubagents"), (hr) => hr.subagent_indexes]])),
    h("div", { class: "group-title", text: t("usage.commitQuestions") }),
    columnChart(t("usage.commitQuestions"), days, series([
      [t("usage.answered"), (hr) => hr.answered], [t("usage.notAnswered"), (hr) => hr.commit_questions - hr.answered, true]])),
    data.recall ? [
      h("div", { class: "group-title", text: t("usage.opened") }),
      columnChart(t("usage.opened"), days, series([
        [t("usage.fromIndexLine"), (hr) => hr.index_opens], [t("usage.elsewhere"), (hr) => hr.opens - hr.index_opens, true]])),
      h("div", { class: "group-title", text: t("usage.citing") }),
      columnChart(t("usage.observes"), days, series([
        [t("usage.cite"), (hr) => hr.cited], [t("usage.citeNone"), (hr) => hr.observes - hr.cited, true]])),
    ] : null,
    rc.reach.length ? h("div", { class: "group-title", text: t("usage.reach") }) : null,
    rc.reach.length ? h("div", { class: "ucard" }, usageTable([t("usage.workspace"), t("usage.hoursBack")], rc.reach)) : null,
    h("div", { class: "group-title", text: t("usage.byCall") }),
    h("div", { class: "ucard" }, usageTable([t("usage.function"), t("usage.workspace"), t("usage.calls"), t("usage.foundNothing"), t("usage.namedNothing"), t("usage.failed")],
      data.by_call.map((r) => [r.kind === "tool" ? r.name : r.kind + ":" + r.name, shownName("workspace", r.workspace), r.calls, r.empty, r.not_found, r.errors]), 2)),
    h("div", { class: "group-title", text: t("usage.byApp") }),
    h("div", { class: "ucard" }, usageTable([t("usage.app"), t("usage.calls")], data.by_app.map(([app, n]) => [shownName("app", app), n]))),
  ];
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

// ---- Loading --------------------------------------------------------------------------------

// Diagnostics and Show in Finder (design-system.md Section 29.5).
async function copyDiagnostics() {
  try {
    await invoke("clipboard_copy", { text: await invoke("diagnostics") });
    toast("ok", t("daemon.diagnosticsCopied"));
  } catch (e) { toast("bad", t("daemon.diagnosticsFailed"), String(e)); }
}
async function revealFile(what) {
  try { await invoke("reveal", { what }); } catch (e) { toast("bad", t("page.notShownInFinder"), String(e)); }
}

// What the page last drew from: the reads that come on their own - the window's focus, the watch -
// draw only when what they read changed (Section 29.5). Every other load draws.
let drawnFrom = null;
async function load(quiet) {
  try {
    state = await invoke("settings_state");
    loadError = null;
  } catch (e) {
    loadError = String(e);
  }
  const now = JSON.stringify([state, loadError]);
  if (!quiet || now !== drawnFrom) render();
  drawnFrom = now;
  if (daemonUnsettled()) watchDaemon();
}

// Whether This Mac's daemon is on its way somewhere: not answering, starting, or failed.
function daemonUnsettled() {
  const d = state && state.daemon;
  if (!d) return false;
  return !d.answering || !d.running || d.shell === "starting" || d.shell === "failed" || indexing();
}

// While the daemon is unsettled the page reads its state again every two seconds, for up to a
// minute, so a restart that is still coming up - or one reported as failed while launchd brought the
// daemon back - shows what is true without waiting for the window to be focused again.
let watch = null; // { until } while the page follows the daemon; one watch at a time
function watchDaemon() {
  if (watch) return;
  watch = { until: Date.now() + 60000 };
  const tick = async () => {
    // A load while an act is running would read the act's middle; wait for its own load.
    if (!pending.size) await load(true);
    // An index being embedded moves; the page follows it for as long as it does.
    if (indexing()) watch.until = Math.max(watch.until, Date.now() + 60000);
    if (!daemonUnsettled() || Date.now() > watch.until) { watch = null; return; }
    setTimeout(tick, 2000);
  };
  setTimeout(tick, 2000);
}

window.addEventListener("hashchange", () => { keepPane(); render(); document.getElementById("pane").scrollTop = 0; });
window.addEventListener("focus", () => { if (!pending.size && !document.getElementById("dialogHost").firstChild) load(true); });
// Esc closes a dialog if one is open, and otherwise closes Settings back to the graph.
document.addEventListener("keydown", (e) => {
  if (e.key !== "Escape") return;
  if (document.getElementById("dialogHost").firstChild) closeDialog();
  else location.href = graphBack();
});
document.getElementById("close").href = graphBack();
// Rebuild index from the starting page (docs/settings-page.md Section 3.3): its click opened this page
// with act=rebuild, which starts the rebuild only while the daemon still cannot open its store. The
// parameter is dropped either way, so a reload asks nothing again.
function actAsked() {
  const act = new URLSearchParams(location.search).get("act");
  if (!act) return;
  const keep = new URLSearchParams(location.search);
  keep.delete("act");
  const q = keep.toString();
  history.replaceState(null, "", location.pathname + (q ? "?" + q : "") + location.hash);
  const d = state && state.daemon;
  if (act === "rebuild") { if (d && d.embedder_conflict) rebuildIndex(); return; }
  // The starting page's acts (design-system.md Section 29.5): a restart only while the daemon
  // neither answers nor is being started; a notice says the state otherwise.
  if (act === "restart") {
    if (!d) return;
    if (!d.answering && d.shell !== "starting") restartDaemon();
    else toast("info", t(d.answering ? "daemon.notRestartedAnswers" : "daemon.notRestartedStarting"));
    return;
  }
  if (act === "diagnostics") { const b = document.getElementById("copyDiagnostics"); if (b) b.focus(); return; }
  if (act === "reveal-log") revealFile("log");
}

keepPane();
render();
load().then(adoptDownload).then(actAsked);
