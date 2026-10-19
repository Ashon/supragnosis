// What the app's own pages share (docs/settings-page.md, docs/onboarding.md Section 6): building
// elements with DOM calls - createElement and textContent, never a markup string, because what the
// pages show includes CLI output that can quote a server's answer (S5) - the Lucide icon element,
// and the notice an action leaves. Loaded before each page's own script.
"use strict";

// ---- DOM helpers ---------------------------------------------------------------------------

// h("div", {class: "row", on: {click}}, child, "text", ...): an element, its attributes, children.
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
  for (const c of children.flat()) {
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

