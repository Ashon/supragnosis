// What the app's own pages share (docs/settings-page.md, docs/onboarding.md Section 6): building
// elements with DOM calls - createElement and textContent, never a markup string, because what the
// pages show includes CLI output that can quote a server's answer (S5) - the Lucide icon element,
// and the notice an action leaves. Loaded before each page's own script.
"use strict";

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

