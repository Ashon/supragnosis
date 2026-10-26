// --- Interaction ---------------------------------------------------------------------
// A pinch zooms as far as the fingers spread (Section 29.30): a trackpad pinch, which Chrome and
// Firefox send as wheel events with ctrlKey and WebKit as gesture events, scales the map 1:1 around
// the pointer through the touch pinch's own camera move. The mouse wheel keeps its step a notch.
let gestureScale = null;   // the scale WebKit last reported, while a trackpad pinch runs
canvas.addEventListener("wheel", ev => {
  if (settling) return;   // graph is hidden behind the loader - ignore interaction until it settles
  ev.preventDefault();
  if (ev.ctrlKey) {
    if (gestureScale !== null) return;   // the gesture events carry this pinch; never count it twice
    const dy = ev.deltaY * (ev.deltaMode === 1 ? 16 : ev.deltaMode === 2 ? innerHeight : 1);
    pinchTo(ev.clientX, ev.clientY, Math.min(1.25, Math.max(0.8, Math.exp(-dy / 100))));
    userMoved = true; personMoved();
    return;
  }
  zoomAt(ev.clientX, ev.clientY, ev.deltaY < 0 ? 1.12 : 0.89);
  personMoved();
}, { passive: false });
canvas.addEventListener("gesturestart", ev => {
  ev.preventDefault();   // the map takes the pinch; the page is not magnified
  if (!settling) gestureScale = ev.scale || 1;
});
canvas.addEventListener("gesturechange", ev => {
  ev.preventDefault();
  if (settling || gestureScale === null || !ev.scale) return;
  pinchTo(ev.clientX, ev.clientY, ev.scale / gestureScale);
  gestureScale = ev.scale;
  userMoved = true; personMoved();
});
canvas.addEventListener("gestureend", ev => { ev.preventDefault(); gestureScale = null; });

canvas.addEventListener("mousedown", ev => {
  if (settling) return;   // graph hidden (loading) - no drag/pan/select
  downPos = { x: ev.clientX, y: ev.clientY };
  const n = nodeAt(ev.clientX, ev.clientY);
  if (n) { drag = n; }   // press alone does not reheat - wake only once the pointer actually drags (mousemove)
  else {
    // Clicking empty canvas clears any proposal preview, but not one Review opened, which keeps its
    // bar until Back to Review or Done (Section 29.3).
    if (proposalSel && !reviewPreview) { proposalSel = null; requestFrame(); }
    panning = { sx: ev.clientX, sy: ev.clientY, px: cam.x, py: cam.y }; canvas.classList.add("grabbing");
  }
});
addEventListener("mousemove", ev => {
  if (drag) {
    const [wx, wy] = toWorld(ev.clientX, ev.clientY); drag.x = wx; drag.y = wy; positionsGen++;
    canvas.classList.add("grabbing");   // grabbing while dragging (Section 29.9)
    wake(0.3); showTip(null); personMoved(); return;
  }
  if (panning) {
    // Panning is instant (1:1) - move cam and camT together so easing does not drag behind.
    cam.x = camT.x = panning.px + (ev.clientX - panning.sx);
    cam.y = camT.y = panning.py + (ev.clientY - panning.sy);
    userMoved = true; showTip(null); personMoved(); return;
  }
  // Only canvas-targeted moves drive node hover from here on. Over the chrome (docks, header,
  // statusbar) the shared #tip belongs to whatever chrome element is showing it (legend chip
  // definitions) - and node hover through an opaque panel was wrong anyway.
  if (ev.target !== canvas) return;
  if (settling) { showTip(null); return; }   // no hover while the graph is hidden behind the loader
  const ph = peerAt(ev.clientX, ev.clientY);   // peer cursor-dots sit above the graph - test them first
  if (ph) { hover = null; showPeerTip(ph.m, ph.id, ev.clientX, ev.clientY); return; }
  hover = nodeAt(ev.clientX, ev.clientY);
  // A hand over an entity, a grab hand over the ground (Section 29.9).
  canvas.classList.toggle("over", !!hover);
  showTip(hover, ev.clientX, ev.clientY);
});
addEventListener("mouseup", ev => {
  if (settling) { drag = null; panning = null; canvas.classList.remove("grabbing"); return; }   // drop any gesture that spanned into a reload
  const moved = downPos && Math.hypot(ev.clientX - downPos.x, ev.clientY - downPos.y) > 4;
  if (!moved && ev.target === canvas) {
    const n = nodeAt(ev.clientX, ev.clientY);
    // A node click selects it (select() puts the inspector first, the camera second: insetB() measures
    // the open panel); a click on the same node or on empty space clears it and leaves the camera,
    // since the person is moving it (Section 29.4).
    if (n && focus !== n) select(n); else clearSelection(false);
  }
  if (drag && moved) {
    wake(0.3);   // settle neighbors only after a real drag, not a plain click
    // Where it was let go reaches the layout before the node is let go, so the next report does not
    // put it back where the last frame left it.
    layoutSync();
  }
  drag = null;
  panning = null; canvas.classList.remove("grabbing");
  downPos = null;
});

// --- Touch (docs/design-system.md Section 29.18) ---------------------------------------------------
// The map takes touches for itself: pointer events of the touch kind, whose down is cancelled so the
// browser sends no mouse events after them; the mouse keeps its handlers above. One finger pans; held
// still on an entity for half a second it picks the entity up; a tap is a click; a double tap on the
// ground zooms in twice as close; two fingers pinch and pan under the fingers; a third is ignored.
const touches = new Map();
let touchTap = null, touchPan = null, touchDrag = null, pinch = null, lastTap = null, holdTimer = 0;
const TOUCH_SLOP = 10, TOUCH_REACH = 22, HOLD_MS = 500, DOUBLE_MS = 300;
// A finger reaches an entity within 22 pixels of its centre, or as far as a click does.
function touchNodeAt(sx, sy) {
  const hit = nodeAt(sx, sy);
  if (hit) return hit;
  const [wx, wy] = toWorld(sx, sy), reach = TOUCH_REACH / cam.s;
  let best = null, bd = reach;
  for (const k of nodesIn(wx - reach, wy - reach, wx + reach, wy + reach)) {
    const n = nodes[k];
    if (typeOff.has(n.type) || (onScreen(n) ? n._o === 0 : frameKept)) continue;
    const d = Math.hypot(n.x - wx, n.y - wy);
    if (d < bd) { bd = d; best = n; }
  }
  return best;
}
// The camera under the fingers, 1:1: both the shown camera and its target move together.
function pinchTo(mx, my, f) {
  const s = Math.max(scaleFloor(), Math.min(4, cam.s * f)), wx = (mx - cam.x) / cam.s, wy = (my - cam.y) / cam.s;
  cam.s = camT.s = s; cam.x = camT.x = mx - wx * s; cam.y = camT.y = my - wy * s;
}
function twoFingers() {
  const [a, b] = [...touches.values()];
  return { d: Math.hypot(a.x - b.x, a.y - b.y) || 1, mx: (a.x + b.x) / 2, my: (a.y + b.y) / 2 };
}
canvas.addEventListener("pointerdown", ev => {
  if (ev.pointerType !== "touch") return;
  ev.preventDefault();
  if (settling || touches.size >= 2) return;   // a third finger is ignored
  const a = document.activeElement;
  if (a && a !== document.body && a.blur) a.blur();   // a cancelled down moves no focus
  try { canvas.setPointerCapture(ev.pointerId); } catch (_) { /* the pointer is already gone */ }
  touches.set(ev.pointerId, { x: ev.clientX, y: ev.clientY });
  clearTimeout(holdTimer);
  showTip(null);
  if (touches.size === 1) {
    touchTap = { x: ev.clientX, y: ev.clientY, moved: false };
    touchPan = { sx: ev.clientX, sy: ev.clientY, px: cam.x, py: cam.y };
    const n = touchNodeAt(ev.clientX, ev.clientY);
    if (n) holdTimer = setTimeout(() => {
      if (touchTap && !touchTap.moved && touches.size === 1) { touchDrag = n; touchPan = null; canvas.classList.add("grabbing"); }
    }, HOLD_MS);
    return;
  }
  // The second finger: an entity being moved is let go where it is, and the pinch begins.
  if (touchDrag) { layoutSync(); touchDrag = null; canvas.classList.remove("grabbing"); }
  touchTap = null; touchPan = null;
  pinch = twoFingers();
});
canvas.addEventListener("pointermove", ev => {
  if (ev.pointerType !== "touch" || !touches.has(ev.pointerId)) return;
  touches.set(ev.pointerId, { x: ev.clientX, y: ev.clientY });
  if (pinch && touches.size === 2) {
    const now = twoFingers();
    cam.x = camT.x = cam.x + (now.mx - pinch.mx); cam.y = camT.y = cam.y + (now.my - pinch.my);
    pinchTo(now.mx, now.my, now.d / pinch.d);
    pinch = now; userMoved = true; personMoved(); return;
  }
  if (touchTap && !touchTap.moved && Math.hypot(ev.clientX - touchTap.x, ev.clientY - touchTap.y) >= TOUCH_SLOP) {
    touchTap.moved = true; clearTimeout(holdTimer);
  }
  if (touchDrag) {
    const [wx, wy] = toWorld(ev.clientX, ev.clientY); touchDrag.x = wx; touchDrag.y = wy; positionsGen++;
    wake(0.3); personMoved(); return;
  }
  if (touchPan && touchTap && touchTap.moved) {
    cam.x = camT.x = touchPan.px + (ev.clientX - touchPan.sx);
    cam.y = camT.y = touchPan.py + (ev.clientY - touchPan.sy);
    userMoved = true; personMoved();
  }
});
function touchEnd(ev, cancelled) {
  if (ev.pointerType !== "touch" || !touches.has(ev.pointerId)) return;
  touches.delete(ev.pointerId);
  clearTimeout(holdTimer);
  if (pinch) {
    pinch = null;
    // The finger left on the glass carries on panning from where it is.
    const rest = [...touches.values()][0];
    if (rest) touchPan = { sx: rest.x, sy: rest.y, px: cam.x, py: cam.y };
    touchTap = null;
    return;
  }
  if (touches.size) return;
  if (touchDrag) { wake(0.3); layoutSync(); touchDrag = null; canvas.classList.remove("grabbing"); }
  const tap = touchTap;
  touchTap = null; touchPan = null;
  if (cancelled || !tap || tap.moved || settling) return;
  const n = touchNodeAt(tap.x, tap.y), now = performance.now();
  const dbl = !!lastTap && now - lastTap.t < DOUBLE_MS && Math.hypot(tap.x - lastTap.x, tap.y - lastTap.y) < 24;
  lastTap = dbl ? null : { x: tap.x, y: tap.y, t: now };
  if (dbl && !n) { zoomAt(tap.x, tap.y, 2); personMoved(); return; }   // eases as the buttons do
  if (dbl && n && focus === n) return;   // a double tap on an entity selects it once
  if (n && focus !== n) select(n); else clearSelection(false);
}
canvas.addEventListener("pointerup", ev => touchEnd(ev, false));
canvas.addEventListener("pointercancel", ev => touchEnd(ev, true));
// While the page is zoomed in, pinches go to the browser, so the page can always be zoomed back out.
function touchActionForZoom() {
  canvas.classList.toggle("pagezoom", !!window.visualViewport && window.visualViewport.scale > 1.01);
}
if (window.visualViewport) window.visualViewport.addEventListener("resize", touchActionForZoom);
// Under 760 the camera buttons ride above whichever sheet is open (Section 29.18): --foot is that
// sheet's top, read from its layout (a sliding panel's transform aside, so the toolbar moves with
// its slide), and --hud-h the toolbar's own height, which the sheet's taller height leaves room for.
function placeFoot() {
  const root = document.documentElement.style;
  if (innerWidth >= 760) { root.removeProperty("--foot"); root.removeProperty("--hud-h"); return; }
  let top = innerHeight - BOTTOM_INSET - 8;
  for (const el of [detailEl, dockLEl, dockREl, stripEl]) {
    if (!el || window.getComputedStyle(el).display === "none") continue;
    const open = el === detailEl ? el.classList.contains("on") : el === stripEl ? true : el.classList.contains("on");
    if (open && el.offsetHeight) top = Math.min(top, el.offsetTop);
  }
  root.setProperty("--foot", Math.max(0, innerHeight - top) + "px");
  const hud = document.getElementById("hud");
  if (hud) root.setProperty("--hud-h", hud.offsetHeight + "px");
}
if (window.ResizeObserver) {
  const ro = new window.ResizeObserver(() => placeFoot());
  for (const el of [detailEl, dockLEl, dockREl, stripEl, document.getElementById("hud")]) if (el) ro.observe(el);
}
addEventListener("resize", placeFoot);

// --- Search (docs/design-system.md Section 8, U6) -------------------------------------------------
// The field asks /api/search, which calls the engine function behind MCP's search_knowledge, and the
// list is what it answers, in its order: entities, then observations, under the label of the surface
// that answered (P19). No ranking is made here - a second one would answer a person differently from
// the agent beside them. Entity ids arrive canonical, so a hit is always a node the map can draw.
const searchWrap = searchEl.closest(".searchwrap");
const searchPop = document.getElementById("searchPop");
const SEARCH_MODE = {
  hybrid: ["keyword + meaning", "Searched the words and what they mean."],
  keyword: ["keyword only", "Searched the words only: there is no meaning index here, so a miss may be one the words did not catch."],
};
let searchAns = null;    // { q, ws, mode, hits, note } or { q, ws, error }
const SEARCH_LIMIT = 20;  // the path's default limit: an answer this long may have been cut (P5)
// The last five queries this page ran, newest first, offered by an empty field (Section 29.11). They
// live in the page's memory only, never in the browser's storage.
const recentQueries = [];
function noteRecent(q) {
  const at = recentQueries.indexOf(q);
  if (at >= 0) recentQueries.splice(at, 1);
  recentQueries.unshift(q);
  recentQueries.length = Math.min(recentQueries.length, 5);
}
// An empty field that has focus lists the recent queries instead of an answer.
function recentShown() { return !searchQuery() && recentQueries.length > 0; }
let searchRows = [];     // the hits in the order the list shows them
let searchSel = -1, searchSeq = 0, searchTimer = 0;   // -1 = nothing chosen yet

function searchQuery() { return searchEl.value.trim(); }
// An answer is shown only for the text in the field and the scope on the map; a workspace switch
// asks again rather than listing what another scope held.
function searchFresh() { return !!searchAns && searchAns.q === searchQuery() && searchAns.ws === currentWs(); }
function setSearchOpen(on) {
  if (on && recentShown()) { renderSearch(); }
  on = on && (searchFresh() || recentShown());
  searchPop.hidden = !on;
  searchEl.setAttribute("aria-expanded", on ? "true" : "false");
  searchLit = on && searchFresh() && searchAns.hits
    ? new Set(searchAns.hits.filter(h => h.kind === "entity").map(h => h.id))
    : null;
  requestFrame();
}
async function runSearch() {
  const q = searchQuery();
  const seq = ++searchSeq;
  // An emptied field offers the recent queries again while it has focus.
  if (!q) { searchAns = null; renderSearch(); setSearchOpen(document.activeElement === searchEl); return; }
  // The scope reads as the map's: the same workspace parameter /api/graph is asked with.
  const ws = currentWs();
  let ans;
  try {
    const r = await fetch("/api/search?q=" + encodeURIComponent(q) + (ws ? "&workspace=" + encodeURIComponent(ws) : ""), { cache: "no-store" });
    const body = await r.json().catch(() => ({}));
    ans = r.ok ? { q, ws, mode: body.mode, hits: body.hits || [], note: body.note }
      : { q, ws, error: "search failed (" + r.status + ")" + (body.error ? " - " + body.error : "") };
  } catch (e) {
    ans = { q, ws, error: "search failed - the daemon did not answer" };
  }
  if (seq !== searchSeq) return;   // a newer query is on its way; this answer is for text no longer there
  searchAns = ans;
  if (!ans.error) noteRecent(q);
  searchSel = -1;   // the person has not moved yet - no row is chosen
  renderSearch();
  setSearchOpen(document.activeElement === searchEl);
  // Nothing here contains the words: count what the wider scope holds, without holding up the list.
  if (!ans.error && ws !== "*" && ws !== "all" && !ans.hits.some(h => h.reached === "words")) {
    const r = await searchWider(q);
    if (r && seq === searchSeq && searchAns === ans) { ans.wider = r.n; ans.widerCapped = r.capped; renderSearch(); }
  }
}
// How many items contain the words across every workspace the reader may read - the count the
// wider-scope row offers. The path's limit cuts the list, so an answer at the limit says "20+".
async function searchWider(q) {
  try {
    const r = await fetch("/api/search?q=" + encodeURIComponent(q) + "&workspace=*", { cache: "no-store" });
    if (!r.ok) return null;
    const hits = (await r.json()).hits || [];
    const n = hits.filter(h => h.reached === "words").length;
    return { n, capped: n >= 20 };   // the keyword list is cut at the limit before it is fused
  } catch (_) { return null; }
}
function searchNode(h) {
  const n = h.kind === "entity" ? nodeById(h.id) : null;
  return n && !typeOff.has(n.type) ? n : null;
}
function renderSearch() {
  if (recentShown()) {
    // An empty field: the queries this page ran, newest first; choosing one asks it again.
    searchRows = recentQueries.map(q => ({ kind: "recent", q }));
    searchSel = -1;
    searchPop.innerHTML = html`<div class="sp-sec">Recent</div>${recentQueries.map((q, i) =>
      html`<button type="button" class="sp-row" role="option" id="sp${i}" data-i="${i}" aria-selected="false"><span class="sp-name">${q}</span></button>`)}`;
    searchPop.querySelectorAll(".sp-row").forEach(b => {
      b.onmousedown = ev => ev.preventDefault();
      b.onclick = () => chooseSearch(Number(b.dataset.i));
    });
    searchEl.setAttribute("aria-activedescendant", "");
    return;
  }
  if (!searchAns) { searchRows = []; searchPop.innerHTML = html``; return; }
  if (searchAns.error) {
    searchRows = [];
    searchPop.innerHTML = html`<div class="sp-note bad">${searchAns.error}</div>`;
    return;
  }
  // Two groups, each in the engine's order (design-system.md Section 8, U15): the hits that contain
  // the words, then the related ones, each saying how it was reached.
  const words = searchAns.hits.filter(h => h.reached === "words");
  const related = searchAns.hits.filter(h => h.reached !== "words");
  // The answer kept on the map (Section 29.11, D8): the first row, when two or more of its entities
  // are drawn. It says "these", or "the first" when the answer reached the path's limit, and counts
  // apart the entity hits this view does not draw (P5).
  const ents = searchAns.hits.filter(h => h.kind === "entity");
  const drawn = ents.filter(h => searchNode(h)).map(h => h.id);
  const keep = drawn.length >= 2
    ? { kind: "keep", ids: drawn, q: searchAns.q, first: searchAns.hits.length >= SEARCH_LIMIT, apart: ents.length - drawn.length }
    : null;
  const off = keep ? 1 : 0;
  searchRows = [...(keep ? [keep] : []), ...words, ...related];
  const why = h => h.reached === "meaning" ? "near in meaning" : h.reached === "neighbor" ? "related to a match" : "";
  // Where the scope holds more than one workspace, an entity's row names its own in the hover card's
  // words, "Service in infra" (Section 29.11).
  const manyWs = new Set(nodes.map(n => n.workspace)).size > 1;
  const [modeWord, modeWhy] = SEARCH_MODE[searchAns.mode] || [String(searchAns.mode || "?"), ""];
  const row = (h, i) => {
    const on = i === searchSel ? " on" : "";
    const reason = why(h) ? html`<span class="sp-why">${why(h)}</span>` : "";
    if (h.kind === "observation")
      return html`<button type="button" class="sp-row${on}" role="option" id="sp${i}" data-i="${i}" aria-selected="${!!on}"><span class="sp-snip">${h.snippet}</span>${reason}</button>`;
    const n = searchNode(h);
    // An entity the answer holds but this view does not draw (its type is switched off, or the loaded
    // graph predates it) stays listed - it is part of the answer - and says why it cannot be focused.
    return html`<button type="button" class="sp-row${on}${n ? "" : " off"}" role="option" id="sp${i}" data-i="${i}" aria-selected="${!!on}">${
      n ? html`<span class="dot" style="background:${typeColor[n.type] || OTHER}"></span>` : ""}<span class="sp-name">${n ? n.name : h.snippet}</span>${
      n ? html`<span class="sp-k">${manyWs && n.workspace ? n.type + " in " + n.workspace : n.type}</span>` : html`<span class="sp-k">not on the map in this view</span>`}${reason}</button>`;
  };
  const count = searchAns.hits.length;
  const cur = currentWs();
  const all = cur === "*" || cur === "all";
  const scope = all ? "any workspace" : cur || defaultName();
  // Nothing in the scope contains the words: say so, and offer the wider scope with what it found.
  const widen = !all && searchAns.wider > 0
    ? html`<button type="button" class="sp-row sp-widen" data-widen="1">${"Search all workspaces (" + (searchAns.widerCapped ? "20+" : searchAns.wider) + " found)"}</button>`
    : "";
  const keepRow = keep ? html`<button type="button" class="sp-row sp-keep${searchSel === 0 ? " on" : ""}" role="option" id="sp0" data-i="0" aria-selected="${searchSel === 0}" aria-keyshortcuts="Meta+Enter"><span class="sp-name">${
    "Show these " + keep.ids.length + " on the map" + (keep.first ? " - the search stopped at " + SEARCH_LIMIT : "")}</span>${
    keep.apart ? html`<span class="sp-why">${keep.apart + (keep.apart === 1 ? " is" : " are") + " not drawn in this view"}</span>` : ""}<kbd class="sp-key">Cmd+Enter</kbd></button>` : "";
  const first = words.length
    ? [html`<div class="sp-sec">${"Contains \"" + searchAns.q + "\""}</div>`, words.map((h, i) => row(h, off + i))]
    : [html`<div class="sp-note">${"No entity in " + scope + " contains \"" + searchAns.q + "\"."}</div>`, widen];
  searchPop.innerHTML = html`<div class="sp-head"><span>${count} ${count === 1 ? "hit" : "hits"}</span><span class="sp-mode">${modeWord}</span></div><div class="sp-modewhy">${modeWhy}</div>${keepRow}${
    count || widen ? first : ""}${
    related.length ? [html`<div class="sp-sec">Related</div>`, related.map((h, i) => row(h, off + words.length + i))] : ""}${
    count ? "" : html`<div class="sp-note">${searchAns.note || "no hits - absence is unknown, not a negation"}</div>`}`;
  searchPop.querySelectorAll("[data-widen]").forEach(b => {
    b.onmousedown = ev => ev.preventDefault();
    b.onclick = () => { wsInput.value = "*"; beginWorkspaceTransition(); renderChipsActive(); poll(); runSearch(); };
  });
  searchPop.querySelectorAll(".sp-row:not(.sp-widen)").forEach(b => {
    // mousedown keeps the field focused, so the click below is the choice and not a blur first.
    b.onmousedown = ev => ev.preventDefault();
    b.onclick = () => chooseSearch(Number(b.dataset.i));
  });
  searchEl.setAttribute("aria-activedescendant", searchRows.length && searchSel >= 0 ? "sp" + searchSel : "");
}
function moveSearch(d) {
  if (!searchRows.length) return;
  // The first press chooses a row: down starts at the top, up at the bottom. After that it cycles.
  if (searchSel < 0) searchSel = d > 0 ? 0 : searchRows.length - 1;
  else searchSel = (searchSel + d + searchRows.length) % searchRows.length;
  searchPop.querySelectorAll(".sp-row").forEach(b => {
    const on = Number(b.dataset.i) === searchSel;
    b.classList.toggle("on", on);
    b.setAttribute("aria-selected", on ? "true" : "false");
    if (on) b.scrollIntoView({ block: "nearest" });
  });
  searchEl.setAttribute("aria-activedescendant", "sp" + searchSel);
}
async function chooseSearch(i) {
  const h = searchRows[i];
  if (!h) return;
  if (h.kind === "recent") {
    searchEl.value = h.q; searchWrap?.classList.toggle("filled", true);
    runSearch();
    return;
  }
  if (h.kind === "keep") {
    // The answer stays on the map as the working set; the field clears (D8), and from Review the
    // Graph opens.
    searchEl.value = ""; searchWrap?.classList.toggle("filled", false);
    closeSearch();
    if (reviewOpen()) location.hash = "";
    keepInView(h.ids, { kind: "search", q: h.q, first: h.first });
    return;
  }
  if (h.kind === "entity") {
    const n = searchNode(h);
    if (!n) return;   // listed, not drawn: the row already says so
    // Search goes somewhere (D8): the field clears, and the chosen name is the sheet's title.
    searchEl.value = ""; searchWrap?.classList.toggle("filled", false);
    closeSearch();
    if (reviewOpen()) location.hash = "";
    select(n);
    return;
  }
  // An observation opens its card - from the Log's rows when they hold it, else read once - and the
  // canvas frames the entities it asserts.
  const o = await observationById(h.id);
  if (!o) { toast("bad", "Could not open the observation", "the log of this scope did not hold " + h.id.slice(0, 10)); return; }
  searchEl.value = ""; searchWrap?.classList.toggle("filled", false);
  closeSearch();
  if (reviewOpen()) location.hash = "";
  showObsCard(o);
  const asserted = (o.entities || []).map(e => nodeById(e.id)).filter(Boolean);
  if (asserted.length) { fitView(asserted, 140); userMoved = true; }
}
function closeSearch() { setSearchOpen(false); searchEl.blur(); }

searchEl.addEventListener("input", () => {
  // Keep the "/" badge clear of the text once the field carries a term (CSS .searchwrap.filled).
  // Optional: the badge is decoration, so losing its wrapper must never break search itself.
  searchWrap?.classList.toggle("filled", searchEl.value !== "");
  // The list of a query no longer in the field is not shown, and the map stops lighting it.
  setSearchOpen(false);
  clearTimeout(searchTimer);
  searchTimer = setTimeout(runSearch, 160);
});
searchEl.addEventListener("focus", () => {
  if (searchQuery() && !searchFresh()) runSearch(); else setSearchOpen(true);
});
searchEl.addEventListener("blur", () => setSearchOpen(false));
searchEl.addEventListener("keydown", ev => {
  if (ev.key === "ArrowDown" || ev.key === "ArrowUp") {
    ev.preventDefault();
    if (!searchPop.hidden) moveSearch(ev.key === "ArrowDown" ? 1 : -1);
    else if (searchFresh()) setSearchOpen(true);
    else if (searchQuery()) runSearch();
  } else if (ev.key === "Enter" && (ev.metaKey || ev.ctrlKey)) {
    // Cmd+Enter keeps a fresh answer on the map; before the answer, it asks first (Section 29.11).
    ev.preventDefault();
    clearTimeout(searchTimer);
    if (searchFresh() && searchRows[0] && searchRows[0].kind === "keep") chooseSearch(0);
    else if (searchQuery() && !searchFresh()) runSearch();
  } else if (ev.key === "Enter") {
    ev.preventDefault();
    clearTimeout(searchTimer);
    // Enter on a chosen row picks it; otherwise ask first. No auto-pick of the top row.
    if ((searchFresh() || recentShown()) && searchSel >= 0) chooseSearch(searchSel);
    else if (searchQuery() && !searchFresh()) runSearch();
  } else if (ev.key === "Escape") {
    // Hand the keyboard back to the canvas, so "/" -> type -> Escape is a closed loop.
    // stopPropagation: the window handler reads Escape as "dismiss the obs card / view popover",
    // which is not what leaving the search field should do.
    ev.stopPropagation();
    closeSearch();
  }
});
// Retry where the map is stale (Section 29.12): the workspaces and the map at once; a Retry that fails
// too says so.
let retryAsked = null;   // when Retry was last pressed, until a read succeeds
const retryBtn = document.getElementById("retry");
retryBtn.onclick = () => {
  retryAsked = new Date().toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  loadWorkspaces(); poll();
};
// The zoom buttons zoom about the map window's centre, which is not the canvas's when a panel is open.
function zoomWindow(f) { const w = mapWindow(); zoomAt((w.l + w.r)/2, (w.t + w.b)/2, f); personMoved(); }
document.getElementById("zin").onclick = e => { if (idle(e)) return; zoomWindow(1.2); };
document.getElementById("zout").onclick = e => { if (idle(e)) return; zoomWindow(1/1.2); };
document.getElementById("fit").onclick = e => {
  if (idle(e)) return;
  userMoved = true;
  // With a working set pinned, Fit frames the set (Section 29.11).
  const kept = keptDrawn();
  fitView(kept.length ? kept : undefined); personMoved();
};
// Follow on the camera toolbar (Section 29.4): a press turns it on or off, waiting or not.
const followBtn = document.getElementById("followBtn");
followBtn.onclick = () => { follow = !follow; renderFollow(); };
setInterval(renderFollow, 1000);
document.getElementById("resetLayout").onclick = () => { setViewPop(false); resetLayout(); };
const peersBtn = document.getElementById("peersBtn");
peersBtn.onclick = () => { peersOn = !peersOn; setSwitch(peersBtn, peersOn); };
const clusterBtn = document.getElementById("clusterBtn");
clusterBtn.onclick = () => { clusterMode = !clusterMode; setSwitch(clusterBtn, clusterMode); wake(0.6); poll(); };
// The view menu's stored choices: each switch, the Regions choice and the single keys.
const VIEW_PREFS = ["followBtn", "clusterBtn", "labelsBtn", "edgesBtn", "arrowsBtn", "footBtn",
  "pulseBtn", "histBtn", "miniBtn", "keysBtn"];
let viewPrefsReady = false;   // false while stored choices are replayed, so the replay stores nothing
function saveViewPrefs() {
  if (!viewPrefsReady) return;
  const now = { v: 5, regions: regionsValue(), boundaries: boundaryMode };
  for (const k of VIEW_PREFS) { const x = document.getElementById(k); if (x) now[k] = k === "followBtn" ? follow : x.classList.contains("on"); }
  try { localStorage.setItem("supra.view", JSON.stringify(now)); } catch (_) { /* private mode - the choice lasts the session */ }
}
// Regions: All, Selected or Off (design-system.md Section 5) - one choice over the two switches it
// replaced, "hulls" and "all regions". Render-only (the draw loop shows it next frame), so the sim is
// not reheated; the hyperedges are fetched only if they are not already here.
const regionsSeg = document.getElementById("regionsSeg");
function regionsValue() { return !hyperMode ? "off" : allRegions ? "all" : "selected"; }
function setRegions(v) {
  hyperMode = v !== "off"; allRegions = v === "all";
  if (hyperMode && !hyperedges.length) poll();
  renderRegionsSeg(); saveViewPrefs(); requestFrame();
}
// What each choice of regions draws, said under the choice rather than in a tooltip (Section 4, U25).
const REGION_WHY = {
  all: "Every region faint, and the active entity's at full strength.",
  selected: "Only the regions of the entity you point at or select.",
  off: "No regions.",
};
function renderRegionsSeg() {
  renderMarks();
  for (const b of regionsSeg.querySelectorAll("button")) b.classList.toggle("on", b.dataset.v === regionsValue());
  radioGroup(regionsSeg, "Regions");
  const desc = document.getElementById("regionsDesc");
  if (desc) desc.textContent = REGION_WHY[regionsValue()] || "";
}
regionsSeg.querySelectorAll("button").forEach(b => { b.onclick = () => setRegions(b.dataset.v); });
// The single keys can be turned off (Section 10, WCAG 2.1.4); the switch is in the Keys dialog, beside
// the keys it governs (Section 29.12, the author's decision).
let singleKeys = true;
const keysBtn = document.getElementById("keysBtn");
keysBtn.onclick = () => { singleKeys = !singleKeys; setSwitch(keysBtn, singleKeys); renderKeyNames(); };
const keysDlg = document.getElementById("keysDlg");
// Same as... (Section 29.17): a search of the entity's own workspace that chooses nothing until the
// person moves; a chosen row offers the merge either way, and the merge opened waits on Review.
const sameDlg = document.getElementById("sameDlg"), sameQ = document.getElementById("sameQ");
const sameListEl = document.getElementById("sameList"), sameActsEl = document.getElementById("sameActs");
let sameFor = null, sameRows = [], sameSel = -1, sameChosen = null, sameSeq = 0, sameAsked = false;
function openSameAs(node) {
  sameFor = node; sameRows = []; sameSel = -1; sameChosen = null; sameAsked = false;
  document.getElementById("sameTitle").textContent = node.name + " is the same as...";
  const ph = "Search " + node.workspace;
  sameQ.value = ""; sameQ.placeholder = ph; sameQ.setAttribute("aria-label", ph);
  renderSame();
  if (!sameDlg.open) sameDlg.showModal();
  sameQ.focus();
}
function sameAlone() { return !nodes.some(n => n.workspace === sameFor.workspace && n.id !== sameFor.id); }
function renderSame() {
  if (!sameFor) return;
  const ws = sameFor.workspace, alone = sameAlone();
  sameQ.disabled = alone;
  const row = (h, i) => html`<button type="button" class="sr${i === sameSel ? " on" : ""}" role="option" id="sr${i}" data-i="${i}" aria-selected="${i === sameSel}"><span>${h.name}</span><span class="k">${h.type}</span></button>`;
  const words = sameRows.filter(h => h.reached === "words"), near = sameRows.filter(h => h.reached !== "words");
  const at = h => sameRows.indexOf(h);
  sameListEl.innerHTML = html`${alone ? html`<div class="none">Nothing else is in ${ws} yet.</div>`
    : sameAsked && !sameRows.length ? html`<div class="none">No entity in ${ws} matches.</div>`
    : html`${words.length ? html`<div class="sg">Contains "${sameQ.value.trim()}"</div>${words.map(h => row(h, at(h)))}` : ""}${
        near.length ? html`<div class="sg">Related</div>${near.map(h => row(h, at(h)))}` : ""}`}`;
  sameQ.setAttribute("aria-expanded", String(sameRows.length > 0));
  if (sameSel >= 0) sameQ.setAttribute("aria-activedescendant", "sr" + sameSel); else sameQ.removeAttribute("aria-activedescendant");
  sameListEl.querySelectorAll(".sr").forEach(b => { b.onclick = () => chooseSame(+b.dataset.i); });
  const c = sameChosen;
  sameActsEl.innerHTML = html`${c ? html`<button type="button" class="btn" data-a="${sameFor.id}" data-b="${c.id}">Merge ${sameFor.name} into ${c.name}</button><button type="button" class="btn" data-a="${c.id}" data-b="${sameFor.id}">Merge ${c.name} into ${sameFor.name}</button>` : ""}`;
  sameActsEl.querySelectorAll("[data-a]").forEach(b => { b.onclick = () => openSameMerge(b.dataset.a, b.dataset.b); });
}
async function searchSame() {
  const q = sameQ.value.trim(), seq = ++sameSeq;
  sameSel = -1; sameChosen = null;
  if (!q) { sameRows = []; sameAsked = false; renderSame(); return; }
  try {
    const r = await fetch("/api/search?q=" + encodeURIComponent(q) + "&workspace=" + encodeURIComponent(sameFor.workspace), { cache: "no-store" });
    const ans = await r.json();
    if (seq !== sameSeq) return;
    sameRows = (ans.hits || []).filter(h => h.kind === "entity" && h.id !== sameFor.id).map(h => {
      const n = nodeById(h.id);
      return { id: h.id, name: n ? n.name : h.snippet, type: n ? n.type : "", reached: h.reached };
    });
  } catch (e) { sameRows = []; }
  sameAsked = true;
  renderSame();
}
function chooseSame(i) { if (i < 0 || i >= sameRows.length) return; sameSel = i; sameChosen = sameRows[i]; renderSame(); }
async function openSameMerge(a, b) {
  const want = [a, b].sort().join("|");
  const open = proposals.find(q => q.kind === "entity_merge" && (q.state === "open" || q.state === "blocked")
    && [...new Set(q.targets || [])].sort().join("|") === want);
  if (open) { sameDlg.close(); toast("info", "A merge of these two already waits under Needs you."); openReviewItem(open.id); return; }
  const o = await consoleWrite("/api/propose_merge", "?a=" + encodeURIComponent(a) + "&b=" + encodeURIComponent(b) + "&src=picked&workspace=" + encodeURIComponent(sameFor.workspace), null, null, null, true);
  if (!o.ok) { toast("bad", "Not opened", o.why); return; }
  sameDlg.close();
  toast("ok", foundSaid(o.body, "merge") || "Merge proposed - it waits under Needs you for a verdict");
  await refreshProposals();
  openReviewItem(o.body.proposal_id);
}
sameQ.addEventListener("input", () => { searchSame(); });
sameQ.addEventListener("keydown", ev => {
  if (ev.key === "ArrowDown" && sameRows.length) { ev.preventDefault(); sameSel = Math.min(sameRows.length - 1, sameSel + 1); renderSame(); }
  else if (ev.key === "ArrowUp" && sameRows.length) { ev.preventDefault(); sameSel = Math.max(0, sameSel - 1); renderSame(); }
  else if (ev.key === "Enter") { ev.preventDefault(); if (sameSel >= 0) chooseSame(sameSel); }
});
// Esc closes from anywhere in the dialog and changes nothing; focus goes back to Same as...
sameDlg.addEventListener("close", () => { sameFor = null; const b = detailEl.querySelector(".sameas"); if (b) b.focus(); });
function openKeys() { setViewPop(false); if (!keysDlg.open) keysDlg.showModal(); }
document.getElementById("keysOpen").onclick = openKeys;
function dialogOpen() { return !!document.querySelector("dialog[open]"); }
// A control names its key only while the single keys are on (Section 9: one line never states two
// contradictory states).
function renderKeyNames() {
  const k = key => singleKeys ? key : null;
  const name = (id, label, key) => { const b = document.getElementById(id); if (b) b.title = tipText(label, k(key)); };
  name("zin", "Zoom in", "="); name("zout", "Zoom out", "-");
  name("mapTab", "Map", "l"); name("listBtn", "List", "l");
  name("panelL", "Filters panel", "["); name("panelR", "Log panel", "]");
  renderKept();   // Fit's name and key follow the working set
  for (const el of document.querySelectorAll(".slashkey, #viewpop .keyhint")) el.hidden = !singleKeys;
}
// Relations off leaves Arrows and History nothing to draw: they are dimmed and say why (Section 4).
function renderShowDeps() {
  for (const id of ["arrowsBtn", "histBtn"]) {
    const b = document.getElementById(id);
    if (!b) continue;
    if (showEdges) { b.removeAttribute("aria-disabled"); b.setAttribute("aria-describedby", id === "arrowsBtn" ? "dArrows" : "dHist"); }
    else { b.setAttribute("aria-disabled", "true"); b.setAttribute("aria-describedby", "dShownWithRel"); }
  }
  // More's summary names a switch under it that is off.
  const off = [["footBtn", "Footprint"], ["pulseBtn", "Pulses"]].filter(([id]) => !document.getElementById(id).classList.contains("on")).map(([, w]) => w);
  document.getElementById("moreSum").textContent = off.length ? "More - " + off.join(" and ") + " off" : "More";
}
// The panels and their title-bar toggles (docs/design-system.md Section 6): each panel opens and
// closes on its own, with its toggle or [ and ], and the choice persists across reloads. A panel is
// part of the frame beside the map's window - body.dl / body.dr move the window's edges - and
// toggling one never reheats the sim.
const panelLBtn = document.getElementById("panelL"), panelRBtn = document.getElementById("panelR");
function applyDock(isLeft, on, remember = true) {
  (isLeft ? dockLEl : dockREl).classList.toggle("on", on);
  (isLeft ? panelLBtn : panelRBtn).setAttribute("aria-pressed", on ? "true" : "false");
  document.body.classList.toggle(isLeft ? "dl" : "dr", on);
  if (isLeft) renderTierNote();
  else if (on) refreshLog();
  // Closing the Log fetches the strip that takes its place at once, not at the next poll.
  if (!isLeft && !on) refreshStrip();
  else renderStrip();
  placeFoot();
  // A phone's choice is not remembered, so the same browser in a wider window keeps its own (Section 29.18).
  if (remember && !isPhone()) {
    try { localStorage.setItem(isLeft ? "supra.dockL" : "supra.dockR", on ? "1" : "0"); } catch (_) { /* private mode */ }
  }
  // Under 1200px the canvas holds one panel at a time (design-system.md Section 6): opening a dock
  // closes the other and the inspector. Not remembered - the wide window's choice stands.
  if (on && narrowWindow()) {
    applyDock(!isLeft, false, false);
    clearSelection(false);
  }
}
function narrowWindow() { return innerWidth < 1200; }
// A phone (Section 29.18): a window under 760 pixels wide, or a touch screen whose shorter side is
// under 600 - a phone held sideways is one too.
const coarsePointer = matchMedia("(pointer: coarse)");
function isPhone() { return innerWidth < 760 || (coarsePointer.matches && Math.min(innerWidth, innerHeight) < 600); }
function toggleDock(isLeft) { applyDock(isLeft, !(isLeft ? dockLEl : dockREl).classList.contains("on")); }
panelLBtn.onclick = () => toggleDock(true);
panelRBtn.onclick = () => toggleDock(false);
// View options popover (HUD "view" button): preferences stay off the data rails; a click
// elsewhere or Escape dismisses it.
const viewBtn = document.getElementById("viewBtn"), viewPop = document.getElementById("viewpop");
function setViewPop(on) {
  if (!on && viewPop.contains(document.activeElement)) viewBtn.focus();
  viewPop.classList.toggle("on", on); viewBtn.classList.toggle("on", on);
  viewBtn.setAttribute("aria-expanded", on ? "true" : "false");
  if (on) { const first = viewPop.querySelector("button"); if (first) first.focus(); }
}
viewBtn.onclick = ev => { ev.stopPropagation(); setViewPop(!viewPop.classList.contains("on")); };
document.addEventListener("click", ev => { if (viewPop.classList.contains("on") && !viewPop.contains(ev.target)) setViewPop(false); });
// [ / ] toggle the docks, "/" jumps to search - but never while typing in a field.
window.addEventListener("keydown", e => {
  // Cmd+K (Ctrl+K off the Mac) opens search from anywhere, a field included - it types nothing.
  if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === "k") {
    e.preventDefault();
    searchEl.focus();
    searchEl.select();
    return;
  }
  if (e.target && (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA")) return;
  // A dialog holds the keyboard: its own Esc closes it alone (Section 10).
  if (dialogOpen()) return;
  // Cmd+[ and Cmd+] step through the selections on the Graph page (Section 29.4); the Review map
  // preview's own way back is a capturing listener and comes first while its bar shows.
  if ((e.key === "[" || e.key === "]") && (e.metaKey || e.ctrlKey) && !e.altKey && !reviewOpen()) {
    e.preventDefault();
    stepSelection(e.key === "[" ? -1 : 1);
    return;
  }
  // The single keys (Section 29.12): without Cmd, Ctrl or Option; a letter by what it types, and by
  // the key's place only when the input source types a letter that is not Latin (b and l under Hangul).
  const bare = !e.metaKey && !e.ctrlKey && !e.altKey;
  const k = /^[\x20-\x7e]$/.test(e.key) ? e.key : e.code === "KeyB" ? "b" : e.code === "KeyL" ? "l" : e.key;
  const single = bare && ["[", "]", "/", "b", "=", "+", "-", "0", "l", "?"].includes(k);
  if (single && !singleKeys) return;   // turned off in the Keys dialog (WCAG 2.1.4)
  // "/" opens search on the Review page too; the map's keys act only on the Graph page with no card open.
  const onMap = !reviewOpen() && !obscardEl.classList.contains("on");
  const press = id => { const b = document.getElementById(id); if (b && b.getAttribute("aria-disabled") !== "true") b.click(); };
  if (single && k === "/") {
    // preventDefault or the "/" itself lands in the field as the first character (and Firefox
    // opens its quick-find). The INPUT/TEXTAREA guard above already keeps this from hijacking
    // a "/" typed into any other field. select() so the shortcut replaces a stale term.
    e.preventDefault();
    searchEl.focus();
    searchEl.select();
  }
  else if (single && !onMap) return;
  else if (single) {
    e.preventDefault();
    if (k === "[") toggleDock(true);
    else if (k === "]") toggleDock(false);
    else if (k === "=" || k === "+") press("zin");
    else if (k === "-") press("zout");
    else if (k === "0") press("fit");
    else if (k === "l") press(listOn ? "mapTab" : "listBtn");
    else if (k === "?") openKeys();
    else if (k === "b") cycleBoundaries();
  }
  // Esc closes the topmost thing: the observation card, then the view menu, then the inspector.
  else if (e.key === "Escape") {
    if (listOn) setList(false);   // List goes back to Map first, from the table or its tab (Section 29.11)
    else if (obscardEl.classList.contains("on")) { if (demoteAsk && cardObs) closeDemote(cardObs); else hideObsCard(); }
    else if (viewPop.classList.contains("on")) setViewPop(false);
    else if (focus) clearSelection(true);   // and the camera goes back to before the walk (Section 29.4)
    else if (workSet) clearWorkSet();       // the working set ends last (Section 29.11)
  }
});
// Restore the persisted state (default: both open).
{
  let keptL = true, keptR = true;
  try {
    keptL = localStorage.getItem("supra.dockL") !== "0";
    keptR = localStorage.getItem("supra.dockR") !== "0";
  } catch (_) { /* private mode - the defaults stand */ }
  // A phone opens on the map (D16, Section 29.18).
  if (isPhone()) keptL = keptR = false;
  applyDock(true, keptL, false);
  applyDock(false, keptR, false);
}
// Fetch the glossary lazily when its section is expanded (and keep it fresh via poll while open).
// Dock tabs: clicking a tab shows only that panel (one at a time, fixed-height body - no upward growth),
// and refreshes the newly active data panel.
document.querySelectorAll(".dock .tabs").forEach(tabs => {
  const dock = tabs.closest(".dock");
  for (const t of tabs.querySelectorAll(".tab")) {
    t.id = "tab-" + t.dataset.tab;
    const panel = dock.querySelector(`.tabpanel[data-panel="${t.dataset.tab}"]`);
    if (panel) { panel.setAttribute("role", "tabpanel"); panel.setAttribute("aria-labelledby", t.id); }
  }
  markTabs(tabs, [...tabs.querySelectorAll(".tab")], tabs.querySelector(".tab.on"));
  arrowTabs(tabs);
  tabs.addEventListener("click", ev => {
    const btn = ev.target.closest(".tab");
    if (!btn) return;
    tabs.querySelectorAll(".tab").forEach(t => t.classList.toggle("on", t === btn));
    markTabs(tabs, [...tabs.querySelectorAll(".tab")], btn);
    dock.querySelectorAll(".tabpanel").forEach(p => p.classList.toggle("on", p.dataset.panel === btn.dataset.tab));
    if (btn.dataset.tab === "review") refreshCuration();
    else if (btn.dataset.tab === "proposals") refreshProposals();
    else if (btn.dataset.tab === "log") refreshLog();
  });
});
arrowTabs(document.getElementById("settingsTabs"));
// Pure render toggles (no layout/data change - rAF reflects them every frame, so wake/poll is unnecessary).
document.getElementById("labelsBtn").onclick = e => { showLabels = !showLabels; setSwitch(e.currentTarget, showLabels); };
document.getElementById("edgesBtn").onclick = e => { showEdges = !showEdges; setSwitch(e.currentTarget, showEdges); renderShowDeps(); };
// Arrows and History act on relations: while Relations is off they are idle (a stored choice still replays).
document.getElementById("arrowsBtn").onclick = e => { if (viewPrefsReady && idle(e)) return; showArrows = !showArrows; setSwitch(e.currentTarget, showArrows); };
document.getElementById("footBtn").onclick = e => { showFootprint = !showFootprint; setSwitch(e.currentTarget, showFootprint); renderMarks(); renderShowDeps(); };
document.getElementById("pulseBtn").onclick = e => { showPulses = !showPulses; setSwitch(e.currentTarget, showPulses); renderMarks(); renderShowDeps(); };
document.getElementById("histBtn").onclick = e => { if (viewPrefsReady && idle(e)) return; showSuperseded = !showSuperseded; setSwitch(e.currentTarget, showSuperseded); };
document.getElementById("settingsBtn").onclick = openSettings;
// The sync pill opens the settings at Sync. In the app the shell takes this click to the app's own
// Settings > Sync page, as it takes the gear's.
document.getElementById("syncPill").onclick = () => { settingsTab = "sync"; openSettings(); };
document.getElementById("miniBtn").onclick = e => { showMini = !showMini; setSwitch(e.currentTarget, showMini); };

// The review pill opens the Review page, where contested beliefs wait under Needs you.
document.querySelector("#kept .kx").onclick = clearWorkSet;
document.getElementById("showAll").onclick = showAll;
document.getElementById("reviewPill").onclick = () => { reviewSeg = "needs"; location.hash = "#review"; };
// The view menu's choices persist, as the docks' open state does (docs/design-system.md Section 14
// step 1): a choice made once stays made. A preference, so it lives in this browser and nowhere else.
// peers is left out on purpose: it switches itself on for a hub, so a stored "off" would fight that.
{
  for (const b of document.querySelectorAll("button.tog")) setSwitch(b, b.classList.contains("on"));
  let saved = {};
  try { saved = JSON.parse(localStorage.getItem("supra.view") || "{}") || {}; } catch (_) { saved = {}; }
  // Any click stored every choice, so a stored minimap is rarely one anyone made: 0.6.0 and 0.6.1
  // started it off and stored that on the first click of any toggle. Forget it once more now that it
  // starts on (docs/design-system.md Section 6), and keep it from the next click. "all regions" is
  // the same case from 0.6.4, when it starts on: a stored "off" before then is forgotten once.
  if (!(saved.v >= 3)) delete saved.miniBtn;
  if (!(saved.v >= 4)) delete saved.regionsBtn;
  // Before 0.7.0 Regions was two switches, "hulls" and "all regions"; their stored choices become the
  // one choice now (Section 5).
  if (!saved.regions && (typeof saved.hyperBtn === "boolean" || typeof saved.regionsBtn === "boolean")) {
    saved.regions = saved.hyperBtn === false ? "off" : saved.regionsBtn === false ? "selected" : "all";
  }
  viewPrefsReady = false;
  for (const id of VIEW_PREFS) {
    const b = document.getElementById(id);
    if (!b) continue;
    // Replay a stored choice through the button's own handler, so its state and side effects agree.
    const on = id === "followBtn" ? follow : b.classList.contains("on");   // Follow's look also says "paused"
    if (typeof saved[id] === "boolean" && saved[id] !== on) b.click();
    b.addEventListener("click", saveViewPrefs);
  }
  if (["all", "selected", "off"].includes(saved.regions)) setRegions(saved.regions);
  else renderRegionsSeg();
  // Boundaries is stored since it only draws (Section 29.12).
  if (["workspace", "origin", "off"].includes(saved.boundaries)) { boundaryMode = saved.boundaries; renderBoundaryChips(); }
  viewPrefsReady = true;
  renderKeyNames(); renderShowDeps();
}

// Restore the workspace selection from the URL (deep link / reload) before the first poll.
{
  const qws = new URLSearchParams(location.search).get("workspace");
  if (qws !== null) wsInput.value = qws;
}
resize(); syncSearchScope(); loadWorkspaces(); loadSurface().then(decideFirstView).finally(poll); connectEvents(); detectServerMode();
setInterval(poll, 2500); setInterval(loadWorkspaces, 5000); setInterval(refreshPeerRoster, 5000); draw();
loadSurface().then(() => { refreshSyncPill(); renderReview(); syncSearchScope(); loadWorkspaces(); renderStats(); }); setInterval(refreshSyncPill, 5000);
