// --- Controls say what they are (docs/design-system.md Sections 4 and 10, U19) -----------------
// A switch applies at once and says on or off. A chip that filters says whether it is pressed. One
// choice among a few is a radio group: one tab stop, and the arrows move the choice. The class the
// stylesheet draws follows the state these set, never the other way round.
function setSwitch(el, on) {
  el.setAttribute("role", "switch");
  el.setAttribute("aria-checked", on ? "true" : "false");
  el.classList.toggle("on", !!on);
}
function setPressed(el, on) {
  el.setAttribute("aria-pressed", on ? "true" : "false");
}
// radioGroup(host, label): the host's buttons become the radios of one group named `label`; the
// checked one is the button whose `on` class is set, and none is checked when none is. Arrows and
// Home/End pick and move focus, as clicking does; call it again whenever the buttons or the choice
// change, so the state said is the state drawn.
function radioGroup(host, label) {
  host.setAttribute("role", "radiogroup");
  if (label) host.setAttribute("aria-label", label);
  const radios = [...host.querySelectorAll("button")];
  const checked = radios.find(b => b.classList.contains("on"));
  for (const b of radios) {
    b.setAttribute("role", "radio");
    b.setAttribute("aria-checked", b === checked ? "true" : "false");
    b.tabIndex = b === (checked || radios[0]) ? 0 : -1;
  }
  if (host.dataset.radios) return;
  host.dataset.radios = "1";
  host.addEventListener("keydown", ev => {
    const all = [...host.querySelectorAll('[role="radio"]')];
    const i = all.indexOf(document.activeElement);
    if (i < 0) return;
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[ev.key];
    const j = ev.key === "Home" ? 0 : ev.key === "End" ? all.length - 1
      : step ? (i + step + all.length) % all.length : -1;
    if (j < 0) return;
    ev.preventDefault();
    // A disabled choice is passed by, as b passes it (Section 29.12).
    let k = j;
    if (step) for (let n = 0; n < all.length && all[k].getAttribute("aria-disabled") === "true"; n++) k = (k + step + all.length) % all.length;
    all[k].click();
    (host.querySelector('[aria-checked="true"]') || all[k]).focus();
  });
}

function arrowTabs(list) {
  list.addEventListener("keydown", ev => {
    if (!["ArrowRight", "ArrowLeft", "Home", "End"].includes(ev.key)) return;
    const tabs = [...list.querySelectorAll('[role="tab"]')];
    const i = tabs.indexOf(document.activeElement);
    if (i < 0) return;
    const j = ev.key === "Home" ? 0 : ev.key === "End" ? tabs.length - 1
      : (i + (ev.key === "ArrowRight" ? 1 : -1) + tabs.length) % tabs.length;
    ev.preventDefault();
    // An idle tab is reached and not chosen (Section 29.10, 29.11).
    if (tabs[j].getAttribute("aria-disabled") === "true") { tabs[j].focus(); return; }
    tabs[j].click();
    // A strip that re-renders on click replaces its buttons, so focus the selected one afresh.
    (list.querySelector('[aria-selected="true"]') || tabs[j]).focus();
  });
}

async function renderSettings() {
  const strip = document.getElementById("settingsTabs");
  const host = document.getElementById("settingsBody");
  strip.textContent = "";
  let selected = null;
  for (const t of SETTINGS_TABS) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "stab" + (t.id === settingsTab ? " on" : "");
    b.textContent = t.label;
    b.onclick = () => { settingsTab = t.id; renderSettings(); };
    strip.appendChild(b);
    if (t.id === settingsTab) selected = b;
  }
  markTabs(strip, [...strip.children], selected);
  host.setAttribute("role", "tabpanel");
  host.textContent = "";
  const tab = SETTINGS_TABS.find(t => t.id === settingsTab) || SETTINGS_TABS[0];
  // The tab owns its own fetch and its own failure, so an unreachable surface empties one tab rather
  // than the dialog.
  await tab.render(host);
}

// What this build IS. Read from the binary that answered the request rather than restated in the
// page, so the version cannot drift from what is running.
async function renderAboutSection(host) {
  let a;
  try {
    a = await (await fetch("/api/about", { cache: "no-store" })).json();
  } catch (e) {
    host.innerHTML = '<div class="empty">version information unavailable</div>';
    return;
  }
  const repo = String(a.repository || "");
  const row = (k, v) => html`<dt>${k}</dt><dd>${v}</dd>`;
  host.innerHTML = html`<dl class="kv">${[
    row("Package", String(a.name || "supragnosis")),
    row("Version", String(a.version || "")),
    row("License", String(a.license || "")),
    repo ? row("Source", html`<a href="${repo}" target="_blank" rel="noreferrer noopener">${repo}</a>`) : "",
  ]}</dl><div class="note">Dependency licenses are not restated here - a hand-kept list is one that \
rots without saying so. They are in the manifest and lockfile at the source above, which \
cannot disagree with this build.</div>`;
}

async function renderFedSection(host) {
  let f;
  try {
    f = await (await fetch("/api/federation", { cache: "no-store" })).json();
  } catch (e) {
    host.innerHTML = '<div class="empty">federation status unavailable</div>';
    return;
  }
  if (!f || f.configured === false) {
    host.innerHTML = html`<div class="empty">Sync is not set up on ${here()} - create ~/.supragnosis/supragnosis.toml to join or host</div>`;
    return;
  }

  const parts = [html`<div class="fsec">${hereTitle()}</div>`,
    html`<div class="prow"><span class="pid">${String(f.node_id || "")}</span>`,
    html`<span class="hint"> ${f.role === "hub" ? "Hub" : "Spoke"}</span></div>`];

  const hubs = f.servers || [];
  if (hubs.length) {
    parts.push(html`<div class="fsec">Hubs ${here()} syncs to</div>`);
    for (const s of hubs) {
      const dot = s.healthy ? TEAL : BAD;
      parts.push(html`<div class="prow"><span class="dot" style="background:${dot}"></span> `,
        html`<span class="pid">${String(s.url || "").replace(/^https?:\/\//, "")}</span>`,
        s.version ? html`<span class="hint"> v${s.version}</span>` : "",
        s.healthy ? "" : html`<span class="hint"> unreachable</span>`);
      for (const w of (s.workspaces || [])) {
        const insync = !(w.local_ahead | 0) && !(w.hub_ahead | 0);
        parts.push(html`<div class="hint">${w.workspace}: ${insync
          ? "in sync"
          : `${here()} +${w.local_ahead | 0}, hub +${w.hub_ahead | 0}`}</div>`);
      }
      // The two buckets only this section names, moved from the dock's Peers tab. Listed here but
      // not admitted there is a setup error that would otherwise just vanish; admitted there but not
      // shared here is knowledge left on the table. Intersecting the two lists would hide both.
      for (const w of (s.local_only || [])) parts.push(html`<div class="hint">${w}: <span class="bad">not admitted by the hub</span></div>`);
      for (const w of (s.peer_only || [])) parts.push(html`<div class="hint">${w}: admitted, not shared from ${here()}</div>`);
      // Unknown is not an empty grant set: a hub that has not answered must not read as one that
      // revoked everything.
      if (!s.negotiated_at) parts.push(html`<div class="hint">what it shares: unknown - no answer yet</div>`);
      else if (f.updated_ms) {
        const age = Math.max(0, Math.round((f.updated_ms - s.negotiated_at) / 1000));
        parts.push(html`<div class="hint">what it shares: agreed ${age}s ago</div>`);
      }
      parts.push(html`</div>`);
    }
  }
  const known = f.known_peers || [];
  if (known.length || f.role === "hub") {
    parts.push(html`<div class="fsec">Peers that have checked in</div>`);
    if (!known.length) parts.push(html`<div class="prow none">no peer has checked in yet</div>`);
    for (const p of known) {
      const ago = f.updated_ms && p.last_seen_ms ? Math.max(0, Math.round((f.updated_ms - p.last_seen_ms) / 1000)) : null;
      parts.push(html`<div class="prow"><span class="pid">${String(p.node_id || "").slice(0, 16)}</span>`,
        html`<span class="hint"> ${p.last_action}${ago !== null ? " " + ago + "s ago" : ""} (${p.hits | 0}) - `,
        html`${p.release ? "v" + p.release : "release not known"}, sync ${p.protocol | 0}</span></div>`);
    }
  }

  const admitted = f.admitted || [];
  if (f.role === "hub") {
    parts.push(html`<div class="fsec">Peers admitted here, and what each may read</div>`);
    if (!admitted.length) {
      parts.push(html`<div class="prow none">no peer is admitted - add one in supragnosis.toml</div>`);
    }
    for (const a of admitted) {
      const ws = a.shared_workspaces || [];
      parts.push(html`<div class="prow"><span class="pid">${String(a.node_id || "")}</span><div class="wschips">`);
      if (!ws.length) {
        parts.push(html`<span class="none">admitted, may read nothing</span>`);
      } else {
        for (const w of ws) {
          const armed = fedCfgArmed === a.node_id + "\u0000" + w;
          parts.push(html`<button type="button" class="wsc${armed ? " armed" : ""}" data-node="${a.node_id}" `,
            html`data-ws="${w}" title="${tipText(armed ? "Press again to stop" : "Stop sharing")}">`,
            html`${w}<span class="x">${armed ? "confirm" : html`<span class="i i-x" aria-hidden="true"></span>`}</span></button>`);
        }
      }
      parts.push(html`</div></div>`);
    }
    parts.push(html`<div class="note">Removing a grant takes effect at once and is written to `,
      html`supragnosis.toml. It stops FUTURE reads - it does not take back what has already synced. `,
      html`Granting a workspace, and adding or removing a peer, stay in the file.</div>`);
  }

  host.innerHTML = html`${parts}`;

  host.querySelectorAll(".wsc").forEach(el => {
    el.onclick = async () => {
      const node = el.getAttribute("data-node"), ws = el.getAttribute("data-ws");
      const key = node + "\u0000" + ws;
      if (fedCfgArmed !== key) { fedCfgArmed = key; renderSettings(); return; }
      fedCfgArmed = null;
      const row = (admitted.find(a => a.node_id === node) || {}).shared_workspaces || [];
      const keep = row.filter(w => w !== ws);
      const q = `?node_id=${encodeURIComponent(node)}&workspaces=${encodeURIComponent(keep.join(","))}`;
      try {
        const r = await fetch("/api/peer/share" + q, { method: "POST", cache: "no-store" });
        if (!r.ok) {
          const body = await r.json().catch(() => ({}));
          const err = document.createElement("div");
          err.className = "err";
          err.textContent = body.error || `refused (${r.status})`;
          host.appendChild(err);
        }
      } catch (e) { /* the re-render below shows the unchanged state */ }
      renderSettings();
      refreshSyncPill();
    };
  });
}

function openSettings() {
  const d = document.getElementById("settings");
  if (!d) return;
  fedCfgArmed = null;
  renderSettings();
  d.showModal();
  // A chip left armed across a close would commit on the next stray click.
  d.onclose = () => { fedCfgArmed = null; };
}

// The legend's order, sorted by count then name once per scope, so a later read never reorders a
// chip under the pointer; a type that arrives later is added at the end (Section 29.9).
let legendOrder = { scope: null, nodes: [], edges: [] };
function legendCounts() {
  const nodeCount = {}, edgeCount = {};
  for (const n of nodes) nodeCount[n.type] = (nodeCount[n.type] || 0) + 1;
  for (const e of edges) edgeCount[e.type] = (edgeCount[e.type] || 0) + 1;
  const byCount = c => Object.keys(c).sort((a, b) => c[b] - c[a] || (a < b ? -1 : a > b ? 1 : 0));
  const scope = currentWs();
  if (legendOrder.scope !== scope) legendOrder = { scope, nodes: byCount(nodeCount), edges: byCount(edgeCount) };
  else {
    for (const [k, c] of [["nodes", nodeCount], ["edges", edgeCount]]) {
      const kept = legendOrder[k].filter(t => c[t]);
      for (const t of byCount(c)) if (!kept.includes(t)) kept.push(t);
      legendOrder[k] = kept;
    }
  }
  return { nodeCount, edgeCount };
}
// A section's head says how many of its types are hidden, with Show all (Section 29.9).
function sectionHidden(ctEl, keys, offSet) {
  const head = ctEl && ctEl.parentElement;
  if (!head) return;
  let note = head.querySelector(".rhid");
  const hidden = keys.filter(t => offSet.has(t)).length;
  if (!hidden) { if (note) note.remove(); return; }
  if (!note) { note = document.createElement("span"); note.className = "rhid"; head.appendChild(note); }
  const all = document.createElement("button");
  all.type = "button"; all.className = "linkish"; all.textContent = "Show all";
  all.onclick = () => { for (const t of keys) offSet.delete(t); renderLegend(); renderStats(); };
  note.replaceChildren(document.createTextNode(" - " + hidden + " hidden "), all);
}
function renderLegend() {
  // Node-type and edge-kind legends, each in its own dock section. Clicking a chip toggles that kind's
  // visibility (the off set); Only, or Option-click, shows that kind alone.
  // Chips are recreated on every render - clear any hover highlight (and the chip tooltip) so
  // neither can stick to a dead chip that will never fire mouseleave.
  typeHl = null; edgeTypeHl = null; tip.style.display = "none";
  const { nodeCount, edgeCount } = legendCounts();
  const fill = (host, keys, colorOf, offSet, isEdge) => {
    host.innerHTML = "";
    if (!keys.length) { host.innerHTML = '<span class="lbl">none</span>'; return; }
    const counts = isEdge ? edgeCount : nodeCount;
    const only = t => { offSet.clear(); for (const k of keys) if (k !== t) offSet.add(k); renderLegend(); renderStats(); };
    for (const t of keys) {
      const wrap = document.createElement("span");
      wrap.className = "lgw";
      const el = document.createElement("button");
      el.type = "button";
      el.className = "lg" + (offSet.has(t) ? " off" : "");
      setPressed(el, !offSet.has(t));
      const sw = document.createElement("span"); sw.className = "sw"; sw.style.background = colorOf(t);
      if (isEdge) { sw.style.height = "3px"; sw.style.borderRadius = "2px"; }  // line-like look
      const n = document.createElement("span"); n.className = "n"; n.textContent = String(counts[t] || 0);
      el.append(sw, document.createTextNode(t || "(none)"), n);
      if (offSet.has(t)) el.appendChild(icon("eye-off"));
      el.onclick = (e) => {
        if (e.altKey) { only(t); return; }
        if (offSet.has(t)) offSet.delete(t); else offSet.add(t);
        renderLegend(); renderStats();
      };
      // Hovering a chip highlights its nodes/edges on the graph (render-only - no sim wake) and
      // shows the type's T-Box definition in the styled tooltip (Principle 8: a type has a stated
      // meaning - surfaced right where the type is read, not only in the definitions below).
      el.onmouseenter = () => { if (isEdge) edgeTypeHl = t; else typeHl = t; showTypeTip(el, t, isEdge); };
      el.onmouseleave = () => {
        if (isEdge) { if (edgeTypeHl === t) edgeTypeHl = null; }
        else if (typeHl === t) typeHl = null;
        tip.style.display = "none";
      };
      const o = document.createElement("button");
      o.type = "button"; o.className = "lgonly"; o.textContent = "Only";
      o.setAttribute("aria-label", "Only " + (t || "(none)"));
      o.onclick = () => only(t);
      wrap.append(el, o);
      host.appendChild(wrap);
    }
  };
  const nodeKeys = legendOrder.nodes, edgeKeys = legendOrder.edges;
  fill(legendNodesEl, nodeKeys, t => typeColor[t], typeOff, false);
  fill(legendEdgesEl, edgeKeys, () => EDGE_NEUTRAL, edgeTypeOff, true);
  nodeCtEl.textContent = nodeKeys.length || "";
  edgeCtEl.textContent = edgeKeys.length || "";
  sectionHidden(nodeCtEl, nodeKeys, typeOff);
  sectionHidden(edgeCtEl, edgeKeys, edgeTypeOff);
  renderBoundaryChips();
  renderList();
}

// Legend chip tooltip: the type's glossary definition (T-Box), anchored beside the chip. A type
// with no recorded definition gets a nudge toward define_type instead of silence - curation as a
// micro-decision in the reading flow (Principle 22), not a separate chore.
function showTypeTip(el, t, isEdge) {
  const target = isEdge ? "relation" : "entity";
  const def = glossaryTypes.find(x => x.target === target && x.name === t);
  // Beside the chip and its Only, which the card must not cover (Section 29.9).
  const r = (el.closest(".lgw") || el).getBoundingClientRect();
  tip.style.display = "block";
  tip.style.left = Math.min(r.right + 10, innerWidth - 330) + "px";
  tip.style.top = Math.max(6, r.top - 4) + "px";
  tip.innerHTML = html`<b>${t || "(none)"}</b> <span class="k">${target} type</span>${def
    ? html`<div class="tdef">${def.description}</div><span class="k">${plural(def.sources | 0, "source")}</span>`
    : html`<div class="tdef none">no definition recorded - give this type a meaning with define_type</div>`}`;
}

// What a hover lights without dimming the rest (Section 29.9): the entity, its relations and its
// neighbours, whose names show. Null while a selection holds the map, which lights its own.
function hoverSet() {
  if (!hover || focus) return null;
  const ns = new Set([hover.id]), es = new Set();
  for (let i = 0; i < edges.length; i++) {
    const e = edges[i];
    if (e.a.id === hover.id || e.b.id === hover.id) { es.add(i); ns.add(e.a.id); ns.add(e.b.id); }
  }
  for (const m of namesakesOf(hover)) ns.add(m.id);   // the same name elsewhere (Section 29.16)
  return { ns, es };
}
// The faint strength a selection leaves the rest at, which a dimmed trust tier takes too (D5).
const DIM_NODE = 0.12, DIM_EDGE = 0.06;
// The set of nodes/edges to highlight from hover/focus/search. If none, null (everything highlighted equally).
function activeSet() {
  // A selection dims the rest; a hover does not (hoverSet, design-system.md Section 29.9).
  const anchor = focus;
  if (anchor) {
    const ns = new Set([anchor.id]), es = new Set();
    for (let i = 0; i < edges.length; i++) {
      const e = edges[i];
      if (e.a.id === anchor.id || e.b.id === anchor.id) { es.add(i); ns.add(e.a.id); ns.add(e.b.id); }
    }
    for (const m of namesakesOf(anchor)) ns.add(m.id);   // lit as its neighbours are (Section 29.16)
    return { ns, es };
  }
  // The open search lights what the engine answered, not names that contain the typed text, so the
  // list and the map give one answer (docs/design-system.md Section 8).
  if (searchLit) return { ns: searchLit, es: new Set() };
  // A selected proposal's preview dims everything it does not touch, instead of ringing its
  // targets: rings belong to trust (docs/design-system.md Section 5). Its own overlay - the fold
  // arrows, the accented edges - is drawn on top.
  if (proposalSel) {
    const touched = proposalSel.kind === "entity_merge"
      ? [proposalSel.into, ...(proposalSel.targets || [])].map(nodeById).filter(Boolean)
      : affectedNodes(proposalSel);
    if (touched.length) {
      const ns = new Set(touched.map(n => n.id)), es = new Set();
      for (let i = 0; i < edges.length; i++) if (ns.has(edges[i].a.id) || ns.has(edges[i].b.id)) es.add(i);
      return { ns, es };
    }
  }
  return null;
}

