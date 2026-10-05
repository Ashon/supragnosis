// The settings page (docs/settings-page.md), shown in the main window in place of the viewer. A form
// over the commands the shell answers only for this page (S1); every change is a CLI call made by the
// shell, and what the CLI answered is what is shown (S3). Everything is rendered as text - createElement and textContent only - because what
// is shown includes CLI output that can quote a server's answer (S5).
"use strict";

const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);
const $ = (id) => document.getElementById(id);

let busy = false;

function el(tag, cls, text) {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text !== undefined && text !== null) e.textContent = String(text);
  return e;
}

function say(text, bad) {
  const m = $("message");
  m.textContent = text || "";
  m.classList.toggle("bad", !!bad);
}

// Runs one change: buttons stay disabled until the CLI has answered, and the state is re-read
// afterwards rather than guessed.
async function act(cmd, args, after) {
  if (busy) return;
  busy = true;
  document.body.classList.add("busy");
  for (const b of document.querySelectorAll("button, input")) b.disabled = true;
  say("working...");
  try {
    const out = await invoke(cmd, args);
    say(out || "done");
    if (after) await after(out);
  } catch (e) {
    say(String(e), true);
  } finally {
    busy = false;
    document.body.classList.remove("busy");
    await load();
  }
}

function serverState(check) {
  if (!check || check.answering === undefined) return ["", ""];
  if (check.answering && check.credential === false) return ["answers, but refused the credential", "bad"];
  if (check.answering) return ["answering", "ok"];
  return ["not answering" + (check.detail ? " - " + check.detail : ""), "bad"];
}

function renderServer(server) {
  const host = $("profiles");
  host.replaceChildren();
  if (!server.known) {
    host.append(el("div", "row", "The supragnosis CLI did not list server profiles - update supragnosis-server."));
    return;
  }
  for (const p of server.profiles) {
    const row = el("div", "row");
    const what = el("div", "what");
    const name = el("div", "name", p.label);
    if (p.active) name.append(el("span", "tag", "in use"));
    what.append(name);
    what.append(el("div", "url", p.remote ? p.url : "this Mac's daemon - " + p.url));
    if (p.active) {
      const [text, cls] = serverState(server.check);
      if (text) what.append(el("div", "state " + cls, text));
    }
    row.append(what);
    const buttons = el("div", "buttons");
    if (!p.active) {
      const use = el("button", "primary", "Use");
      use.onclick = () => act("server_use", { name: p.name });
      buttons.append(use);
    }
    if (p.remote) {
      const remove = el("button", "danger", "Remove");
      remove.onclick = () => act("server_remove", { name: p.name });
      buttons.append(remove);
    }
    row.append(buttons);
    host.append(row);
  }
}

// The next step the CLI named for an app ("quit Claude and open it again") stays beside that app
// until the state is read again.
const appNotes = {};

function renderApps(apps) {
  const host = $("appRows");
  host.replaceChildren();
  for (const a of apps) {
    const row = el("div", "row");
    const what = el("div", "what");
    what.append(el("div", "name", a.name));
    what.append(el("div", "state" + (a.state === "connected" ? " ok" : ""), a.state));
    if (appNotes[a.id]) what.append(el("div", "note", appNotes[a.id]));
    row.append(what);
    if (a.action) {
      const b = el("button", a.action === "Disconnect" ? "danger" : "primary", a.action);
      b.onclick = () => act("app_toggle", { id: a.id }, (out) => { appNotes[a.id] = out; });
      row.append(b);
    }
    host.append(row);
  }
}

function renderDaemon(d) {
  const section = $("daemon");
  const remote = d.remote;
  section.classList.toggle("off", !!remote);
  $("daemonLine").textContent = remote ? "" : d.line;
  $("daemonNote").textContent = remote
    ? "AI apps on this Mac use the server \"" + remote + "\", so this Mac's daemon is not what they use. Its controls apply again when you switch back to This Mac."
    : "";
  const login = $("login");
  login.checked = d.login === true;
  login.disabled = !!remote || d.login === null || d.login === undefined;
  $("restart").disabled = !!remote;
  const store = d.store || {};
  const owed = store.owed_projections;
  const recovery = store.last_recovery;
  $("storeLine").textContent =
    (owed === undefined ? "store health unknown" : owed === 0 ? "store healthy - nothing owed" : owed + " projections owed") +
    (recovery ? " - last recovery repaired " + (recovery.observations || 0) + " observations" : "");
}

async function load() {
  if (busy) return;
  try {
    const s = await invoke("settings_state");
    if (!s.cli) say("The supragnosis CLI was not found - install supragnosis-server.", true);
    renderServer(s.server);
    renderApps(s.apps);
    renderDaemon(s.daemon);
    for (const b of document.querySelectorAll("button, input")) {
      if (b.id !== "login" && b.id !== "restart") b.disabled = false;
    }
  } catch (e) {
    say(String(e), true);
  }
}

$("addForm").addEventListener("submit", (ev) => {
  ev.preventDefault();
  const name = $("addName").value.trim();
  const url = $("addUrl").value.trim();
  const credential = $("addCred").value;
  const ca = $("addCa").value.trim() || null;
  const useNow = $("addUse").checked;
  // The field is cleared as soon as the value is handed over, whatever the CLI answers (S2).
  $("addCred").value = "";
  act("server_add", { name, url, credential, ca }, async () => {
    $("addName").value = "";
    $("addUrl").value = "";
    $("addCa").value = "";
    $("addServer").open = false;
    if (useNow) {
      const out = await invoke("server_use", { name });
      say(out);
    }
  });
});

$("login").addEventListener("change", (ev) => {
  act("login_set", { on: ev.target.checked });
});

$("restart").addEventListener("click", () => act("daemon_restart"));

// The viewer's address on this platform: the shell serves it on its own scheme (viz://localhost),
// which Windows' webview reaches as http://viz.localhost.
if (location.protocol !== "tauri:") $("back").href = "http://viz.localhost/";

window.addEventListener("focus", load);
load();
