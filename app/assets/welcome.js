// The Welcome page (docs/onboarding.md Section 6): the first run, step by step.
//
// Every state this page draws is an event `supragnosis setup --events` wrote (O4): the checklist
// from `setup --check`, and while a step runs, the lines it streams. The page holds only the plan -
// what the person has ticked - and the log of what ran. Its acts are setup's steps, the app's two
// helpers, and the choices the app remembers. Built with DOM calls only (kit.js, S5).
"use strict";

const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);
const listen = (ev, fn) => window.__TAURI__.event.listen(ev, fn);
const GRAPH = location.protocol === "tauri:" ? "viz://localhost/" : "http://viz.localhost/";

// The helper an app has once this page connected it (Section 4.2, D9).
const HELPER = { "claude-desktop": "Restart Claude Desktop", cursor: "Open Cursor" };

const STEPS = [
  { step: "server", title: "Keep it running", log: "server" },
  { step: "apps", title: "Connect your AI apps", log: "apps" },
  { step: "model", title: "Search by meaning", log: "model" },
  { step: "first", title: "Try it", log: "first" },
];
const PILL = {
  done: ["ok", "Done"], todo: ["", "Not done"], working: ["gold", "Working"],
  attention: ["warn", "Needs you"], failed: ["bad", "Failed"], skipped: ["", "Not needed here"],
};
// The last step's work is the person's: while it waits, it says so rather than "Working".
const FIRST_PILL = Object.assign({}, PILL, { working: ["gold", "Waiting for an entry"] });
const APP_PILL = {
  done: ["ok", "Connected"], todo: ["", "Not connected"], working: ["gold", "Connecting"],
  attention: ["warn", "Needs you"], failed: ["bad", "Failed"], skipped: ["", "Not installed"],
};

let events = [];              // the last checklist
let info = null;              // welcome_state: what the app remembers, and its facts
let loadError = null;         // why the checklist could not be read
let applied = false;          // whether the plan was applied, or there was none to apply
// The plan Set up applies (onboarding.md Section 6). Replacing what the person set up - an app's
// other supragnosis entry, a daemon someone else started - is in the plan too, and starts unchecked
// (O8): nothing changes until Set up, and then only what was ticked.
const plan = { login: true, apps: {}, capture: true, model: true, replace: {}, takeover: false };
let planTotal = 0, planDone = 0;   // how many of the plan's acts Set up started, and how many ended
const running = {};           // run key -> true while its CLI call runs
const live = {};              // step key -> the last event a running step streamed
const logs = { server: [], apps: [], model: [], first: [] };
const drawers = {};           // step -> whether its log drawer is open
const connectedHere = new Set();

// ---- Reading the checklist ---------------------------------------------------------------------

function stepEvent(step) { return events.find((e) => e.step === step && (step === "first" || !("id" in e))); }
function appEvents() { return events.filter((e) => e.step === "app"); }
function stateOf(step) {
  const l = live[step];
  if (l && running[runKeyOf(step)]) return l.state;
  const e = stepEvent(step);
  return e ? e.state : null;
}
function runKeyOf(step) { return step === "server" && running.takeover ? "takeover" : step; }
function busy() { return Object.keys(running).length > 0; }
function todoApps() { return appEvents().filter((e) => e.state === "todo" && APPS[e.id]).map((e) => e.id); }
function replaceApps() { return appEvents().filter((e) => e.state === "attention" && /--replace/.test(e.say) && APPS[e.id]).map((e) => e.id); }
function canTakeOver() { const e = stepEvent("server"); return !!(e && e.state === "attention" && /--take-over/.test(e.say)); }
function needsPlan() {
  return ["server", "model"].some((s) => stepEvent(s) && stepEvent(s).state === "todo") || todoApps().length > 0
    || replaceApps().length > 0 || canTakeOver();
}

async function check() {
  try {
    const r = await invoke("setup_check");
    events = r.events || [];
    loadError = null;
  } catch (e) {
    loadError = String(e);
  }
  if (!applied && events.length && !needsPlan()) applied = true;
  for (const id of todoApps()) if (!(id in plan.apps)) plan.apps[id] = true;
  // A check that changed nothing draws nothing (Section 29.5).
  const now = JSON.stringify([events, loadError, applied]);
  if (now === checkedFrom) return;
  checkedFrom = now;
  render();
}
let checkedFrom = null;

async function loadInfo() {
  try {
    info = await invoke("welcome_state");
    if (info.prefs && info.prefs.login_off) plan.login = false;
  } catch (e) { loadError = String(e); }
}

// ---- Logs ------------------------------------------------------------------------------------------

function logFor(key) {
  if (key === "server" || key === "takeover") return logs.server;
  if (key.startsWith("app=")) return logs.apps;
  return logs[key] || logs.first;
}

// A line a running step wrote: one of setup's events, passed on as they come, or plain output.
listen("setup-line", (e) => {
  const { key, line, err } = e.payload || {};
  if (!key) return;
  const log = logFor(key);
  let ev = null;
  try { ev = JSON.parse(line); } catch (_) { /* plain output */ }
  if (ev && ev.schema !== undefined && !ev.step) return;
  // A line of output is added to its open drawer in place, with no draw, so focus and a screen
  // reader stay where they are; an event that changes what a card says draws (Section 29.5).
  let entry, changed = false;
  if (ev && ev.step) {
    entry = ev.cmd ? { kind: "cmd", text: ev.cmd } : ev.out ? { kind: "out", text: ev.out } : { kind: "event", text: ev.state + ": " + ev.say };
    const at = ev.step === "app" ? "app=" + ev.id : ev.step;
    const was = live[at];
    changed = !was || was.state !== ev.state || was.say !== ev.say;
    live[at] = ev;
  } else {
    entry = { kind: err ? "err" : "out", text: line };
  }
  log.push(entry);
  if (entry.kind === "event" || changed) { renderSoon(); return; }
  const name = Object.keys(logs).find((k) => logs[k] === log);
  const open = name && document.querySelector('.drawer[data-log="' + name + '"]');
  if (open) {
    const none = open.querySelector(".l-none");
    if (none) none.remove();
    open.append(h("div", { class: "l-" + entry.kind, text: entry.text }));
  }
  // The foot's Copy Diagnostics appears with the first line that ran.
  if (log.length === 1) renderSoon();
});

// ---- Acts ------------------------------------------------------------------------------------------

async function run(key, opts) {
  if (running[key]) return;
  running[key] = true;
  delete live[key.startsWith("app=") ? key : key === "takeover" ? "server" : key];
  render();
  const log = logFor(key);
  try {
    const out = await invoke("setup_run", { step: key, noCapture: !!(opts && opts.noCapture) });
    log.push({ kind: "exit", text: out.ok ? "finished" : "failed (exit " + out.code + ")" + (out.why ? ": " + out.why : "") });
    if (out.ok && key.startsWith("app=")) connectedHere.add(key.slice(4));
    if (!out.ok) toast("bad", (opts && opts.failTitle) || "That step did not finish", out.why || "See its details.");
    announce(stepName(key) + (out.ok ? ": done" : ": failed"));
  } catch (e) {
    log.push({ kind: "err", text: String(e) });
    if (/already running/.test(String(e))) {
      // Started on an earlier visit and still going: say so, and wait for it (adoptRunning).
      toast("ok", stepName(key) + " is already running");
      adoptRunning();
    } else {
      toast("bad", (opts && opts.failTitle) || "That step did not run", String(e));
      announce(stepName(key) + ": failed");
    }
  } finally {
    delete running[key];
    if (planTotal) planDone += 1;
    await check();
  }
}

// Rebuild index (docs/onboarding.md Section 4.1, O4): the command Settings > This Mac runs - it stops
// the daemon, resets the index and starts the daemon again - and then setup is asked again.
async function rebuild() {
  if (running.rebuild) return;
  running.rebuild = true;
  render();
  try {
    toast("ok", "Rebuilding the search index", await invoke("search_rebuild"));
    announce("Rebuild index: done");
  } catch (e) {
    toast("bad", "The index was not rebuilt", String(e));
    announce("Rebuild index: failed");
  } finally {
    delete running.rebuild;
    await check();
  }
}

// What a step is called when its end is said: its card's title, or the app it connects.
function stepName(key) {
  if (key.startsWith("app=")) return "Connect " + (APPS[key.slice(4)] || key.slice(4));
  const s = STEPS.find((x) => x.step === (key === "takeover" ? "server" : key));
  return s ? s.title : key;
}

// A step's end, in the live region a screen reader reads (design-system.md Section 10). Emptied first,
// so the same sentence twice is read twice.
function announce(text) {
  const el = document.getElementById("announce");
  if (!el) return;
  el.textContent = "";
  setTimeout(() => { el.textContent = text; }, 50);
}

// "Set up": the plan, applied in the order the steps depend on - the download first, since nothing
// waits for it (O5), then keeping the daemon running, then each ticked app.
async function applyPlan() {
  applied = true;
  const server = stepEvent("server"), model = stepEvent("model");
  planDone = 0;
  const replacing = replaceApps().filter((id) => plan.replace[id]);
  const takingOver = canTakeOver() && plan.takeover;
  planTotal = (model && model.state === "todo" && plan.model ? 1 : 0)
    + (server && server.state === "todo" && plan.login ? 1 : 0) + (takingOver ? 1 : 0)
    + todoApps().filter((id) => plan.apps[id] !== false).length + replacing.length;
  render();
  if (server && server.state === "todo" && !plan.login) {
    try { info.prefs = await invoke("welcome_set", { loginOff: true }); } catch (_) { /* remembered next time */ }
  }
  if (model && model.state === "todo" && plan.model) run("model", { failTitle: "The model did not download" });
  if (server && server.state === "todo" && plan.login) await run("server", { failTitle: "The daemon is not kept running" });
  if (takingOver) await run("takeover", { failTitle: "The daemon was not taken over" });
  for (const id of todoApps()) {
    if (plan.apps[id] === false) continue;
    await run("app=" + id, { noCapture: id === "claude-code" && !plan.capture, failTitle: APPS[id] + " was not connected" });
  }
  for (const id of replacing) {
    await switchToBridge(id);
    if (planTotal) planDone += 1;
  }
}

async function helper(id) {
  const key = "helper:" + id;
  running[key] = true;
  render();
  try {
    const out = await invoke("app_helper", { id });
    logs.apps.push({ kind: "out", text: out });
    toast("ok", out);
  } catch (e) {
    logs.apps.push({ kind: "err", text: String(e) });
    toast("bad", "Do it by hand", String(e));
  } finally {
    delete running[key];
    render();
  }
}

async function switchToBridge(id) {
  const key = "replace:" + id;
  running[key] = true;
  render();
  logs.apps.push({ kind: "cmd", text: "supragnosis connect " + id + " --replace" });
  try {
    const out = await invoke("app_toggle", { id });
    logs.apps.push({ kind: "out", text: out });
    connectedHere.add(id);
  } catch (e) {
    logs.apps.push({ kind: "err", text: String(e) });
    toast("bad", APPS[id] + " was not switched", String(e));
  } finally {
    delete running[key];
    await check();
  }
}

async function copy(text, what) {
  try { await invoke("clipboard_copy", { text }); toast("ok", what + " copied"); }
  catch (e) { toast("bad", "Could not copy", String(e)); }
}

function logText() {
  return STEPS.map((s) => {
    const lines = logs[s.log].map((l) => (l.kind === "cmd" ? "$ " : "") + l.text);
    return "## " + s.title + "\n" + (lines.length ? lines.join("\n") : "(nothing ran)");
  }).join("\n\n");
}

async function finishLater() {
  try { await invoke("welcome_set", { later: true }); } catch (_) { /* it shows again next launch */ }
  location.href = graphBack();
}

function openGraph(ws) {
  location.href = ws ? GRAPH + "?workspace=" + encodeURIComponent(ws) : GRAPH;
}

// ---- Rendering ---------------------------------------------------------------------------------------

// A sentence setup wrote, with each command it names in backticks set as code. Text nodes and a code
// element, never markup (S5).
// A sentence's `code` spans; a command - one that begins with "supragnosis " - gets a Copy beside it
// (design-system.md Section 29.5).
function sayNodes(text) {
  return String(text || "").split("`").map((part, i) => {
    if (!(i % 2)) return part;
    const code = h("code", { text: part });
    if (!part.startsWith("supragnosis ")) return code;
    return h("span", { class: "cmd" }, code, h("button", { type: "button", class: "btn quiet copycmd", "aria-label": "Copy " + part,
      on: { click: (e) => { e.preventDefault(); e.stopPropagation(); copy(part, "The command"); } } }, "Copy"));
  });
}

function pill(map, state) {
  const [cls, text] = map[state] || ["", state || "Unknown"];
  return h("span", { class: "pill " + cls }, cls === "ok" || cls === "warn" || cls === "bad" ? h("span", { class: "dot" }) : null,
    state === "working" && map !== FIRST_PILL ? icon("spin", "i-spin") : null, text);
}

function button(label, iconName, cls, onclick, key) {
  const isBusy = key && running[key];
  return h("button", { class: "btn " + (cls || "") + (isBusy ? " busy" : ""), disabled: !!(key && running[key]), on: { click: onclick } },
    isBusy ? icon("spin", "i-spin") : iconName ? icon(iconName) : null, label);
}

function check_(checked, label, onchange) {
  return h("label", { class: "check" }, h("input", { type: "checkbox", checked, on: { change: (e) => onchange(e.target.checked) } }), label);
}

function drawer(s) {
  const open = !!drawers[s.step];
  const lines = logs[s.log];
  const toggle = h("button", { class: "drawer-toggle", "aria-expanded": open ? "true" : "false",
    on: { click: () => { drawers[s.step] = !open; render(); } } }, icon("chevron"), "Details");
  if (!open) return toggle;
  const body = lines.length
    ? lines.map((l) => h("div", { class: "l-" + l.kind, text: l.text }))
    : [h("div", { class: "l-none", text: "Nothing has run for this step yet." })];
  return [toggle, h("div", { class: "drawer", "data-log": s.log }, body)];
}

function stepCard(s, i) {
  const e = stepEvent(s.step);
  const state = stateOf(s.step) || "todo";
  const l = live[s.step];
  const say = (l && running[runKeyOf(s.step)] ? l.say : e && e.say) || "";
  const num = h("div", { class: "num " + state }, state === "done" || state === "skipped" ? icon("check") : String(i + 1));
  const body = { server: serverBody, apps: appsBody, model: modelBody, first: firstBody }[s.step](e, state, say);
  return h("section", { class: "step", "aria-labelledby": "t-" + s.step, "data-k": "step:" + s.step },
    num,
    h("div", { class: "shead" }, h("h3", { id: "t-" + s.step, text: s.title }),
      e ? pill(s.step === "first" ? FIRST_PILL : PILL, state) : null),
    h("div", null, body, drawer(s)));
}

function serverBody(e, state, say) {
  const controls = [];
  if (state === "todo" && !applied) {
    controls.push(check_(plan.login, "Start at Login", (v) => { plan.login = v; }));
  } else if (state === "attention" && /embed rebuild/.test(say)) {
    // A store indexed by another model keeps the daemon from starting (onboarding.md Section 4.1, O4).
    controls.push(button("Rebuild index", "restart", "primary", rebuild, "rebuild"));
  } else if (state === "attention" && /--take-over/.test(say) && !applied) {
    controls.push(check_(plan.takeover, "Take over the daemon that runs now, so it starts at login", (v) => { plan.takeover = v; }));
  } else if ((state === "todo" || state === "failed") && !running.server) {
    controls.push(button("Start at Login", "power", "primary", () => run("server", { failTitle: "The daemon is not kept running" }), "server"));
  } else if (state === "attention" && /--take-over/.test(say)) {
    controls.push(button("Take over", "power", "", () => run("takeover", { failTitle: "The daemon was not taken over" }), "takeover"));
  }
  return [h("p", { class: "say" }, sayNodes(say)), h("div", { class: "controls" }, controls)];
}

function appsBody(e, state, say) {
  const rows = appEvents().map((a) => {
    const name = APPS[a.id] || a.id;
    const key = "app=" + a.id;
    const l = live[key];
    const st = l && running[key] ? l.state : a.state;
    const end = [];
    let sub = null;
    let option = null;
    let tick = null;
    if (st === "todo" && !applied && APPS[a.id]) {
      tick = h("input", { type: "checkbox", checked: plan.apps[a.id] !== false,
        on: { change: (ev) => { plan.apps[a.id] = ev.target.checked; render(); } } });
      end.push(tick);
      if (a.id === "claude-code" && plan.apps[a.id] !== false) {
        option = h("div", { class: "sub option" },
          check_(plan.capture, "After a commit, ask for the decision behind it", (v) => { plan.capture = v; }));
      }
    } else if ((st === "todo" || st === "failed") && APPS[a.id]) {
      end.push(button("Connect", "plug", "primary", () => run(key, { noCapture: a.id === "claude-code" && !plan.capture, failTitle: name + " was not connected" }), key));
    } else if (st === "attention" && /--replace/.test(a.say) && !applied) {
      // Replacing an entry the person made is in the plan, unchecked (O8).
      tick = h("input", { type: "checkbox", checked: !!plan.replace[a.id],
        on: { change: (ev) => { plan.replace[a.id] = ev.target.checked; } } });
      end.push(h("span", { class: "sub", text: "Switch to the bridge" }), tick);
    } else if (st === "attention" && /--replace/.test(a.say)) {
      end.push(button("Switch to the bridge", "plug", "", () => switchToBridge(a.id), "replace:" + a.id));
    }
    if (st === "done" && HELPER[a.id] && connectedHere.has(a.id)) {
      end.push(button(HELPER[a.id], a.id === "cursor" ? "open" : "restart", "", () => helper(a.id), "helper:" + a.id));
    }
    if (st !== "todo" && st !== "skipped") sub = h("div", { class: "sub" }, sayNodes(l && running[key] ? l.say : a.say));
    // A plan row is one checkbox whose label is the whole row (design-system.md Section 6a).
    return h(tick ? "label" : "div", { class: "row" + (st === "skipped" ? " muted" : "") + (tick ? " pick" : ""), "data-k": "app:" + a.id },
      h("span", { class: "lead" }, icon(APP_KIND[a.id] || "desktop")),
      h("div", { class: "body" }, h("div", { class: "title" }, name, pill(APP_PILL, st)), sub, option),
      h("div", { class: "end" }, end));
  });
  return [h("p", { class: "say" + (state === "done" ? " quiet" : "") }, sayNodes(say)),
    rows.length ? h("div", { class: "apps" }, rows) : null];
}

function modelBody(e, state, say) {
  const controls = [];
  const out = [h("p", { class: "say" }, sayNodes(say))];
  if (running.model) {
    const l = live.model;
    const m = l && /^(\d+) of (\d+) MB/.exec(l.say || "");
    const pct = m ? Math.min(100, Math.round((100 * +m[1]) / Math.max(1, +m[2]))) : null;
    const bar = h("div", { class: "bar" + (m ? "" : " busy"), role: "progressbar", "aria-label": "Model download",
      "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": pct === null ? null : String(pct) }, h("span"));
    if (m) bar.firstChild.style.width = pct + "%";
    out.push(bar);
    controls.push(button("Not now", null, "quiet", stopModel));
  } else if (state === "todo" && !applied) {
    controls.push(check_(plan.model, "Download it now, in the background", (v) => { plan.model = v; }));
  } else if (state === "todo" || state === "failed") {
    controls.push(button("Download", "download", "", () => run("model", { failTitle: "The model did not download" }), "model"));
  }
  out.push(h("div", { class: "controls" }, controls));
  return out;
}

async function stopModel() {
  try { const out = await invoke("setup_stop", { step: "model" }); logs.model.push({ kind: "exit", text: out }); }
  catch (e) { toast("bad", "The download did not stop", String(e)); }
}

function firstBody(e, state, say) {
  const out = [];
  if (state === "working") {
    // What to ask is setup's sentence, quoted inside its own; the page only gives it a Copy button.
    const m = /"([^"]+)"/.exec(say);
    const ask = m ? m[1] : say;
    out.push(h("p", { class: "say", text: "In a connected app, in the project you are working on, ask:" }),
      h("div", { class: "ask" }, h("q", { class: "selectable", text: ask }),
        h("button", { class: "btn quiet", on: { click: () => copy(ask, "The question") } }, icon("copy"), "Copy")),
      h("p", { class: "say quiet", text: "The entry appears here as soon as the app records it." }));
  } else {
    out.push(h("p", { class: "say" }, sayNodes(say)));
  }
  return out;
}

function essentialsDone() {
  return ["server", "apps"].every((s) => stepEvent(s) && (stepEvent(s).state === "done" || stepEvent(s).state === "skipped"));
}

function renderIntro() {
  const text = !applied && needsPlan()
    ? "Four steps. Each one says what it ran and what it changed. Nothing changes until you click Set up."
    : "Each step says what it ran. You can leave, and come back from Open Setup in Settings > About.";
  // The heading says the work is done once the essentials are, not only a notice that leaves.
  const title = essentialsDone() ? "Supragnosis is set up" : "Set up supragnosis";
  return h("div", { class: "wintro" }, h("h2", { text: title }), h("p", { text }));
}

function renderActions() {
  const host = document.getElementById("actions");
  // The row is drawn right to left (welcome.css): the page's main act comes first, so it is first in
  // tab order and rightmost; the copy buttons come last and sit at the left, by the privacy line.
  const main = [];
  if (!applied && needsPlan()) {
    // Set up is the default button, which Return presses.
    main.push(h("button", { class: "btn primary", id: "setup-go", disabled: busy() || !events.length, on: { click: applyPlan } }, "Set up"),
      h("button", { class: "btn quiet", on: { click: finishLater } }, "Finish later"));
  } else {
    const essentials = essentialsDone();
    const first = stepEvent("first");
    main.push(h("button", { class: "btn " + (essentials ? "primary" : ""), on: { click: () => openGraph(first && first.state === "done" ? first.id : null) } },
      "Open the graph", icon("arrow")));
    if (!essentials) main.push(h("button", { class: "btn quiet", on: { click: finishLater } }, "Finish later"));
  }
  // While the plan runs, the foot says how far it is (Section 6a).
  if (applied && busy() && planTotal) main.push(h("span", { class: "progress-say", text: "Setting up... " + Math.min(planDone, planTotal) + " of " + planTotal }));
  // One Copy Diagnostics, once a step has run or failed in this visit or the check could not be read,
  // since before that the log is empty and the reads say what the cards already do (Section 29.5).
  const ran = Object.values(logs).some((l) => l.length > 0) || !!loadError
    || STEPS.some((st) => stateOf(st.step) === "failed");
  const diag = h("button", { class: "btn quiet copydiag", on: { click: async () => {
    try { copy(await invoke("diagnostics") + "\n\n" + logText(), "Diagnostics"); } catch (e) { toast("bad", "Could not gather diagnostics", String(e)); }
  } } }, "Copy Diagnostics");
  host.replaceChildren(...main, ...(ran ? [diag] : []));
}

// Every draw keeps the person's focus (design-system.md Section 29.5).
function render() { keepFocus(document.body, draw); }
function draw() {
  const page = document.getElementById("page");
  let body;
  if (loadError && !events.length) {
    body = [h("div", { class: "callout warn" }, icon("warn"), h("div", { class: "grow selectable", text: loadError }))];
  } else if (!events.length) {
    body = [h("div", { class: "step" }, h("div", { class: "skeleton sk-tile" }), h("div", null, h("div", { class: "skeleton sk-line" }), h("div", { class: "skeleton sk-line" })))];
  } else {
    body = STEPS.map(stepCard);
  }
  page.replaceChildren(h("div", { class: "wcol" }, renderIntro(), body));
  renderActions();
}

let pendingRender = false;
function renderSoon() {
  if (pendingRender) return;
  pendingRender = true;
  requestAnimationFrame(() => { pendingRender = false; render(); });
}

// ---- Keeping up ------------------------------------------------------------------------------------

// An entry, or anything else the daemon reports, may have changed what setup says; the checklist is
// read again shortly after, once, rather than per event (Section 4.4).
let recheck = null;
listen("viz-event", () => {
  const first = stepEvent("first");
  if (first && first.state === "done") return;
  clearTimeout(recheck);
  recheck = setTimeout(() => { if (!busy()) check(); }, 1500);
});
setInterval(() => { if (!busy() && document.visibilityState === "visible") check(); }, 15000);

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") location.href = graphBack();
  // Return presses the default button, Set up, unless focus is on a control Return already acts on.
  if (e.key === "Enter" && !applied && needsPlan() && !(e.target.closest && e.target.closest("button, a, textarea"))) {
    const go = document.getElementById("setup-go");
    if (go && !go.disabled) { e.preventDefault(); applyPlan(); }
  }
});
document.getElementById("close").href = graphBack();

// Acts the shell still runs from an earlier visit - the person left Setup while a step ran - are shown
// as running and waited for, instead of being lost or offered again.
let adopting = false;
async function adoptRunning() {
  if (adopting) return;
  let keys = [];
  try { keys = await invoke("setup_running"); } catch (_) { return; }
  keys = keys.filter((k) => !running[k]);
  if (!keys.length) return;
  adopting = true;
  applied = true;
  for (const k of keys) running[k] = true;
  render();
  const wait = async () => {
    let still = [];
    try { still = await invoke("setup_running"); } catch (_) { still = []; }
    for (const k of keys) if (!still.includes(k)) delete running[k];
    if (keys.some((k) => still.includes(k))) { setTimeout(wait, 2000); return; }
    adopting = false;
    await check();
  };
  setTimeout(wait, 2000);
}

render();
loadInfo().then(check).then(adoptRunning);
