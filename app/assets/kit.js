// What the app's own pages share (docs/settings-page.md, docs/onboarding.md Section 6): building
// elements with DOM calls - createElement and textContent, never a markup string, because what the
// pages show includes CLI output that can quote a server's answer (S5) - the Lucide icon element,
// and the notice an action leaves. Loaded before each page's own script.
"use strict";

// ---- The AI apps -----------------------------------------------------------------------------

// The apps `supragnosis connect` knows (docs/client-connect.md Section 4), for both pages: the name
// a button carries, and what kind of app each is, for its icon - a desktop app, an editor, or a
// command-line tool. Their state is the CLI's.
const APPS = {
  "claude-desktop": "Claude Desktop", "claude-code": "Claude Code", cursor: "Cursor",
  vscode: "VS Code", codex: "Codex", gemini: "Gemini CLI", opencode: "OpenCode",
};
const APP_KIND = {
  "claude-desktop": "desktop", "claude-code": "terminal", cursor: "editor",
  vscode: "editor", codex: "terminal", gemini: "terminal", opencode: "terminal",
};

// The AI app's row, built once and drawn by both pages (docs/design-system.md Sections 6a and 29.6):
// one word for each state, one label for Claude Code's capture option, and the helper that finishes
// a connection. A page says what it knows - the row's state in this vocabulary, the line under the
// name - and what goes at the row's end: a checkbox in Setup's plan, a button on Settings.
const APP_PILL = {
  connected: ["ok", "Connected"], todo: ["", "Not connected"], working: ["gold", "Connecting"],
  attention: ["warn", "Needs you"], failed: ["bad", "Failed"], absent: ["", "Not installed"],
  unavailable: ["", "Unavailable"],
};
const CAPTURE = {
  label: "Save the why behind each commit",
  sub: "After a commit, Claude Code is asked to record the decision behind it, if there was one.",
};
// What finishes a connection an app loads only at its start (onboarding.md Section 4.2, D9).
const HELPER = { "claude-desktop": "Restart Claude Desktop", cursor: "Open Cursor" };
function appPill(state) {
  const [cls, text] = APP_PILL[state] || ["", state || "Unknown"];
  return h("span", { class: "pill " + cls }, cls === "ok" || cls === "warn" || cls === "bad" ? h("span", { class: "dot" }) : null,
    state === "working" ? icon("spin", "i-spin") : null, text);
}
// row: { id, state, sub, option, end, pick }. A plan row is one checkbox whose label is the whole
// row (Section 6a), so `pick` makes the row a label.
function appRow(row) {
  return h(row.pick ? "label" : "div", {
    class: "row" + (row.state === "absent" || row.state === "unavailable" ? " muted" : "") + (row.pick ? " pick" : ""),
    "data-k": "app:" + row.id,
  },
    h("span", { class: "lead" }, icon(APP_KIND[row.id] || "desktop")),
    h("div", { class: "body" }, h("div", { class: "title" }, APPS[row.id] || row.id, appPill(row.state)), row.sub || null, row.option || null),
    h("div", { class: "end" }, row.end || null));
}

// ---- The way back --------------------------------------------------------------------------

// The graph as the person left it (docs/design-system.md Section 29.4): the viewer's address the way
// in carried in `?from=`, followed only when it is the viewer's own - its scheme and host - since
// anything else would make "< Graph" a redirect (docs/settings-page.md Section 4, S1). Without one,
// the bare graph.
const GRAPH_HOME = "viz://localhost/";
function graphBack() {
  try {
    const from = new URL(new URLSearchParams(location.search).get("from"));
    const home = new URL(GRAPH_HOME);
    if (from.protocol === home.protocol && from.host === home.host) { from.hash = ""; return from.href; }
  } catch (_) { /* no address, or not one */ }
  return GRAPH_HOME;
}
// This page's address with the way back kept, for a link to the other app page.
function withBack(page) {
  const from = new URLSearchParams(location.search).get("from");
  return from ? page + "?from=" + encodeURIComponent(from) : page;
}

// ---- Focus across a draw (docs/design-system.md Section 29.5) -------------------------------

// A page that draws itself again keeps the person's focus (WCAG 2.4.3). Before the draw the focused
// control's key is noted - the nearest data-k or id at or above it, its kind, and its name - with an
// input's selection; after it, the control with the same key takes focus. A control whose name
// changed with the draw ("Restart" to "Restarting...") is found by the rest of its key when only one
// control matches it.
function focusKey(el, withName) {
  const at = el.closest("[data-k], [id]");
  const field = el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT";
  const name = field ? (el.name || el.getAttribute("aria-label") || el.placeholder || "")
    : (el.getAttribute("aria-label") || el.textContent || "").trim().slice(0, 60);
  return [at ? (at.dataset.k || at.id) : "", el.tagName, el.getAttribute("type") || "", withName ? name : ""].join("|");
}
const FOCUSABLE_ANY = "button, a[href], input, select, textarea, [tabindex]";
function keepFocus(host, draw) {
  const el = document.activeElement;
  const inside = !!el && el !== document.body && host.contains(el);
  const key = inside ? focusKey(el, true) : null, loose = inside ? focusKey(el, false) : null;
  const sel = inside && typeof el.selectionStart === "number" ? [el.selectionStart, el.selectionEnd] : null;
  draw();
  if (!key || (document.activeElement && document.activeElement !== document.body && host.contains(document.activeElement))) return;
  const all = [...host.querySelectorAll(FOCUSABLE_ANY)];
  let to = all.find((c) => focusKey(c, true) === key);
  if (!to) { const near = all.filter((c) => focusKey(c, false) === loose); if (near.length === 1) to = near[0]; }
  if (!to) return;
  to.focus({ preventScroll: true });
  if (sel && typeof to.setSelectionRange === "function") { try { to.setSelectionRange(sel[0], sel[1]); } catch (_) { /* not a text field */ } }
}

// ---- DOM helpers ---------------------------------------------------------------------------

// h("div", {class: "row", on: {click}}, child, "text", ...): an element, its attributes, children.
// Cmd+[ goes back to the graph from the app's pages, as Esc does (docs/design-system.md Section 6a,
// U26): the way back is the title bar's "< Graph".
document.addEventListener("keydown", (e) => {
  const back = document.getElementById("close");
  if (e.key === "[" && (e.metaKey || e.ctrlKey) && back) { e.preventDefault(); location.href = back.href; }
});
// A tooltip names a control and its key, and explains nothing a person needs to act
// (docs/design-system.md Section 4, U25).
function tip(name, key) { return key ? name + " (" + key + ")" : name; }
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
  // Flattened to any depth: a child that is a list of rows, inside a list of children, was once
  // turned into text - "[object HTMLDivElement],[object HTMLDivElement]" in place of a row.
  for (const c of children.flat(Infinity)) {
    if (c === null || c === undefined || c === false) continue;
    el.append(c instanceof Node ? c : document.createTextNode(String(c)));
  }
  return el;
}

const icon = (name, extra) => h("span", { class: "i i-" + name + (extra ? " " + extra : ""), "aria-hidden": "true" });

// ---- Dialogs hold focus (docs/design-system.md Section 10, U19) ----------------------------
// holdFocus(dialog): Tab and Shift+Tab stay inside the dialog, and every other child of the body is
// inert while it is open, so neither the keyboard nor a screen reader wanders behind it. The function
// it returns gives the page back. Giving focus back to what opened the dialog is the caller's, which
// knows what that was.
const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';
function holdFocus(dialog) {
  const host = dialog.closest("body > *") || dialog;
  const others = [...document.body.children].filter(el => el !== host && !el.inert);
  for (const el of others) el.inert = true;
  const onKey = (e) => {
    if (e.key !== "Tab") return;
    const all = [...dialog.querySelectorAll(FOCUSABLE)].filter(el => el.getClientRects().length);
    if (!all.length) { e.preventDefault(); return; }
    const first = all[0], last = all[all.length - 1];
    if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  };
  dialog.addEventListener("keydown", onKey);
  return () => {
    dialog.removeEventListener("keydown", onKey);
    for (const el of others) el.inert = false;
  };
}

// ---- Notices --------------------------------------------------------------------------------

// The viewer's anatomy and clock (docs/design-system.md Section 28.4, U42): the kind's icon - ok, an
// act done; bad, refused or not done; info, news - the words, and a 28-pixel dismiss. At most four,
// the newest first.
const NOTICE_ICON = { ok: "ok", bad: "alert", info: "info" };
const REDUCED_MOTION = matchMedia("(prefers-reduced-motion: reduce)").matches;
function toast(kind, title, detail) {
  const host = document.getElementById("toasts");
  const close = h("button", { type: "button", class: "btn quiet iconbtn", "aria-label": "Dismiss" }, icon("x"));
  const el = h("div", { class: "toast " + kind },
    icon(NOTICE_ICON[kind] || "info"),
    h("div", { class: "grow" }, h("div", { class: "t-title", text: title }), detail ? h("div", { class: "t-detail", text: detail }) : null),
    close);
  close.addEventListener("click", () => dismiss(el));
  host.prepend(el);
  // A success needs no reading twice; an error stays until it is dismissed.
  if (kind === "ok") leaveAfter(el, 4500);
  while (host.children.length > 4) host.lastChild.remove();
}

// A success leaves 4.5 seconds after it shows, counted only while it is not being read: the pointer
// over it or focus inside it stops the clock, and the whole wait starts again once both have left
// (WCAG 2.2.1).
function leaveAfter(el, ms) {
  let timer = 0, pointer = false, focus = false;
  const sync = () => { clearTimeout(timer); if (!pointer && !focus) timer = setTimeout(() => dismiss(el), ms); };
  el.addEventListener("pointerenter", () => { pointer = true; sync(); });
  el.addEventListener("pointerleave", () => { pointer = false; sync(); });
  el.addEventListener("focusin", () => { focus = true; sync(); });
  el.addEventListener("focusout", (e) => { focus = el.contains(e.relatedTarget); sync(); });
  sync();
}

// A notice leaves by fading over 120 ms, and at once under reduced motion.
function dismiss(el) {
  if (!el.isConnected || el.classList.contains("out")) return;
  if (REDUCED_MOTION) { el.remove(); return; }
  el.classList.add("out");
  setTimeout(() => el.remove(), 120);
}

